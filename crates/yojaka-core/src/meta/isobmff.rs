//! ISO-BMFF / QuickTime video metadata (MOV, MP4, M4V, 3GP).
//!
//! The extractor walks box headers (8–16 bytes each) and **seeks** past everything it does not
//! need, so `mdat` (the media payload, often gigabytes) is never read, wherever `moov` sits.
//! Even inside `moov` only the small leaf boxes are read; sample tables (`stts`, `stco`, ...)
//! are skipped. `stsz` contributes just its 12-byte header for the frame count.

use std::io::{Read, SeekFrom};

use super::time::{from_mac_epoch, parse_exif_datetime};
use super::{DateSrc, Extractor, FileInfo, Meta, MetaError, ReadSeek};
use crate::geo::GeoPoint;
use crate::model::Format;

/// Largest leaf box we are willing to read into memory.
const MAX_LEAF: u64 = 1 << 20;

#[derive(Debug)]
pub(crate) struct IsoBmff;

#[derive(Debug, Clone, Copy)]
struct BoxHdr {
    kind: [u8; 4],
    /// First byte after the header.
    body: u64,
    end: u64,
    /// The box claims to extend past its parent (a truncated file).
    truncated: bool,
}

impl BoxHdr {
    fn len(&self) -> u64 {
        self.end - self.body
    }
    fn is(&self, k: &[u8; 4]) -> bool {
        &self.kind == k
    }
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Reads up to `n` bytes at `pos`.
fn read_at(r: &mut dyn ReadSeek, pos: u64, n: u64) -> Result<Vec<u8>, MetaError> {
    r.seek(SeekFrom::Start(pos))?;
    let mut buf = Vec::new();
    r.take(n).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Lists the boxes in `[start, end)` by reading headers only.
fn list(r: &mut dyn ReadSeek, start: u64, end: u64) -> Result<Vec<BoxHdr>, MetaError> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 <= end {
        let h = read_at(r, pos, 16)?;
        let (Some(size32), Some(kind)) = (be32(&h, 0), h.get(4..8)) else {
            break;
        };
        let kind: [u8; 4] = kind
            .try_into()
            .map_err(|_| MetaError::Malformed("box".into()))?;
        let (hdr_len, size) = match size32 {
            0 => (8, end - pos),
            1 => (
                16,
                be64(&h, 8).ok_or_else(|| MetaError::Malformed("truncated 64-bit box".into()))?,
            ),
            n => (8, u64::from(n)),
        };
        if size < hdr_len {
            return Err(MetaError::Malformed(format!(
                "box '{}' has size {size}",
                String::from_utf8_lossy(&kind)
            )));
        }
        // A box that claims to extend past its parent (truncated file) is clamped; callers that
        // need the whole body check it.
        let box_end = pos.saturating_add(size).min(end);
        out.push(BoxHdr {
            kind,
            body: pos + hdr_len,
            end: box_end,
            truncated: pos.saturating_add(size) > end,
        });
        pos = box_end;
    }
    Ok(out)
}

fn find<'a>(boxes: &'a [BoxHdr], kind: &[u8; 4]) -> Option<&'a BoxHdr> {
    boxes.iter().find(|b| b.is(kind))
}

fn leaf(r: &mut dyn ReadSeek, b: &BoxHdr) -> Result<Vec<u8>, MetaError> {
    if b.len() > MAX_LEAF {
        return Err(MetaError::Malformed(format!(
            "'{}' box is {} bytes",
            String::from_utf8_lossy(&b.kind),
            b.len()
        )));
    }
    read_at(r, b.body, b.len())
}

#[derive(Default)]
struct Track {
    handler: [u8; 4],
    width: Option<u32>,
    height: Option<u32>,
    rotation: Option<u16>,
    timescale: u32,
    duration: u64,
    samples: Option<u32>,
    codec: Option<[u8; 4]>,
}

