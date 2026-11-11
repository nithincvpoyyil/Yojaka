//! Similar tier: pictures that look alike (resized or re-compressed copies).
//!
//! Each picture is reduced to two numbers:
//!
//! - a 64-bit *difference hash*: the picture is shrunk to 9×8 grey cells and each bit says
//!   whether a cell is brighter than its right-hand neighbour. Copies of one picture differ in
//!   only a few bits. This is the shape of the light and dark areas, without colour.
//! - a *colour signature*: the average red, green and blue of each quarter of the picture.
//!   Two pictures with the same light-and-dark shape but other colours are not copies. One
//!   more bit of that number says whether the upright picture is portrait ([`TALL`]).
//!
//! Both are kept in the index (`file.perceptual_hash`, `file.color_signature`; a rescan clears
//! them when the file changes). Hashes are compared with a BK-tree.
//!
//! Pictures are turned upright before they are measured (EXIF orientation, or the HEIC
//! rotation), so a photo stored sideways with a rotation flag matches its upright copy. That
//! is how iPhones store portrait photos.
//!
//! Decoders ([`decoder`]): the `image` crate for JPEG, PNG, GIF, WebP, TIFF and BMP; macOS
//! ImageIO ([`crate::sys::imageio`]) for HEIC, AVIF and raw photos. A raw photo is measured on
//! the camera's own preview inside the file ([`Pixels::RawPreview`]), so it matches the JPEG the
//! camera saved next to it.
//!
//! This is the only stage of Yojaka that decodes pixels, so it is opt-in. Files are opened
//! read-only; cloud-only files are never opened.

use std::collections::HashMap;
use std::io::Cursor;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use image::ImageDecoder;
use rayon::prelude::*;
use rusqlite::Connection;

use super::exact::Run;
use crate::model::Format;
use crate::sys::imageio::{self, Pixels};
use crate::{CoreEvent, IssueKind, Result, TaskKind};

/// Every format compared here; [`decoder`] says how each is read.
const COMPARED: [Format; 15] = {
    use Format::*;
    [
        Jpeg, Png, Gif, Webp, Tiff, Bmp, Heic, Avif, RawDng, RawCr2, RawCr3, RawNef, RawArw,
        RawRaf, RawOrf,
    ]
};

/// How a picture is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decoder {
    /// The `image` crate.
    Image,
    /// macOS ImageIO, told the file's type: it takes most raw files for plain TIFF otherwise.
    ImageIo {
        type_id: &'static str,
        pixels: Pixels,
    },
}

/// How a picture of `format` is decoded; `None` for videos.
fn decoder(format: Format) -> Option<Decoder> {
    use Format::*;
    match format {
        Jpeg | Png | Gif | Webp | Tiff | Bmp => Some(Decoder::Image),
        Heic | Avif | RawDng | RawCr2 | RawCr3 | RawNef | RawArw | RawRaf | RawOrf => {
            Some(Decoder::ImageIo {
                type_id: imageio::type_id(format)?,
                pixels: if format.is_raw() {
                    Pixels::RawPreview
                } else {
                    Pixels::Full
                },
            })
        }
        Mov | Mp4 | M4v | ThreeGp | Avi | Mkv | Webm | Mts => None,
    }
}

