//! Tree generation and the manifest.

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{ExtendedColorType, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::exif_writer::{ExifData, splice_into_jpeg};
use crate::rng::Rng;
use crate::video::{VideoSpec, write_video};
use crate::{GenError, io};

pub const GENERATOR_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct Spec {
    pub files: usize,
    pub seed: u64,
    pub depth: usize,
    pub dupes: f64,
    pub corrupt: f64,
    pub gps: f64,
    pub heic: f64,
    pub hardlinks: usize,
    pub symlinks: usize,
    pub noise: usize,
    /// Extra videos, as a fraction of `files` (iPhone MOV, Android MP4, `moov` last, corrupt).
    pub videos: f64,
    /// Live Photo pairs (JPEG + MOV sharing a content identifier), plus decoys that must not pair.
    pub live_photos: usize,
    /// Sets of look-alikes for the likely, name and similar duplicate tiers: a detailed picture
    /// (or a clip) plus a re-encoded, resized, stripped, renamed or re-exported copy, and decoys
    /// that only seem to match. 0 (the default) leaves every other file exactly as before.
    pub variants: usize,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            files: 1000,
            seed: 42,
            depth: 4,
            dupes: 0.10,
            corrupt: 0.01,
            gps: 0.6,
            heic: 0.0,
            hardlinks: 5,
            symlinks: 5,
            noise: 200,
            videos: 0.0,
            live_photos: 0,
            variants: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub expect_city: String,
    pub expect_cc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Relative to `tree/`
    pub path: String,
    /// `image` or `other`
    pub kind: String,
    pub format: Option<String>,
    pub size: u64,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
    pub gps: Option<Gps>,
    pub dup_group: Option<u32>,
    pub name_group: Option<u32>,
    pub hardlink_of: Option<String>,
    pub symlink_to: Option<String>,
    pub corrupt: bool,
    pub should_index: bool,
    /// Videos only.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Both halves of a true Live Photo pair share this number.
    #[serde(default)]
    pub live_pair: Option<u32>,
    /// Set by `Spec::variants`: `base`, a true copy (`reencoded`, `resized`, `stripped`,
    /// `renamed`, `reexport`) or a decoy (`burst`, `name_decoy`, `longer`).
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expected {
    pub indexed_count: usize,
    pub exact_groups: Vec<Vec<String>>,
    pub by_format: BTreeMap<String, usize>,
    #[serde(default)]
    pub live_pairs: usize,
    /// Files that are the same picture or clip but not the same bytes (from `Spec::variants`).
    /// Decoys are not listed.
    #[serde(default)]
    pub same_content: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub seed: u64,
    pub generator_version: u32,
    pub files: Vec<Entry>,
    pub expected: Expected,
    #[serde(skip)]
    pub warnings: Vec<String>,
}

const PLACES: &[(&str, &str, f64, f64)] = &[
    ("Panaji", "IN", 15.4989, 73.8278),
    ("Paris", "FR", 48.8566, 2.3522),
    ("Tokyo", "JP", 35.6762, 139.6503),
    ("Sydney", "AU", -33.8688, 151.2093),
    ("New York City", "US", 40.7128, -74.0060),
    ("Cape Town", "ZA", -33.9249, 18.4241),
];
const CAMERAS: &[(&str, &str)] = &[
    ("Apple", "iPhone 15"),
    ("Google", "Pixel 8"),
    ("Canon", "EOS R6"),
];
const DIR_NAMES: &[&str] = &[
    "a", "b", "c", "d", "trips", "2023", "family", "misc", "raw", "old",
];

struct Gen<'a> {
    spec: &'a Spec,
    tree: PathBuf,
    rng: Rng,
    entries: Vec<Entry>,
    same_content: Vec<Vec<String>>,
    used: HashSet<String>,
    dirs: Vec<String>,
    warnings: Vec<String>,
}

pub fn generate(spec: &Spec, out: &Path) -> Result<Manifest, GenError> {
    prepare_out(out)?;
    let mut g = Gen {
        spec,
        tree: out.join("tree"),
        rng: Rng::new(spec.seed),
        entries: Vec::new(),
        same_content: Vec::new(),
        used: HashSet::new(),
        dirs: Vec::new(),
        warnings: Vec::new(),
    };
    io(&g.tree, std::fs::create_dir_all(&g.tree))?;
    io(
        &out.join("outside"),
        std::fs::create_dir_all(out.join("outside")),
    )?;
    g.make_dirs();
    let uniques = g.make_uniques()?;
    g.make_dupes(&uniques)?;
    g.make_links(&uniques, out)?;
    g.make_videos()?;
    g.make_live_photos()?;
    g.make_variants()?;
    g.make_noise_and_bait()?;
    let manifest = g.finish(out)?;
    Ok(manifest)
}

/// Only ever touch a directory that is empty or was produced by fixturegen.
fn prepare_out(out: &Path) -> Result<(), GenError> {
    if out.exists() {
        let empty = io(out, std::fs::read_dir(out))?.next().is_none();
        if !empty {
            if !out.join("manifest.json").is_file() {
                return Err(GenError::Other(format!(
                    "{} is not empty and not a fixturegen output; refusing to touch it",
                    out.display()
                )));
            }
            for sub in ["tree", "outside"] {
                let p = out.join(sub);
                if p.exists() {
                    io(&p, std::fs::remove_dir_all(&p))?;
                }
            }
            io(out, std::fs::remove_file(out.join("manifest.json")))?;
        }
    }
    io(out, std::fs::create_dir_all(out))
}

fn take_stem(path: &str) -> (&str, &str) {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').unwrap_or((name, ""))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

impl Gen<'_> {
    fn make_dirs(&mut self) {
        let n = (self.spec.files / 20).max(8);
        self.dirs.push(String::new());
        for _ in 0..n {
            let depth = self.rng.below(self.spec.depth + 1);
            let parts: Vec<&str> = (0..depth)
                .map(|_| DIR_NAMES[self.rng.below(DIR_NAMES.len())])
                .collect();
            self.dirs.push(parts.join("/"));
        }
        self.dirs.sort();
        self.dirs.dedup();
    }

    fn write(&mut self, rel: &str, bytes: &[u8]) -> Result<(), GenError> {
        let full = self.tree.join(rel);
        if let Some(parent) = full.parent() {
            io(parent, std::fs::create_dir_all(parent))?;
        }
        io(&full, std::fs::write(&full, bytes))?;
        self.used.insert(rel.to_string());
        Ok(())
    }

    fn base_image(&mut self, idx: usize) -> RgbImage {
        let (r, g, b) = (
            self.rng.below(256) as u8,
            self.rng.below(256) as u8,
            self.rng.below(256) as u8,
        );
        let mut img = RgbImage::from_fn(64, 48, |x, y| {
            Rgb([
                r.wrapping_add((x * 3) as u8),
                g.wrapping_add((y * 4) as u8),
                b.wrapping_add((x + y) as u8),
            ])
        });
        // Encode the index into the first row so every image is distinct.
        for bit in 0..32u32 {
            let v = if (idx >> bit) & 1 == 1 { 255 } else { 0 };
            img.put_pixel(bit, 0, Rgb([v, v, v]));
        }
        img
    }

    fn encode_jpeg(&self, img: &RgbImage) -> Result<Vec<u8>, GenError> {
        self.encode_jpeg_q(img, 85)
    }

    fn encode_jpeg_q(&self, img: &RgbImage, quality: u8) -> Result<Vec<u8>, GenError> {
        let mut buf = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
            .encode(
                img.as_raw(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|e| GenError::Image(e.to_string()))?;
        Ok(buf)
    }

    fn encode_png(&self, img: &RgbImage) -> Result<Vec<u8>, GenError> {
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .map_err(|e| GenError::Image(e.to_string()))?;
        Ok(buf)
    }

    fn encode_gif(&mut self, img: &RgbImage, frames: u32) -> Result<Vec<u8>, GenError> {
        let mut buf = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut buf);
            for f in 0..frames {
                let rgba = RgbaImage::from_fn(img.width(), img.height(), |x, y| {
                    let p = img.get_pixel(x, y);
                    Rgba([p[0].wrapping_add((f * 40) as u8), p[1], p[2], 255])
                });
                enc.encode_frame(image::Frame::new(rgba))
                    .map_err(|e| GenError::Image(e.to_string()))?;
            }
        }
        Ok(buf)
    }

    fn heic_from_jpeg(&mut self, jpeg: &[u8]) -> Option<Vec<u8>> {
        crate::pictures::heic_from_jpeg(jpeg)
    }

    fn make_uniques(&mut self) -> Result<Vec<usize>, GenError> {
        let mut uniques = Vec::new();
        let mut warned_heic = false;
        for i in 0..self.spec.files {
            let dir = self.dirs[self.rng.below(self.dirs.len())].clone();
            let roll = self.rng.below(100);
            let want_heic = self.spec.heic > 0.0 && self.rng.chance(self.spec.heic);
            let img = self.base_image(i);

            let year = self.rng.range(2015, 2024);
            let (mo, d) = (self.rng.range(1, 12), self.rng.range(1, 28));
            let (h, mi, s) = (
                self.rng.range(0, 23),
                self.rng.range(0, 59),
                self.rng.range(0, 59),
            );
            let taken_at = format!("{year:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
            let exif_dt = format!("{year:04}:{mo:02}:{d:02} {h:02}:{mi:02}:{s:02}");
            let (make, model) = CAMERAS[self.rng.below(CAMERAS.len())];
            let place = self
                .rng
                .chance(self.spec.gps)
                .then(|| PLACES[self.rng.below(PLACES.len())]);
            let corrupt = self.rng.chance(self.spec.corrupt);

            let (ext, format) = if roll < 70 {
                if want_heic {
                    ("heic", "heic")
                } else {
                    ("jpg", "jpeg")
                }
            } else if roll < 90 {
                ("png", "png")
            } else {
                ("gif", "gif")
            };
            let rel = join(&dir, &format!("IMG_{i:04}.{ext}"));

            let exif_capable = matches!(format, "jpeg" | "heic");
            let (bytes, has_meta) = if corrupt {
                (corrupt_bytes(format, &mut self.rng), false)
            } else if exif_capable {
                let jpeg = self.encode_jpeg(&img)?;
                let jpeg = splice_into_jpeg(
                    &jpeg,
                    &ExifData {
                        make,
                        model,
                        orientation: 1,
                        date_time_original: &exif_dt,
                        gps: place.map(|p| (p.2, p.3)),
                    },
                );
                if format == "heic" {
                    match self.heic_from_jpeg(&jpeg) {
                        Some(b) => (b, true),
                        None => {
                            if !warned_heic {
                                self.warnings.push("sips unavailable or failed; HEIC files written as JPEG content".into());
                                warned_heic = true;
                            }
                            (jpeg, true)
                        }
                    }
                } else {
                    (jpeg, true)
                }
            } else if format == "png" {
                (self.encode_png(&img)?, false)
            } else {
                let frames = if self.rng.chance(0.5) { 2 } else { 1 };
                (self.encode_gif(&img, frames)?, false)
            };

            self.write(&rel, &bytes)?;
            self.entries.push(Entry {
                path: rel,
                kind: "image".into(),
                format: Some(format.into()),
                size: bytes.len() as u64,
                taken_at: has_meta.then_some(taken_at),
                camera: has_meta.then(|| format!("{make} {model}")),
                gps: if has_meta {
                    place.map(|p| Gps {
                        lat: p.2,
                        lon: p.3,
                        expect_city: p.0.into(),
                        expect_cc: p.1.into(),
                    })
                } else {
                    None
                },
                dup_group: None,
                name_group: None,
                hardlink_of: None,
                symlink_to: None,
                corrupt,
                should_index: true,
                duration_ms: None,
                live_pair: None,
                variant: None,
            });
            uniques.push(self.entries.len() - 1);
        }
        Ok(uniques)
    }

    fn pick_other_dir(&mut self, avoid: &str) -> String {
        for _ in 0..20 {
            let d = &self.dirs[self.rng.below(self.dirs.len())];
            if d != avoid {
                return d.clone();
            }
        }
        format!("{avoid}/copies")
    }

    fn make_dupes(&mut self, uniques: &[usize]) -> Result<(), GenError> {
        let healthy: Vec<usize> = uniques
            .iter()
            .copied()
            .filter(|&i| !self.entries[i].corrupt)
            .collect();
        if healthy.is_empty() {
            return Ok(());
        }
        let n = (self.spec.files as f64 * self.spec.dupes).round() as usize;
        let mut name_groups = 0u32;
        let mut group_of: BTreeMap<usize, u32> = BTreeMap::new();

        for k in 0..n {
            let src = healthy[self.rng.below(healthy.len())];
            let src_path = self.entries[src].path.clone();
            let (stem, ext) = take_stem(&src_path);
            let (stem, ext) = (stem.to_string(), ext.to_string());
            let other = self.pick_other_dir(dir_of(&src_path));
            let name = match k % 3 {
                0 => format!("{stem} (1).{ext}"),
                1 => format!("{stem} copy.{ext}"),
                _ => format!("{stem}.{ext}"),
            };
            let rel = join(&other, &name);
            if self.used.contains(&rel) {
                continue;
            }
            let bytes = std::fs::read(self.tree.join(&src_path)).map_err(|e| GenError::Io {
                path: self.tree.join(&src_path),
                source: e,
            })?;
            self.write(&rel, &bytes)?;
            let mut e = self.entries[src].clone();
            e.path = rel;
            if k % 3 != 2 {
                let g = *group_of.entry(src).or_insert_with(|| {
                    name_groups += 1;
                    name_groups
                });
                self.entries[src].name_group = Some(g);
                e.name_group = Some(g);
            } else {
                e.name_group = None;
            }
            self.entries.push(e);
        }

        // Same-looking names, different bytes (for the name tier).
        let jpegs: Vec<usize> = healthy
            .iter()
            .copied()
            .filter(|&i| self.entries[i].format.as_deref() == Some("jpeg"))
            .collect();
        for k in 0..(n / 2) {
            if jpegs.is_empty() {
                break;
            }
            let src = jpegs[self.rng.below(jpegs.len())];
            let src_path = self.entries[src].path.clone();
            let (stem, _) = take_stem(&src_path);
            let rel = join(
                &self.pick_other_dir(dir_of(&src_path)),
                &format!("{stem} (2).jpg"),
            );
            if self.used.contains(&rel) {
                continue;
            }
            let img = self.base_image(self.spec.files + k);
            let bytes = self.encode_jpeg(&img)?;
            self.write(&rel, &bytes)?;
            let g = *group_of.entry(src).or_insert_with(|| {
                name_groups += 1;
                name_groups
            });
            self.entries[src].name_group = Some(g);
            self.entries.push(Entry {
                path: rel,
                kind: "image".into(),
                format: Some("jpeg".into()),
                size: bytes.len() as u64,
                taken_at: None,
                camera: None,
                gps: None,
                dup_group: None,
                name_group: Some(g),
                hardlink_of: None,
                symlink_to: None,
                corrupt: false,
                should_index: true,
                duration_ms: None,
                live_pair: None,
                variant: None,
            });
        }
        Ok(())
    }

    fn make_links(&mut self, uniques: &[usize], out: &Path) -> Result<(), GenError> {
        if uniques.is_empty() {
            return Ok(());
        }
        for k in 0..self.spec.hardlinks {
            let src = uniques[self.rng.below(uniques.len())];
            let src_path = self.entries[src].path.clone();
            let (stem, ext) = take_stem(&src_path);
            let rel = join(
                &self.pick_other_dir(dir_of(&src_path)),
                &format!("{stem} hardlink{k}.{ext}"),
            );
            if self.used.contains(&rel) {
                continue;
            }
            let full = self.tree.join(&rel);
            if let Some(p) = full.parent() {
                io(p, std::fs::create_dir_all(p))?;
            }
            io(&full, std::fs::hard_link(self.tree.join(&src_path), &full))?;
            self.used.insert(rel.clone());
            let mut e = self.entries[src].clone();
            e.path = rel;
            e.hardlink_of = Some(src_path);
            e.name_group = None;
            self.entries.push(e);
        }

        for k in 0..self.spec.symlinks {
            let inside = k % 2 == 0;
            let dir = self.dirs[self.rng.below(self.dirs.len())].clone();
            let rel = join(&dir, &format!("link{k}.jpg"));
            if self.used.contains(&rel) {
                continue;
            }
            let ups = if dir.is_empty() {
                0
            } else {
                dir.split('/').count()
            };
            let (target_rel, link_target) = if inside {
                let src = uniques[self.rng.below(uniques.len())];
                let tp = self.entries[src].path.clone();
                (tp.clone(), format!("{}{}", "../".repeat(ups), tp))
            } else {
                let name = format!("outside{k}.jpg");
                let img = self.base_image(1_000_000 + k);
                let bytes = self.encode_jpeg(&img)?;
                let p = out.join("outside").join(&name);
                io(&p, std::fs::write(&p, bytes))?;
                (
                    format!("../outside/{name}"),
                    format!("{}../outside/{name}", "../".repeat(ups)),
                )
            };
            let full = self.tree.join(&rel);
            if let Some(p) = full.parent() {
                io(p, std::fs::create_dir_all(p))?;
            }
            io(&full, std::os::unix::fs::symlink(&link_target, &full))?;
            self.used.insert(rel.clone());
            self.entries.push(Entry {
                path: rel,
                kind: "image".into(),
                format: Some("jpeg".into()),
                size: 0,
                taken_at: None,
                camera: None,
                gps: None,
                dup_group: None,
                name_group: None,
                hardlink_of: None,
                symlink_to: Some(target_rel),
                corrupt: false,
                should_index: false,
                duration_ms: None,
                live_pair: None,
                variant: None,
            });
        }
        Ok(())
    }

    fn video_entry(&self, rel: String, spec: &VideoSpec, format: &str) -> Result<Entry, GenError> {
        let full = self.tree.join(&rel);
        let size = io(&full, std::fs::metadata(&full))?.len();
        Ok(Entry {
            path: rel,
            kind: "video".into(),
            format: Some(format.into()),
            size,
            taken_at: None,
            camera: spec
                .make
                .as_ref()
                .zip(spec.model.as_ref())
                .map(|(a, b)| format!("{a} {b}")),
            gps: None,
            dup_group: None,
            name_group: None,
            hardlink_of: None,
            symlink_to: None,
            corrupt: false,
            should_index: true,
            duration_ms: Some(spec.duration_ms),
            live_pair: None,
            variant: None,
        })
    }

    /// Extra videos: iPhone MOV, Android MP4, `moov` after `mdat`, no metadata, and corrupt.
    fn make_videos(&mut self) -> Result<(), GenError> {
        let n = (self.spec.files as f64 * self.spec.videos).round() as usize;
        let mut rng = Rng::new(self.spec.seed ^ 0x51DE_0000);
        for i in 0..n {
            let dir = self.dirs[rng.below(self.dirs.len())].clone();
            let place = PLACES[rng.below(PLACES.len())];
            let (y, mo, d) = (
                rng.range(2016, 2024) as i64,
                rng.range(1, 12) as i64,
                rng.range(1, 28) as i64,
            );
            let (h, mi, s) = (
                rng.range(0, 23) as i64,
                rng.range(0, 59) as i64,
                rng.range(0, 59) as i64,
            );
            let wall = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
            let unix = civil_to_unix(y, mo, d, h, mi, s);
            let duration_ms = 1000 + rng.range(0, 60) as u64 * 1000;
            let iso = format!("{:+08.4}{:+09.4}+012.000/", place.2, place.3);
            let variant = i % 5;
            let (rel, format, spec) = match variant {
                0 => (
                    join(&dir, &format!("IMG_V{i:04}.mov")),
                    "mov",
                    VideoSpec {
                        duration_ms,
                        rotation: if i % 2 == 0 { 90 } else { 0 },
                        creationdate: Some(format!("{wall}+0530")),
                        iso6709: Some(iso.clone()),
                        make: Some("Apple".into()),
                        model: Some("iPhone 15".into()),
                        content_id: Some(format!("VID-{:08X}-{i:08X}", self.spec.seed)),
                        mvhd_unix: Some(unix),
                        ..VideoSpec::default()
                    },
                ),
                1 => (
                    join(&dir, &format!("VID_{i:04}.mp4")),
                    "mp4",
                    VideoSpec {
                        quicktime: false,
                        duration_ms,
                        video_codec: *b"avc1",
                        fps: 25,
                        xyz: Some(format!("{:+.4}{:+.4}/", place.2, place.3)),
                        mvhd_unix: Some(unix),
                        ..VideoSpec::default()
                    },
                ),
                2 => (
                    join(&dir, &format!("LATE_{i:04}.mp4")),
                    "mp4",
                    VideoSpec {
                        quicktime: false,
                        moov_last: true,
                        mdat_len: 2 << 20,
                        duration_ms,
                        video_codec: *b"avc1",
                        mvhd_unix: Some(unix),
                        ..VideoSpec::default()
                    },
                ),
                3 => (
                    join(&dir, &format!("BARE_{i:04}.mov")),
                    "mov",
                    VideoSpec {
                        duration_ms,
                        audio_codec: None,
                        ..VideoSpec::default()
                    },
                ),
                _ => {
                    let rel = join(&dir, &format!("BROKEN_{i:04}.mp4"));
                    // ftyp, then a moov whose size claims more than the file holds.
                    let mut bytes = vec![
                        0, 0, 0, 0x14, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0,
                        b'i', b's', b'o', b'm',
                    ];
                    bytes.extend([0, 0, 0xFF, 0xFF, b'm', b'o', b'o', b'v', 1, 2, 3]);
                    self.write(&rel, &bytes)?;
                    let mut e = other_entry(rel, bytes.len() as u64);
                    e.kind = "video".into();
                    e.format = Some("mp4".into());
                    e.corrupt = true;
                    e.should_index = true;
                    self.entries.push(e);
                    continue;
                }
            };
            let full = self.tree.join(&rel);
            if let Some(p) = full.parent() {
                io(p, std::fs::create_dir_all(p))?;
            }
            write_video(&full, &spec)?;
            self.used.insert(rel.clone());
            let mut e = self.video_entry(rel, &spec, format)?;
            if spec.creationdate.is_some() {
                e.taken_at = Some(wall.clone());
            } else if spec.mvhd_unix.is_some() {
                e.taken_at = Some(wall.clone()); // UTC wall clock (no timezone applied)
            }
            if spec.iso6709.is_some() || spec.xyz.is_some() {
                e.gps = Some(Gps {
                    lat: place.2,
                    lon: place.3,
                    expect_city: place.0.into(),
                    expect_cc: place.1.into(),
                });
            }
            self.entries.push(e);
        }
        Ok(())
    }

    /// Live Photo pairs and decoys (see [`Spec::live_photos`]).
    fn make_live_photos(&mut self) -> Result<(), GenError> {
        let mut rng = Rng::new(self.spec.seed ^ 0x11FE_0000);
        let mut pair_no = 0u32;
        for k in 0..self.spec.live_photos {
            let case = k % 4;
            let dir = self.dirs[rng.below(self.dirs.len())].clone();
            let stem = format!("IMG_L{k:04}");
            let place = rng.chance(0.7).then(|| PLACES[rng.below(PLACES.len())]);
            let (y, mo, d) = (
                rng.range(2018, 2024) as i64,
                rng.range(1, 12) as i64,
                rng.range(1, 28) as i64,
            );
            let (h, mi, s) = (
                rng.range(0, 21) as i64,
                rng.range(0, 58) as i64,
                rng.range(0, 57) as i64,
            );
            let at = |extra: i64| {
                let u = civil_to_unix(y, mo, d, h, mi, s) + extra;
                let (yy, m2, d2, hh, mm, ss) = unix_to_civil(u);
                format!("{yy:04}-{m2:02}-{d2:02}T{hh:02}:{mm:02}:{ss:02}")
            };
            let exif_dt = at(0).replace('T', " ").replacen('-', ":", 2);
            let content_id = format!("LIVE-{:08X}-{k:08X}", self.spec.seed);
            let (make, model) = ("Apple", "iPhone 15");
            let iso = place.map(|p| format!("{:+08.4}{:+09.4}+012.000/", p.2, p.3));
            let gps = place.map(|p| Gps {
                lat: p.2,
                lon: p.3,
                expect_city: p.0.into(),
                expect_cc: p.1.into(),
            });

            // (video wall time, shared content id?, true pair?)
            let (vid_time, with_id, paired) = match case {
                0 | 3 => (at(0), true, true),
                1 => (at(1), false, true),
                _ => (at(3600), false, false),
            };
            let folders = if case == 3 {
                vec![dir.clone(), self.pick_other_dir(&dir)]
            } else {
                vec![dir.clone()]
            };
            for folder in folders {
                let img = self.base_image(3_000_000 + k);
                let jpeg = self.encode_jpeg(&img)?;
                let jpeg = splice_into_jpeg(
                    &jpeg,
                    &ExifData {
                        make,
                        model,
                        orientation: 1,
                        date_time_original: &exif_dt,
                        gps: place.map(|p| (p.2, p.3)),
                    },
                );
                let jrel = join(&folder, &format!("{stem}.JPG"));
                let mrel = join(&folder, &format!("{stem}.MOV"));
                if self.used.contains(&jrel) || self.used.contains(&mrel) {
                    continue;
                }
                self.write(&jrel, &jpeg)?;
                let vspec = VideoSpec {
                    duration_ms: 1500,
                    creationdate: Some(format!("{vid_time}+0000")),
                    iso6709: iso.clone(),
                    make: Some(make.into()),
                    model: Some(model.into()),
                    content_id: with_id.then(|| content_id.clone()),
                    ..VideoSpec::default()
                };
                let full = self.tree.join(&mrel);
                if let Some(p) = full.parent() {
                    io(p, std::fs::create_dir_all(p))?;
                }
                write_video(&full, &vspec)?;
                self.used.insert(mrel.clone());

                let pair = paired.then(|| {
                    pair_no += 1;
                    pair_no
                });
                let mut je = other_entry(jrel, jpeg.len() as u64);
                je.kind = "image".into();
                je.format = Some("jpeg".into());
                je.should_index = true;
                je.taken_at = Some(at(0));
                je.camera = Some(format!("{make} {model}"));
                je.gps = gps.clone();
                je.live_pair = pair;
                self.entries.push(je);
                let mut me = self.video_entry(mrel, &vspec, "mov")?;
                me.taken_at = Some(vid_time.clone());
                me.gps = gps.clone();
                me.live_pair = pair;
                self.entries.push(me);
            }
        }
        Ok(())
    }

    /// Look-alike sets for the non-exact duplicate tiers (see [`Spec::variants`]).
    fn make_variants(&mut self) -> Result<(), GenError> {
        let mut rng = Rng::new(self.spec.seed ^ 0x7A21_0000);
        for k in 0..self.spec.variants {
            let dir = self.dirs[rng.below(self.dirs.len())].clone();
            let other = self.pick_other_dir(&dir);
            // Every set has its own second, so sets never match each other by date.
            let unix = 1_650_000_000 + k as i64 * 3671;
            let (y, mo, d, h, mi, s) = unix_to_civil(unix);
            let wall = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
            let exif_dt = format!("{y:04}:{mo:02}:{d:02} {h:02}:{mi:02}:{s:02}");
            let (make, model) = CAMERAS[k % CAMERAS.len()];
            let case = k % 6;

            if case == 5 {
                // A clip, the same clip exported again (0.4 s longer, other codec), and a
                // different clip recorded in the same second that is 5 s longer.
                let duration_ms = 4000 + rng.range(0, 50) as u64 * 1000;
                let clips = [
                    (
                        join(&dir, &format!("CLIP_{k:04}.mp4")),
                        "base",
                        0,
                        *b"avc1",
                        8192,
                    ),
                    (
                        join(&other, &format!("CLIP_{k:04}_export.mp4")),
                        "reexport",
                        400,
                        *b"hvc1",
                        6000,
                    ),
                    (
                        join(&other, &format!("CLIP_{k:04}_long.mp4")),
                        "longer",
                        5000,
                        *b"avc1",
                        9000,
                    ),
                ];
                let mut same = Vec::new();
                for (rel, label, extra_ms, codec, mdat_len) in clips {
                    if self.used.contains(&rel) {
                        continue;
                    }
                    let spec = VideoSpec {
                        quicktime: false,
                        duration_ms: duration_ms + extra_ms,
                        video_codec: codec,
                        mdat_len,
                        mvhd_unix: Some(unix),
                        ..VideoSpec::default()
                    };
                    let full = self.tree.join(&rel);
                    if let Some(p) = full.parent() {
                        io(p, std::fs::create_dir_all(p))?;
                    }
                    write_video(&full, &spec)?;
                    self.used.insert(rel.clone());
                    let mut e = self.video_entry(rel.clone(), &spec, "mp4")?;
                    e.taken_at = Some(wall.clone());
                    e.variant = Some(label.into());
                    self.entries.push(e);
                    if label != "longer" {
                        same.push(rel);
                    }
                }
                if same.len() > 1 {
                    self.same_content.push(same);
                }
                continue;
            }

            let picture = detailed_picture(&mut rng, 320, 240);
            let exif = ExifData {
                make,
                model,
                orientation: 1,
                date_time_original: &exif_dt,
                gps: None,
            };
            let with_exif = |jpeg: &[u8]| splice_into_jpeg(jpeg, &exif);
            let stem = format!("PIC_{k:04}");
            // (path, label, bytes, format, keeps the camera's date?)
            let mut files: Vec<(String, &str, Vec<u8>, &str, bool)> = vec![(
                join(&dir, &format!("{stem}.jpg")),
                "base",
                with_exif(&self.encode_jpeg_q(&picture, 90)?),
                "jpeg",
                true,
            )];
            match case {
                // Saved again at a lower quality: same date, camera and size in pixels.
                0 => files.push((
                    join(&other, &format!("{stem}_edit.jpg")),
                    "reencoded",
                    with_exif(&self.encode_jpeg_q(&picture, 60)?),
                    "jpeg",
                    true,
                )),
                // Half the size in pixels.
                1 => files.push((
                    join(&other, &format!("{stem}_small.jpg")),
                    "resized",
                    with_exif(&self.encode_jpeg_q(&halved(&picture), 85)?),
                    "jpeg",
                    true,
                )),
                // Another format, no camera data; plus an unrelated picture with a counter
                // suffix that makes the names collide.
                2 => {
                    files.push((
                        join(&other, &format!("{stem}_shared.png")),
                        "stripped",
                        self.encode_png(&picture)?,
                        "png",
                        false,
                    ));
                    let unrelated = detailed_picture(&mut rng, 320, 240);
                    files.push((
                        join(&other, &format!("{stem}-1.jpg")),
                        "name_decoy",
                        self.encode_jpeg_q(&unrelated, 90)?,
                        "jpeg",
                        false,
                    ));
                }
                // A copy the Finder renamed, re-saved without camera data: almost the same size.
                3 => {
                    let mut touched = picture.clone();
                    touched.put_pixel(0, 0, Rgb([1, 2, 3]));
                    files.push((
                        join(&other, &format!("{stem} copy.jpg")),
                        "renamed",
                        self.encode_jpeg_q(&touched, 90)?,
                        "jpeg",
                        false,
                    ));
                }
                // Another picture taken in the same second with the same camera (a burst).
                _ => {
                    let next_shot = detailed_picture(&mut rng, 320, 240);
                    files.push((
                        join(&dir, &format!("{stem}_b.jpg")),
                        "burst",
                        with_exif(&self.encode_jpeg_q(&next_shot, 90)?),
                        "jpeg",
                        true,
                    ));
                }
            }
            let mut same = Vec::new();
            for (rel, label, bytes, format, dated) in files {
                if self.used.contains(&rel) {
                    continue;
                }
                self.write(&rel, &bytes)?;
                let mut e = other_entry(rel.clone(), bytes.len() as u64);
                e.kind = "image".into();
                e.format = Some(format.into());
                e.should_index = true;
                e.variant = Some(label.into());
                if dated {
                    e.taken_at = Some(wall.clone());
                    e.camera = Some(format!("{make} {model}"));
                }
                self.entries.push(e);
                if !matches!(label, "burst" | "name_decoy") {
                    same.push(rel);
                }
            }
            if same.len() > 1 {
                self.same_content.push(same);
            }
        }
        Ok(())
    }

    fn make_noise_and_bait(&mut self) -> Result<(), GenError> {
        const EXTS: &[&str] = &["txt", "pdf", "json", "md"];
        for i in 0..self.spec.noise {
            let dir = self.dirs[self.rng.below(self.dirs.len())].clone();
            let ext = EXTS[i % EXTS.len()];
            let rel = join(&dir, &format!("note_{i:04}.{ext}"));
            let body = if ext == "pdf" {
                format!("%PDF-1.4\n% fixturegen noise {i}\n")
            } else {
                format!("fixturegen noise {i}\n")
            };
            self.write(&rel, body.as_bytes())?;
            self.entries.push(other_entry(rel, body.len() as u64));
        }
        // Skip-dir bait: images that must NOT be indexed.
        let img = self.base_image(2_000_000);
        let jpeg = self.encode_jpeg(&img)?;
        for rel in [
            "Test.photoslibrary/originals/IMG_9000.jpg",
            "Test.photoslibrary/IMG_9001.jpg",
            ".git/objects/IMG_9002.jpg",
        ] {
            self.write(rel, &jpeg)?;
            let mut e = other_entry(rel.to_string(), jpeg.len() as u64);
            e.kind = "image".into();
            e.format = Some("jpeg".into());
            self.entries.push(e);
        }
        Ok(())
    }

    fn finish(mut self, out: &Path) -> Result<Manifest, GenError> {
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));

        // Ground truth: group by actual bytes among files that should be indexed.
        let mut by_hash: BTreeMap<[u8; 32], Vec<usize>> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.should_index && !e.corrupt {
                let full = self.tree.join(&e.path);
                let bytes = std::fs::read(&full).map_err(|s| GenError::Io {
                    path: full,
                    source: s,
                })?;
                by_hash
                    .entry(*blake3::hash(&bytes).as_bytes())
                    .or_default()
                    .push(i);
            }
        }
        let mut exact_groups: Vec<Vec<String>> = by_hash
            .into_values()
            .filter(|v| v.len() > 1)
            .map(|v| {
                v.into_iter()
                    .map(|i| self.entries[i].path.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        for g in &mut exact_groups {
            g.sort();
        }
        exact_groups.sort();
        for (gi, group) in exact_groups.iter().enumerate() {
            for p in group {
                if let Some(e) = self.entries.iter_mut().find(|e| &e.path == p) {
                    e.dup_group = Some(gi as u32 + 1);
                }
            }
        }

        let live_pairs = self
            .entries
            .iter()
            .filter(|e| e.kind == "video" && e.live_pair.is_some())
            .count();
        let mut by_format: BTreeMap<String, usize> = BTreeMap::new();
        let mut indexed = 0;
        for e in self.entries.iter().filter(|e| e.should_index) {
            indexed += 1;
            if let Some(f) = &e.format {
                *by_format.entry(f.clone()).or_default() += 1;
            }
        }
        let manifest = Manifest {
            seed: self.spec.seed,
            generator_version: GENERATOR_VERSION,
            files: self.entries,
            expected: Expected {
                indexed_count: indexed,
                exact_groups,
                by_format,
                live_pairs,
                same_content: {
                    let mut sets = self.same_content;
                    for set in &mut sets {
                        set.sort();
                    }
                    sets.sort();
                    sets
                },
            },
            warnings: self.warnings,
        };
        let path = out.join("manifest.json");
        let json =
            serde_json::to_string_pretty(&manifest).map_err(|e| GenError::Other(e.to_string()))?;
        io(&path, std::fs::write(&path, json))?;
        Ok(manifest)
    }
}

/// A picture with detail at several scales (soft colour patches plus stripes), so that it
/// survives resizing and re-compression the way a photo does, and no two look alike.
pub(crate) fn detailed_picture(rng: &mut Rng, w: u32, h: u32) -> RgbImage {
    const GX: usize = 7;
    const GY: usize = 6;
    let grid: Vec<[f64; 3]> = (0..GX * GY)
        .map(|_| {
            [
                rng.below(256) as f64,
                rng.below(256) as f64,
                rng.below(256) as f64,
            ]
        })
        .collect();
    let (fx, fy) = (rng.range(3, 9) as f64, rng.range(3, 9) as f64);
    let phase = rng.below(628) as f64 / 100.0;
    RgbImage::from_fn(w, h, |x, y| {
        let u = x as f64 / w as f64 * (GX - 1) as f64;
        let v = y as f64 / h as f64 * (GY - 1) as f64;
        let (x0, y0) = (u.floor() as usize, v.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(GX - 1), (y0 + 1).min(GY - 1));
        let (tx, ty) = (u - x0 as f64, v - y0 as f64);
        let stripes = ((u * fx + v * fy + phase).sin()) * 18.0;
        let mut px = [0u8; 3];
        for (c, out) in px.iter_mut().enumerate() {
            let top = grid[y0 * GX + x0][c] * (1.0 - tx) + grid[y0 * GX + x1][c] * tx;
            let bottom = grid[y1 * GX + x0][c] * (1.0 - tx) + grid[y1 * GX + x1][c] * tx;
            *out = (top * (1.0 - ty) + bottom * ty + stripes).clamp(0.0, 255.0) as u8;
        }
        Rgb(px)
    })
}

/// Half the width and height: each pixel is the average of a 2×2 block.
fn halved(img: &RgbImage) -> RgbImage {
    RgbImage::from_fn(img.width() / 2, img.height() / 2, |x, y| {
        let mut sum = [0u32; 3];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let p = img.get_pixel(x * 2 + dx, y * 2 + dy);
            for c in 0..3 {
                sum[c] += u32::from(p[c]);
            }
        }
        Rgb([(sum[0] / 4) as u8, (sum[1] / 4) as u8, (sum[2] / 4) as u8])
    })
}