impl Extractor for IsoBmff {
    fn formats(&self) -> &'static [Format] {
        &[Format::Mov, Format::Mp4, Format::M4v, Format::ThreeGp]
    }

    fn extract(&self, r: &mut dyn ReadSeek, info: &FileInfo) -> Result<Meta, MetaError> {
        let top = list(r, 0, info.size)?;
        if find(&top, b"ftyp").is_none() {
            return Err(MetaError::Malformed("no ftyp box".into()));
        }
        let moov = find(&top, b"moov").ok_or_else(|| MetaError::Malformed("no moov box".into()))?;
        if moov.end <= moov.body {
            return Err(MetaError::Malformed("empty moov".into()));
        }
        if moov.truncated {
            return Err(MetaError::Malformed("moov box is truncated".into()));
        }
        let mut meta = Meta {
            container: Some(info.format.as_str().to_string()),
            ..Meta::default()
        };
        let children = list(r, moov.body, moov.end)?;

        if find(&children, b"mvhd").is_none() {
            return Err(MetaError::Malformed("moov has no mvhd".into()));
        }
        let mut movie_scale = 0u32;
        let mut movie_duration = 0u64;
        if let Some(mvhd) = find(&children, b"mvhd") {
            let b = leaf(r, mvhd)?;
            let v1 = b.first() == Some(&1);
            let (ctime, ts, dur) = if v1 {
                (be64(&b, 4), be32(&b, 20), be64(&b, 24))
            } else {
                (
                    be32(&b, 4).map(u64::from),
                    be32(&b, 12),
                    be32(&b, 16).map(u64::from),
                )
            };
            movie_scale = ts.unwrap_or(0);
            movie_duration = dur.unwrap_or(0);
            if let Some(t) = ctime.and_then(from_mac_epoch) {
                meta.taken_at = Some(t);
                meta.taken_at_src = DateSrc::Container;
            }
        }

        let mut tracks = Vec::new();
        for trak in children.iter().filter(|b| b.is(b"trak")) {
            match read_track(r, trak) {
                Ok(t) => tracks.push(t),
                // One odd track must not lose the rest of the file's metadata.
                Err(MetaError::Io(e)) => return Err(MetaError::Io(e)),
                Err(_) => {}
            }
        }
        if let Some(v) = tracks.iter().find(|t| &t.handler == b"vide") {
            meta.width = v.width;
            meta.height = v.height;
            meta.rotation = v.rotation;
            meta.video_codec = v.codec.map(|c| video_codec(&c));
            if v.timescale > 0 {
                let secs = v.duration as f64 / f64::from(v.timescale);
                if let (Some(n), true) = (v.samples, secs > 0.0) {
                    let fps = f64::from(n) / secs;
                    meta.fps =
                        (fps.is_finite() && fps > 0.0).then_some((fps * 100.0).round() / 100.0);
                }
            }
        }
        if let Some(a) = tracks.iter().find(|t| &t.handler == b"soun") {
            meta.audio_codec = a.codec.map(|c| audio_codec(&c));
        }
        if movie_scale > 0 {
            meta.duration_ms = Some(movie_duration.saturating_mul(1000) / u64::from(movie_scale));
        }

        // Apple-style key/value metadata: creation date with timezone, location, make/model,
        // and the Live Photo content identifier.
        let mut location: Option<String> = None;
        let mut meta_boxes = Vec::new();
        if let Some(m) = find(&children, b"meta") {
            meta_boxes.push(*m);
        }
        let udta = find(&children, b"udta")
            .map(|u| list(r, u.body, u.end))
            .transpose()?;
        if let Some(udta) = &udta {
            if let Some(m) = find(udta, b"meta") {
                meta_boxes.push(*m);
            }
            // Android and older QuickTime: ©xyz = "+lat+lon/", ©mak / ©mod = make / model.
            for (tag, slot) in [
                (b"\xA9xyz", &mut location as &mut Option<String>),
                (b"\xA9mak", &mut meta.camera_make),
                (b"\xA9mod", &mut meta.camera_model),
            ] {
                if let Some(b) = find(udta, tag) {
                    let d = leaf(r, b)?;
                    if d.len() > 4 {
                        *slot = Some(clean(&d[4..]));
                    }
                }
            }
        }
        let mut qt_date = None;
        let mut iso_loc = None;
        for m in &meta_boxes {
            for (key, value) in read_mdta(r, m)? {
                match key.as_str() {
                    "com.apple.quicktime.creationdate" => qt_date = Some(value),
                    "com.apple.quicktime.location.ISO6709" => iso_loc = Some(value),
                    "com.apple.quicktime.make" | "com.android.manufacturer" => {
                        meta.camera_make = Some(value)
                    }
                    "com.apple.quicktime.model" | "com.android.model" => {
                        meta.camera_model = Some(value)
                    }
                    "com.apple.quicktime.content.identifier" => meta.content_id = Some(value),
                    _ => {}
                }
            }
        }
        if let Some(t) = qt_date.as_deref().and_then(parse_exif_datetime) {
            meta.taken_at = Some(t);
            meta.taken_at_src = DateSrc::QuickTime;
        }
        meta.gps = iso_loc
            .or(location)
            .as_deref()
            .and_then(parse_iso6709)
            .and_then(|(lat, lon)| GeoPoint::checked(lat, lon));
        Ok(meta)
    }
}