/// The compared formats as the index names them, for SQL: `'jpeg','png',...`.
fn formats_sql() -> String {
    COMPARED
        .iter()
        .map(|f| format!("'{}'", f.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Saved hashes made by an older way of measuring are thrown away and made again. Raise this
/// whenever the hash or colour signature of a picture changes (adding a format does not).
/// 2: pictures turned upright first; HEIC, AVIF and raw decoded.
const LOOK_VERSION: &str = "2";
/// ImageIO shrinks a picture to this longer side before it is measured. The 9×8 cells and the
/// four quarters are averages, so a smaller copy gives the same numbers, faster.
const IMAGEIO_MAX_SIDE: usize = 512;
/// Rows fetched, hashed and saved at a time.
const PAGE: usize = 256;
/// A hash with this few bits set (or unset) comes from a picture with almost no detail, such as
/// a blank scan. Those all look "alike", so they are not compared.
const MIN_DETAIL_BITS: u32 = 3;
/// Copies keep their shape: sides may differ by this much (rounding when resizing).
const ASPECT_TOLERANCE: f64 = 0.03;

/// Why a picture has no hash.
#[derive(Debug)]
pub enum HashError {
    Io(std::io::Error),
    /// Not a picture that can be decoded (broken, or an unsupported variant).
    Decode(String),
}

/// How a picture looks: see the module text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub hash: u64,
    /// The colour signature (60 bits), plus [`TALL`].
    pub color: u64,
}

/// Set in [`Look::color`] when the upright picture is clearly taller than wide. The saved width
/// and height are as stored (a portrait photo can be stored sideways), so the shape check takes
/// which way up a picture is from here and its proportions from the saved size.
pub(super) const TALL: u64 = 1 << 60;

/// Decodes the picture at `path`, a file of `format` (the first frame of an animation; the
/// camera's preview of a raw photo), turns it upright and measures its look.
pub fn look_file(path: &Path, format: Format) -> std::result::Result<Look, HashError> {
    let Some(how) = decoder(format) else {
        return Err(HashError::Decode(format!(
            "{} is not a picture format",
            format.as_str()
        )));
    };
    // Never through a symlink, like the metadata pass.
    let mut file = crate::sys::macos::open_nofollow(path).map_err(HashError::Io)?;
    match how {
        Decoder::ImageIo { type_id, pixels } => {
            // ImageIO reads only the parts of the file it needs.
            let pic = imageio::decode_upright(&file, type_id, pixels, IMAGEIO_MAX_SIDE)
                .map_err(HashError::Decode)?;
            Ok(look(&pic.pixels, pic.width, pic.height))
        }
        Decoder::Image => {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut bytes).map_err(HashError::Io)?;
            look_with_image_crate(&bytes)
        }
    }
}

fn look_with_image_crate(bytes: &[u8]) -> std::result::Result<Look, HashError> {
    let decode_err = |e: image::ImageError| match e {
        image::ImageError::IoError(io) => HashError::Io(io),
        other => HashError::Decode(other.to_string()),
    };
    let mut decoder = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(HashError::Io)?
        .into_decoder()
        .map_err(decode_err)?;
    // A missing or unreadable orientation tag means the picture is stored upright.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(decode_err)?;
    img.apply_orientation(orientation);
    let rgb = img.into_rgb8();
    Ok(look(
        rgb.as_raw(),
        rgb.width() as usize,
        rgb.height() as usize,
    ))
}

/// Measures an RGB picture (3 bytes per pixel, row by row).
fn look(rgb: &[u8], w: usize, h: usize) -> Look {
    // Integer luma (ITU-R 601 weights, scaled by 256).
    let grey: Vec<u8> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|[r, g, b]| {
            ((77 * u32::from(*r) + 150 * u32::from(*g) + 29 * u32::from(*b)) >> 8) as u8
        })
        .collect();
    // Nearly square pictures count as not tall, so rounding when resizing cannot flip it.
    let tall = h as f64 > w as f64 * (1.0 + ASPECT_TOLERANCE);
    Look {
        hash: dhash(&grey, w, h),
        color: color_signature(rgb, w, h) | if tall { TALL } else { 0 },
    }
}

/// The difference hash of the picture at `path`, a file of `format`.
pub fn dhash_file(path: &Path, format: Format) -> std::result::Result<u64, HashError> {
    look_file(path, format).map(|l| l.hash)
}

/// Levels of each colour average (5 bits).
const COLOR_STEPS: u64 = 32;

/// Average red, green and blue of the four quarters of an RGB picture, 5 bits each (12 values,
/// 60 bits; the first value is in the lowest bits).
pub(super) fn color_signature(rgb: &[u8], w: usize, h: usize) -> u64 {
    if w == 0 || h == 0 || rgb.len() < w * h * 3 {
        return 0;
    }
    let mut sum = [[0u64; 3]; 4];
    let mut count = [0u64; 4];
    for y in 0..h {
        let qy = usize::from(y * 2 >= h);
        for x in 0..w {
            let q = qy * 2 + usize::from(x * 2 >= w);
            let p = &rgb[(y * w + x) * 3..(y * w + x) * 3 + 3];
            for c in 0..3 {
                sum[q][c] += u64::from(p[c]);
            }
            count[q] += 1;
        }
    }
    let mut sig = 0u64;
    let mut shift = 0;
    let mut last = [0u64; 3];
    for q in 0..4 {
        for c in 0..3 {
            // A picture one pixel wide or high has empty quarters; they repeat the last one.
            let level = sum[q][c]
                .checked_div(count[q])
                .map_or(last[c], |average| average * COLOR_STEPS / 256);
            last[c] = level;
            sig |= level << shift;
            shift += 5;
        }
    }
    sig
}