fn other_entry(path: String, size: u64) -> Entry {
    Entry {
        path,
        kind: "other".into(),
        format: None,
        size,
        taken_at: None,
        camera: None,
        gps: None,
        dup_group: None,
        name_group: None,
        hardlink_of: None,
        symlink_to: None,
        corrupt: false,
        should_index: false,
        duration_ms: None,
        live_pair: None,
        variant: None,
    }
}

/// Correct magic bytes followed by a truncated body.
fn corrupt_bytes(format: &str, rng: &mut Rng) -> Vec<u8> {
    let mut v: Vec<u8> = match format {
        "png" => vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        "gif" => b"GIF89a".to_vec(),
        "heic" => vec![
            0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c',
        ],
        _ => vec![0xFF, 0xD8, 0xFF, 0xE0],
    };
    for _ in 0..12 {
        v.push(rng.below(256) as u8);
    }
    v
}

/// Unix seconds of a UTC civil time.
pub(crate) fn civil_to_unix(y: i64, m: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400 + h * 3600 + mi * 60 + s
}

pub(crate) fn unix_to_civil(u: i64) -> (i64, i64, i64, i64, i64, i64) {
    let z = u.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = u.rem_euclid(86_400);
    (y, m, d, t / 3600, t % 3600 / 60, t % 60)
}