fn clean(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

fn read_track(r: &mut dyn ReadSeek, trak: &BoxHdr) -> Result<Track, MetaError> {
    let mut t = Track::default();
    let kids = list(r, trak.body, trak.end)?;
    if let Some(tkhd) = find(&kids, b"tkhd") {
        let b = leaf(r, tkhd)?;
        let (matrix, w, h) = if b.first() == Some(&1) {
            (52, 88, 92)
        } else {
            (40, 76, 80)
        };
        t.width = be32(&b, w).map(|v| v >> 16).filter(|v| *v > 0);
        t.height = be32(&b, h).map(|v| v >> 16).filter(|v| *v > 0);
        if let (Some(a), Some(bb)) = (be32(&b, matrix), be32(&b, matrix + 4)) {
            // 16.16 fixed point; only the four axis-aligned rotations matter.
            t.rotation = match (a as i32, bb as i32) {
                (0x10000, 0) => Some(0),
                (0, 0x10000) => Some(90),
                (-0x10000, 0) => Some(180),
                (0, -0x10000) => Some(270),
                _ => None,
            };
        }
    }
    let Some(mdia) = find(&kids, b"mdia") else {
        return Ok(t);
    };
    let m = list(r, mdia.body, mdia.end)?;
    if let Some(hdlr) = find(&m, b"hdlr") {
        let b = leaf(r, hdlr)?;
        if let Some(h) = b.get(8..12) {
            t.handler = h.try_into().unwrap_or_default();
        }
    }
    if let Some(mdhd) = find(&m, b"mdhd") {
        let b = leaf(r, mdhd)?;
        let (ts, dur) = if b.first() == Some(&1) {
            (be32(&b, 20), be64(&b, 24))
        } else {
            (be32(&b, 12), be32(&b, 16).map(u64::from))
        };
        t.timescale = ts.unwrap_or(0);
        t.duration = dur.unwrap_or(0);
    }
    if let Some(minf) = find(&m, b"minf") {
        let mk = list(r, minf.body, minf.end)?;
        if let Some(stbl) = find(&mk, b"stbl") {
            let sk = list(r, stbl.body, stbl.end)?;
            if let Some(stsd) = find(&sk, b"stsd") {
                let b = read_at(r, stsd.body, 16.min(stsd.len()))?;
                t.codec = b.get(12..16).and_then(|c| c.try_into().ok());
            }
            if let Some(stsz) = find(&sk, b"stsz") {
                let b = read_at(r, stsz.body, 12.min(stsz.len()))?;
                t.samples = be32(&b, 8);
            }
        }
    }
    Ok(t)
}

/// Reads a `meta` box in QuickTime "mdta" form: a `keys` list and an `ilst` of values.
fn read_mdta(r: &mut dyn ReadSeek, meta: &BoxHdr) -> Result<Vec<(String, String)>, MetaError> {
    // `meta` is a FullBox in ISO files (4 version/flags bytes) but a plain box in QuickTime.
    let probe = read_at(r, meta.body, 12.min(meta.len()))?;
    let start = if probe.len() >= 12 && probe[..4] == [0; 4] && &probe[8..12] == b"hdlr" {
        meta.body + 4
    } else {
        meta.body
    };
    let kids = list(r, start, meta.end)?;
    let (Some(keys), Some(ilst)) = (find(&kids, b"keys"), find(&kids, b"ilst")) else {
        return Ok(Vec::new());
    };
    let kb = leaf(r, keys)?;
    let mut names = Vec::new();
    let mut pos = 8;
    while let Some(size) = be32(&kb, pos).map(|s| s as usize) {
        if size < 8 || pos + size > kb.len() {
            break;
        }
        names.push(clean(&kb[pos + 8..pos + size]));
        pos += size;
    }
    let mut out = Vec::new();
    for item in list(r, ilst.body, ilst.end)? {
        let idx = u32::from_be_bytes(item.kind) as usize;
        let Some(name) = idx.checked_sub(1).and_then(|i| names.get(i)) else {
            continue;
        };
        let inner = list(r, item.body, item.end)?;
        if let Some(data) = find(&inner, b"data") {
            let d = leaf(r, data)?;
            // data: type(4) locale(4) value...; type 1 = UTF-8 text
            if be32(&d, 0) == Some(1) && d.len() > 8 {
                out.push((name.clone(), clean(&d[8..])));
            }
        }
    }
    Ok(out)
}

/// `"+15.4989+073.8278+012.000/"` → (lat, lon). Decimal-degree form only.
pub(crate) fn parse_iso6709(s: &str) -> Option<(f64, f64)> {
    let s = s.trim().trim_end_matches('/');
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if (c == '+' || c == '-') && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    parts.push(cur);
    if parts.len() < 2 || parts.iter().any(|p| p.len() < 2) {
        return None;
    }
    // Reject degrees+minutes forms like "+1530.5" (more than 2 integer digits for latitude).
    let int_digits = |p: &str| p[1..].split('.').next().map_or(0, str::len);
    if int_digits(&parts[0]) > 2 || int_digits(&parts[1]) > 3 {
        return None;
    }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?))
}