/// Copies keep their colours: every one of the 12 averages is within one step (of 32).
pub(super) fn same_colors(a: u64, b: u64) -> bool {
    (0..12).all(|i| {
        let (x, y) = ((a >> (i * 5)) & 31, (b >> (i * 5)) & 31);
        x.abs_diff(y) <= 1
    })
}

/// Difference hash of a grey picture (`w * h` bytes, row by row).
pub(super) fn dhash(grey: &[u8], w: usize, h: usize) -> u64 {
    const COLS: usize = 9;
    const ROWS: usize = 8;
    if w == 0 || h == 0 || grey.len() < w * h {
        return 0;
    }
    // Average brightness of each of the 9×8 cells (every pixel belongs to exactly one cell).
    let mut sum = [[0u64; COLS]; ROWS];
    let mut count = [[0u64; COLS]; ROWS];
    for y in 0..h {
        let row = &grey[y * w..(y + 1) * w];
        let cy = y * ROWS / h;
        for (x, &v) in row.iter().enumerate() {
            let cx = x * COLS / w;
            sum[cy][cx] += u64::from(v);
            count[cy][cx] += 1;
        }
    }
    // A picture narrower than 9 pixels leaves cells empty; they borrow from the left.
    let mut cells = [[0u64; COLS]; ROWS];
    for cy in 0..ROWS {
        for cx in 0..COLS {
            // Fixed point: keeps differences smaller than one grey level.
            cells[cy][cx] = match (sum[cy][cx] * 16).checked_div(count[cy][cx]) {
                Some(average) => average,
                None if cx > 0 => cells[cy][cx - 1],
                None if cy > 0 => cells[cy - 1][cx],
                None => 0,
            };
        }
    }
    let mut bits = 0u64;
    for row in &cells {
        for cx in 0..COLS - 1 {
            bits = (bits << 1) | u64::from(row[cx] > row[cx + 1]);
        }
    }
    bits
}

/// A BK-tree over 64-bit hashes with the Hamming distance (number of differing bits).
#[derive(Debug, Default)]
pub struct BkTree {
    nodes: Vec<Node>,
}

#[derive(Debug)]
struct Node {
    hash: u64,
    /// Every picture with exactly this hash.
    items: Vec<u32>,
    /// (distance to this node, node index)
    children: Vec<(u8, u32)>,
}

impl BkTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn insert(&mut self, hash: u64, item: u32) {
        if self.nodes.is_empty() {
            self.nodes.push(Node {
                hash,
                items: vec![item],
                children: Vec::new(),
            });
            return;
        }
        let mut at = 0usize;
        loop {
            let d = (self.nodes[at].hash ^ hash).count_ones() as u8;
            if d == 0 {
                self.nodes[at].items.push(item);
                return;
            }
            match self.nodes[at].children.iter().find(|(cd, _)| *cd == d) {
                Some(&(_, child)) => at = child as usize,
                None => {
                    let new = self.nodes.len() as u32;
                    self.nodes[at].children.push((d, new));
                    self.nodes.push(Node {
                        hash,
                        items: vec![item],
                        children: Vec::new(),
                    });
                    return;
                }
            }
        }
    }

    /// Calls `found(item, distance)` for every item within `max` bits of `hash`.
    pub fn within(&self, hash: u64, max: u32, mut found: impl FnMut(u32, u32)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(at) = stack.pop() {
            let node = &self.nodes[at as usize];
            let d = (node.hash ^ hash).count_ones();
            if d <= max {
                for &item in &node.items {
                    found(item, d);
                }
            }
            let (lo, hi) = (d.saturating_sub(max), d + max);
            for &(cd, child) in &node.children {
                if (lo..=hi).contains(&u32::from(cd)) {
                    stack.push(child);
                }
            }
        }
    }
}

struct Todo {
    id: i64,
    path: PathBuf,
    size: u64,
    mtime_ns: i64,
    /// As the index names it (one of [`COMPARED`]).
    format: String,
}

enum Outcome {
    Hash(Look),
    /// Changed, missing or cloud-only since the scan: reported, not opened.
    Skip(IssueKind, String),
    Unusable,
}

fn hash_one(t: &Todo) -> Outcome {
    let md = match std::fs::symlink_metadata(&t.path) {
        Ok(md) => md,
        Err(e) => return Outcome::Skip(IssueKind::from_io(e.kind()), e.to_string()),
    };
    let mtime = md.mtime() * 1_000_000_000 + md.mtime_nsec();
    if !md.is_file() || md.len() != t.size || mtime != t.mtime_ns {
        return Outcome::Skip(
            IssueKind::Changed,
            "changed since the last scan; rescan to include it".into(),
        );
    }
    if crate::sys::macos::is_dataless(&md) {
        return Outcome::Skip(
            IssueKind::Changed,
            "now stored only in iCloud; rescan to update it".into(),
        );
    }
    let Some(format) = COMPARED.iter().copied().find(|f| f.as_str() == t.format) else {
        return Outcome::Unusable;
    };
    match look_file(&t.path, format) {
        Ok(look) => Outcome::Hash(look),
        Err(HashError::Io(e)) => Outcome::Skip(IssueKind::from_io(e.kind()), e.to_string()),
        Err(HashError::Decode(why)) => {
            tracing::debug!(path = %t.path.display(), "dupes: picture not decoded: {why}");
            Outcome::Unusable
        }
    }
}