fn video_codec(c: &[u8; 4]) -> String {
    match c {
        b"avc1" | b"avc3" => "h264",
        b"hvc1" | b"hev1" => "hevc",
        b"av01" => "av1",
        b"vp09" => "vp9",
        b"vp08" => "vp8",
        b"mp4v" => "mpeg4",
        b"ap4h" | b"ap4x" | b"apch" | b"apcn" | b"apco" | b"apcs" => "prores",
        b"jpeg" | b"mjpa" | b"mjpb" => "mjpeg",
        other => return fourcc(other),
    }
    .to_string()
}

fn audio_codec(c: &[u8; 4]) -> String {
    match c {
        b"mp4a" => "aac",
        b"ac-3" => "ac3",
        b"ec-3" => "eac3",
        b"alac" => "alac",
        b"lpcm" | b"sowt" | b"twos" | b"in24" | b"in32" | b"fl32" => "pcm",
        b"Opus" => "opus",
        b"fLaC" => "flac",
        other => return fourcc(other),
    }
    .to_string()
}

fn fourcc(c: &[u8; 4]) -> String {
    clean(c).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso6709_forms() {
        assert_eq!(
            parse_iso6709("+15.4989+073.8278+012.000/"),
            Some((15.4989, 73.8278))
        );
        assert_eq!(
            parse_iso6709("-33.8568+151.2153/"),
            Some((-33.8568, 151.2153))
        );
        assert_eq!(
            parse_iso6709("+37.4219-122.0840"),
            Some((37.4219, -122.0840))
        );
        assert_eq!(parse_iso6709("+1530.5+07350.5/"), None);
        assert_eq!(parse_iso6709("garbage"), None);
        assert_eq!(parse_iso6709(""), None);
    }

    #[test]
    fn codec_names() {
        assert_eq!(video_codec(b"hvc1"), "hevc");
        assert_eq!(video_codec(b"avc1"), "h264");
        assert_eq!(audio_codec(b"mp4a"), "aac");
        assert_eq!(video_codec(b"zzzz"), "zzzz");
    }
}