/// Decodes and hashes every supported picture that has no saved hash yet.
fn hash_missing(run: &mut Run<'_>) -> Result<()> {
    let base = format!(
        "FROM file f WHERE f.media_kind = 'image' AND f.is_cloud_only = 0 AND f.size_bytes > 0 \
         AND f.perceptual_hash IS NULL AND f.format IN ({})",
        formats_sql()
    );
    let total: i64 = run
        .conn
        .query_row(&format!("SELECT COUNT(*) {base}"), [], |r| r.get(0))?;
    let total = u64::try_from(total).unwrap_or(0);
    let mut done = 0u64;
    let mut after = 0i64;
    let mut last_emit = Instant::now();
    loop {
        run.cancel.check()?;
        let page: Vec<Todo> = {
            let mut stmt = run.conn.prepare_cached(&format!(
                "SELECT f.id, f.full_path, f.size_bytes, f.modified_at_ns, f.format {base} AND f.id > ?1 \
                 ORDER BY f.id LIMIT {PAGE}"
            ))?;
            stmt.query_map([after], |r| {
                Ok(Todo {
                    id: r.get(0)?,
                    path: PathBuf::from(std::ffi::OsString::from_vec(r.get::<_, Vec<u8>>(1)?)),
                    size: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                    mtime_ns: r.get(3)?,
                    format: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        let Some(last) = page.last() else {
            break;
        };
        after = last.id;
        let outcomes: Vec<Outcome> = run.pool.install(|| page.par_iter().map(hash_one).collect());

        let tx = run.conn.transaction()?;
        {
            // Guarded by size and mtime so a rescan that raced with us is not overwritten.
            let mut save = tx.prepare(
                "UPDATE file SET perceptual_hash = ?1, color_signature = ?5 WHERE id = ?2 \
                 AND size_bytes = ?3 AND modified_at_ns = ?4",
            )?;
            for (t, o) in page.iter().zip(&outcomes) {
                if let Outcome::Hash(h) = o {
                    save.execute(rusqlite::params![
                        h.hash as i64,
                        t.id,
                        i64::try_from(t.size).unwrap_or(i64::MAX),
                        t.mtime_ns,
                        h.color as i64
                    ])?;
                }
            }
        }
        tx.commit()?;
        for (t, o) in page.iter().zip(outcomes) {
            match o {
                Outcome::Hash(_) => {
                    run.stats.pictures_hashed += 1;
                    run.stats.bytes_read += t.size;
                    run.stats.picture_bytes += t.size;
                }
                Outcome::Skip(kind, msg) => run.issue(&t.path, kind, msg),
                Outcome::Unusable => run.stats.pictures_unusable += 1,
            }
        }
        done += page.len() as u64;
        if last_emit.elapsed().as_millis() >= 100 || done >= total {
            last_emit = Instant::now();
            run.sink.progress(CoreEvent::Progress {
                task: TaskKind::Hash,
                done,
                total: Some(total),
                bytes: run.stats.bytes_read,
                current: page.last().map(|t| t.path.clone()),
            });
        }
    }
    Ok(())
}

/// Smallest set of merges: `find` with path halving.
fn root(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

/// Long side over short side, when both are known. The saved width and height are as stored,
/// and a photo stored sideways with a rotation flag must have the same shape as its upright
/// copy. Which way up a picture is comes from [`TALL`], measured on the upright picture.
fn aspect(w: Option<i64>, h: Option<i64>) -> Option<f64> {
    match (w, h) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            let (w, h) = (w as f64, h as f64);
            Some(w.max(h) / w.min(h))
        }
        _ => None,
    }
}

/// Throws away saved hashes made by an older way of measuring (see [`LOOK_VERSION`]).
fn forget_old_looks(conn: &mut Connection) -> Result<()> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta_kv WHERE key='picture_look_version'",
            [],
            |r| r.get(0),
        )
        .ok();
    if stored.as_deref() == Some(LOOK_VERSION) {
        return Ok(());
    }
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE file SET perceptual_hash = NULL, color_signature = NULL \
         WHERE perceptual_hash IS NOT NULL OR color_signature IS NOT NULL",
        [],
    )?;
    tx.execute(
        "INSERT INTO meta_kv(key,value) VALUES('picture_look_version',?1) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [LOOK_VERSION],
    )?;
    tx.commit()?;
    Ok(())
}

/// One hashed picture: file id, difference hash, colour signature, long side over short side.
pub(super) type Pic = (i64, u64, u64, Option<f64>);

/// Groups of file ids whose hashes are within `max_bits` of each other (linked through a chain
/// of such pairs) and whose colours and shapes agree.
pub(super) fn cluster(pics: &[Pic], max_bits: u32) -> Vec<Vec<i64>> {
    let mut tree = BkTree::new();
    for (i, p) in pics.iter().enumerate() {
        tree.insert(p.1, i as u32);
    }
    let mut parent: Vec<u32> = (0..pics.len() as u32).collect();
    for (i, p) in pics.iter().enumerate() {
        let i = i as u32;
        tree.within(p.1, max_bits, |j, _| {
            if j <= i {
                return;
            }
            let other = &pics[j as usize];
            let same_way_up = (p.2 & TALL) == (other.2 & TALL);
            let same_shape = same_way_up
                && match (p.3, other.3) {
                    (Some(a), Some(b)) => (a - b).abs() <= ASPECT_TOLERANCE * a.max(b),
                    _ => true,
                };
            if same_shape && same_colors(p.2, other.2) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                if a != b {
                    parent[a.max(b) as usize] = a.min(b);
                }
            }
        });
    }
    let mut by_root: HashMap<u32, Vec<i64>> = HashMap::new();
    for i in 0..pics.len() as u32 {
        let r = root(&mut parent, i);
        by_root.entry(r).or_default().push(pics[i as usize].0);
    }
    let mut groups: Vec<Vec<i64>> = by_root.into_values().filter(|g| g.len() > 1).collect();
    for g in &mut groups {
        g.sort_unstable();
    }
    groups.sort();
    groups
}

/// Candidate groups (file ids) for the similar tier. Decodes pictures not hashed before.
pub(super) fn candidates(run: &mut Run<'_>, max_bits: u32) -> Result<Vec<Vec<i64>>> {
    // An index made before this tier existed has nowhere to keep what a picture looks like.
    let has_column: bool = run.conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('file') WHERE name = 'color_signature'",
        [],
        |r| Ok(r.get::<_, i64>(0)? > 0),
    )?;
    if !has_column {
        return Err(crate::YojakaError::IndexVersion(
            "this folder was scanned by an older Yojaka; scan it from scratch (--force) to compare pictures"
                .into(),
        ));
    }
    forget_old_looks(&mut run.conn)?;
    let unsupported: i64 = run.conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM file f WHERE f.media_kind = 'image' AND f.is_cloud_only = 0 \
             AND f.size_bytes > 0 AND (f.format IS NULL OR f.format NOT IN ({}))",
            formats_sql()
        ),
        [],
        |r| r.get(0),
    )?;
    run.stats.pictures_unsupported = u64::try_from(unsupported).unwrap_or(0);
    let saved: i64 = run.conn.query_row(
        "SELECT COUNT(*) FROM file WHERE perceptual_hash IS NOT NULL AND is_cloud_only = 0",
        [],
        |r| r.get(0),
    )?;
    run.stats.pictures_cached = u64::try_from(saved).unwrap_or(0);

    hash_missing(run)?;
    run.cancel.check()?;

    let mut pics: Vec<Pic> = Vec::new();
    {
        let mut stmt = run.conn.prepare(
            "SELECT f.id, f.perceptual_hash, m.width, m.height, f.color_signature, \
                    f.format LIKE 'raw-%' \
             FROM file f LEFT JOIN meta m ON m.file_id = f.id \
             WHERE f.media_kind = 'image' AND f.is_cloud_only = 0 AND f.size_bytes > 0 \
               AND f.perceptual_hash IS NOT NULL AND f.color_signature IS NOT NULL \
             ORDER BY f.id",
        )?;
        let rows = stmt.query_map([], |r| {
            // The saved size of a raw photo is not trusted. The metadata pass reads it through
            // ImageIO, but falls back to the EXIF reader (which can give a Nikon NEF its
            // thumbnail's size) when ImageIO cannot read a file, and saved data from before keeps
            // such sizes until the next scan. Which way up it is still counts ([`TALL`]).
            let raw: bool = r.get(5)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(4)? as u64,
                if raw {
                    None
                } else {
                    aspect(r.get(2)?, r.get(3)?)
                },
            ))
        })?;
        for row in rows {
            let (id, hash, color, shape) = row?;
            let ones = hash.count_ones();
            if !(MIN_DETAIL_BITS..=64 - MIN_DETAIL_BITS).contains(&ones) {
                run.stats.pictures_unusable += 1;
            } else {
                pics.push((id, hash, color, shape));
            }
        }
    }
    Ok(cluster(&pics, max_bits))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A grey test picture: smooth blobs, so it survives resizing.
    fn picture(seed: u32, w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f64 / w as f64, y as f64 / h as f64);
                let s = f64::from(seed);
                let a = ((fx * (3.0 + s % 5.0) + s).sin() + (fy * (2.0 + s % 3.0) + s * 0.7).cos())
                    * 60.0;
                v.push((128.0 + a).clamp(0.0, 255.0) as u8);
            }
        }
        v
    }

    #[test]
    fn a_resized_or_noisy_copy_hashes_close_and_another_picture_far() {
        let a = dhash(&picture(1, 640, 480), 640, 480);
        let small = dhash(&picture(1, 160, 120), 160, 120);
        let mut noisy = picture(1, 640, 480);
        for (i, v) in noisy.iter_mut().enumerate() {
            *v = v.saturating_add((i % 3) as u8);
        }
        let noisy = dhash(&noisy, 640, 480);
        let other = dhash(&picture(2, 640, 480), 640, 480);
        assert!(
            (a ^ small).count_ones() <= 4,
            "{}",
            (a ^ small).count_ones()
        );
        assert!((a ^ noisy).count_ones() <= 4);
        assert!(
            (a ^ other).count_ones() > 10,
            "{}",
            (a ^ other).count_ones()
        );
    }

    #[test]
    fn tiny_and_empty_pictures_do_not_panic() {
        assert_eq!(dhash(&[], 0, 0), 0);
        let _ = dhash(&[10, 200, 30], 3, 1);
        let _ = dhash(&[7; 4], 2, 2);
        // A flat picture has no detail.
        assert_eq!(dhash(&[90; 64 * 64], 64, 64), 0);
    }

    #[test]
    fn the_tree_finds_exactly_what_a_full_comparison_finds() {
        // A small deterministic generator (no extra crate).
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut hashes: Vec<u64> = (0..400).map(|_| next()).collect();
        // Near copies of some of them.
        for i in 0..100 {
            let flips = (1u64 << (i % 64)) | (1u64 << ((i * 7) % 64));
            hashes.push(hashes[i] ^ flips);
        }
        hashes.push(hashes[0]);
        let mut tree = BkTree::new();
        for (i, h) in hashes.iter().enumerate() {
            tree.insert(*h, i as u32);
        }
        for max in [0, 2, 6, 12] {
            for (i, h) in hashes.iter().enumerate() {
                let mut got = Vec::new();
                tree.within(*h, max, |j, d| {
                    assert_eq!(d, (hashes[j as usize] ^ h).count_ones());
                    got.push(j);
                });
                got.sort_unstable();
                let want: Vec<u32> = (0..hashes.len() as u32)
                    .filter(|&j| (hashes[j as usize] ^ h).count_ones() <= max)
                    .collect();
                assert_eq!(got, want, "item {i}, distance {max}");
            }
        }
    }

    #[test]
    fn clusters_need_close_hashes_the_same_shape_and_the_same_colors() {
        let h = 0b1111_0000u64 << 20;
        let red = color_signature(&[200, 30, 30].repeat(16), 4, 4);
        let blue = color_signature(&[30, 30, 200].repeat(16), 4, 4);
        let pics = [
            (1, h, red, Some(1.5)),
            (2, h ^ 0b11, red, Some(1.5)),   // 2 bits away
            (3, h ^ 0b1, red, Some(1.78)),   // close, but wider (a crop)
            (4, !0u64 >> 3, red, Some(1.5)), // far
            (5, h ^ 0b100, red, Some(1.51)), // rounding when resizing
            (6, h, blue, Some(1.5)),         // same light and dark, other colours
            (7, h, red | TALL, Some(1.5)),   // same proportions, but portrait
        ];
        assert_eq!(cluster(&pics, 6), vec![vec![1, 2, 5]]);
        assert!(cluster(&pics, 0).is_empty());
        // A picture whose shape is unknown is compared by hash and colour alone.
        let unknown = [(1, h, red, Some(1.5)), (2, h, red, None)];
        assert_eq!(cluster(&unknown, 0), vec![vec![1, 2]]);
    }

    #[test]
    fn shape_ignores_whether_a_picture_is_stored_sideways() {
        assert_eq!(
            aspect(Some(4032), Some(3024)),
            aspect(Some(3024), Some(4032))
        );
        assert_eq!(aspect(Some(300), Some(200)), Some(1.5));
        assert_eq!(aspect(Some(0), Some(200)), None);
        assert_eq!(aspect(None, Some(200)), None);
    }

    /// Writes `bytes` to `name` in `dir` and measures it.
    fn look_of(dir: &Path, name: &str, bytes: &[u8]) -> Look {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        let ext = p.extension().unwrap().to_str().unwrap();
        look_file(&p, Format::from_extension(ext).unwrap()).unwrap()
    }

    fn alike(a: Look, b: Look) -> bool {
        (a.hash ^ b.hash).count_ones() <= 6 && same_colors(a.color, b.color)
    }

    #[test]
    fn every_picture_format_is_compared_and_no_video_is() {
        use crate::model::MediaKind;
        for f in COMPARED {
            assert_eq!(f.kind(), MediaKind::Image, "{f:?}");
            assert!(decoder(f).is_some(), "{f:?}");
        }
        // Raw files are TIFF inside: ImageIO must be told which camera format each is.
        let raw = COMPARED.iter().filter(|f| f.as_str().starts_with("raw-"));
        for f in raw {
            assert!(
                matches!(decoder(*f), Some(Decoder::ImageIo { pixels: Pixels::RawPreview, type_id }) if type_id.contains("raw")),
                "{f:?}"
            );
        }
        for f in [Format::Mov, Format::Mp4, Format::Mkv, Format::Avi] {
            assert_eq!(decoder(f), None);
            assert!(matches!(
                look_file(Path::new("/nonexistent"), f),
                Err(HashError::Decode(_))
            ));
        }
        assert!(formats_sql().starts_with("'jpeg','png',"));
        assert!(formats_sql().contains("'raw-arw'"));
    }

    #[test]
    fn an_avif_looks_like_the_jpeg_it_was_made_from() {
        use fixturegen::pictures;
        let tmp = tempfile::tempdir().unwrap();
        let jpeg = pictures::jpeg(&pictures::detailed(13, 320, 240), 90, 1).unwrap();
        let avif = pictures::avif_from_jpeg(&jpeg).expect("sips makes an AVIF");
        let from_jpeg = look_of(tmp.path(), "a.jpg", &jpeg);
        let from_avif = look_of(tmp.path(), "a.avif", &avif);
        assert!(
            alike(from_jpeg, from_avif),
            "{} bits apart",
            (from_jpeg.hash ^ from_avif.hash).count_ones()
        );
    }

    #[test]
    fn a_heic_looks_like_the_jpeg_it_was_made_from() {
        use fixturegen::pictures;
        let tmp = tempfile::tempdir().unwrap();
        let jpeg = pictures::jpeg(&pictures::detailed(11, 320, 240), 90, 1).unwrap();
        let heic = pictures::heic_from_jpeg(&jpeg).expect("sips makes a HEIC");
        let from_jpeg = look_of(tmp.path(), "a.jpg", &jpeg);
        let from_heic = look_of(tmp.path(), "a.heic", &heic);
        assert!(
            alike(from_jpeg, from_heic),
            "{} bits apart",
            (from_jpeg.hash ^ from_heic.hash).count_ones()
        );
        let other = pictures::jpeg(&pictures::detailed(12, 320, 240), 90, 1).unwrap();
        assert!(!alike(from_heic, look_of(tmp.path(), "b.jpg", &other)));
    }

    #[test]
    fn a_photo_stored_sideways_with_a_rotation_flag_looks_like_its_upright_copy() {
        use fixturegen::pictures;
        let tmp = tempfile::tempdir().unwrap();
        // A portrait photo as an iPhone stores it: pixels turned a quarter to the left, plus
        // orientation 6. sips keeps that in the HEIC (rotation property and EXIF).
        let upright = pictures::detailed(21, 240, 320);
        let sideways = image::imageops::rotate270(&upright);
        let flagged_jpeg = pictures::jpeg(&sideways, 90, 6).unwrap();
        let flagged_heic = pictures::heic_from_jpeg(&flagged_jpeg).expect("sips makes a HEIC");

        let want = look_of(
            tmp.path(),
            "up.jpg",
            &pictures::jpeg(&upright, 90, 1).unwrap(),
        );
        for (name, bytes) in [("flag.jpg", &flagged_jpeg), ("flag.heic", &flagged_heic)] {
            let got = look_of(tmp.path(), name, bytes);
            assert!(
                alike(want, got),
                "{name}: {} bits apart",
                (want.hash ^ got.hash).count_ones()
            );
        }
        // Without its flag the sideways picture does not look the same, so the flag mattered.
        let unflagged = look_of(
            tmp.path(),
            "side.jpg",
            &pictures::jpeg(&sideways, 90, 1).unwrap(),
        );
        assert!(!alike(want, unflagged));
    }

    #[test]
    fn colors_tolerate_recompression_but_not_another_tint() {
        let base: Vec<u8> = (0..64 * 64)
            .flat_map(|i| [(i % 251) as u8, 90, 160])
            .collect();
        let nudged: Vec<u8> = base.iter().map(|v| v.saturating_add(3)).collect();
        let tinted: Vec<u8> = base
            .iter()
            .enumerate()
            .map(|(i, v)| if i % 3 == 1 { v.saturating_add(40) } else { *v })
            .collect();
        let sig = |v: &[u8]| color_signature(v, 64, 64);
        assert!(same_colors(sig(&base), sig(&nudged)));
        assert!(!same_colors(sig(&base), sig(&tinted)));
        // Odd sizes and empty input do not panic.
        let _ = color_signature(&[1, 2, 3], 1, 1);
        assert_eq!(color_signature(&[], 0, 0), 0);
    }
}
