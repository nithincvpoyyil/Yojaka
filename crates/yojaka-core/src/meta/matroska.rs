//! Matroska / WebM metadata via a minimal EBML walker. Reads element headers and a few small
//! leaves; stops at the first `Cluster`, so media data is never read.

use std::io::SeekFrom;

use super::time::days_from_civil;
use super::{DateSrc, Extractor, FileInfo, Meta, MetaError, ReadSeek};
use crate::model::Format;

const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const TRACK_ENTRY: u32 = 0xAE;
const MAX_LEAF: u64 = 64 << 10;

#[derive(Debug)]
pub(crate) struct Matroska;

struct Elem {
    id: u32,
    body: u64,
    /// `None` = unknown size (live streams).
    end: Option<u64>,
}

fn vint(r: &mut dyn ReadSeek, keep_marker: bool) -> Result<Option<(u64, usize)>, MetaError> {
    let mut first = [0u8; 1];
    if r.read(&mut first)? == 0 {
        return Ok(None);
    }
    let len = first[0].leading_zeros() as usize + 1;
    if len > 8 {
        return Err(MetaError::Malformed("bad EBML length".into()));
    }
    let mut v = if keep_marker {
        u64::from(first[0])
    } else {
        u64::from(first[0]) & ((1u64 << (8 - len)) - 1)
    };
    let mut rest = vec![0u8; len - 1];
    r.read_exact(&mut rest)?;
    for b in rest {
        v = (v << 8) | u64::from(b);
    }
    Ok(Some((v, len)))
}

fn elem(r: &mut dyn ReadSeek, pos: u64) -> Result<Option<Elem>, MetaError> {
    r.seek(SeekFrom::Start(pos))?;
    let Some((id, il)) = vint(r, true)? else {
        return Ok(None);
    };
    let Some((size, sl)) = vint(r, false)? else {
        return Ok(None);
    };
    let body = pos + (il + sl) as u64;
    let unknown = size == (1u64 << (7 * sl)) - 1;
    Ok(Some(Elem {
        id: id as u32,
        body,
        end: (!unknown).then(|| body.saturating_add(size)),
    }))
}

fn bytes(r: &mut dyn ReadSeek, e: &Elem) -> Result<Vec<u8>, MetaError> {
    let n = e.end.unwrap_or(e.body) - e.body;
    if n > MAX_LEAF {
        return Err(MetaError::Malformed("oversized element".into()));
    }
    r.seek(SeekFrom::Start(e.body))?;
    let mut b = vec![0u8; n as usize];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn uint(b: &[u8]) -> u64 {
    b.iter().fold(0, |a, x| (a << 8) | u64::from(*x))
}

fn float(b: &[u8]) -> Option<f64> {
    match b.len() {
        4 => Some(f64::from(f32::from_be_bytes(b.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(b.try_into().ok()?)),
        _ => None,
    }
}

fn each(
    r: &mut dyn ReadSeek,
    start: u64,
    end: u64,
    f: &mut dyn FnMut(&mut dyn ReadSeek, &Elem) -> Result<bool, MetaError>,
) -> Result<(), MetaError> {
    let mut pos = start;
    while pos < end {
        let Some(e) = elem(r, pos)? else { break };
        if !f(r, &e)? {
            break;
        }
        let Some(next) = e.end else { break };
        pos = next;
    }
    Ok(())
}

fn codec(id: &str) -> String {
    match id {
        "V_MPEG4/ISO/AVC" => "h264",
        "V_MPEGH/ISO/HEVC" => "hevc",
        "V_VP9" => "vp9",
        "V_VP8" => "vp8",
        "V_AV1" => "av1",
        "A_AAC" => "aac",
        "A_OPUS" => "opus",
        "A_VORBIS" => "vorbis",
        "A_AC3" => "ac3",
        "A_EAC3" => "eac3",
        "A_FLAC" => "flac",
        other => return other.to_ascii_lowercase(),
    }
    .to_string()
}

impl Extractor for Matroska {
    fn formats(&self) -> &'static [Format] {
        &[Format::Mkv, Format::Webm]
    }

    fn extract(&self, r: &mut dyn ReadSeek, info: &FileInfo) -> Result<Meta, MetaError> {
        let mut meta = Meta {
            container: Some(info.format.as_str().to_string()),
            ..Meta::default()
        };
        let mut seg: Option<(u64, u64)> = None;
        each(r, 0, info.size, &mut |_, e| {
            if e.id == SEGMENT {
                seg = Some((e.body, e.end.unwrap_or(info.size).min(info.size)));
                return Ok(false);
            }
            Ok(true)
        })?;
        let (start, end) = seg.ok_or_else(|| MetaError::Malformed("no Segment element".into()))?;

        let mut scale = 1_000_000u64;
        let mut duration = None;
        each(r, start, end, &mut |r, e| {
            match e.id {
                CLUSTER => return Ok(false), // everything we need precedes the first cluster
                INFO => {
                    let ie = e.end.unwrap_or(end);
                    each(r, e.body, ie, &mut |r, c| {
                        let b = bytes(r, c)?;
                        match c.id {
                            0x2A_D7B1 => scale = uint(&b).max(1),
                            0x4489 => duration = float(&b),
                            0x4461 => {
                                // signed ns since 2001-01-01T00:00:00Z
                                let ns = uint(&b) as i64;
                                let secs = 978_307_200 + ns.div_euclid(1_000_000_000);
                                if secs > days_from_civil(1990, 1, 1) * 86_400 {
                                    meta.taken_at = Some(secs);
                                    meta.taken_at_src = DateSrc::Container;
                                }
                            }
                            _ => {}
                        }
                        Ok(true)
                    })?;
                }
                TRACKS => {
                    let te = e.end.unwrap_or(end);
                    each(r, e.body, te, &mut |r, t| {
                        if t.id == TRACK_ENTRY {
                            track(r, t, &mut meta)?;
                        }
                        Ok(true)
                    })?;
                }
                _ => {}
            }
            Ok(true)
        })?;
        if let Some(d) = duration {
            meta.duration_ms = Some((d * scale as f64 / 1_000_000.0).round() as u64);
        }
        if meta.duration_ms.is_none() && meta.width.is_none() {
            return Err(MetaError::Malformed("no Info or Tracks metadata".into()));
        }
        Ok(meta)
    }
}

fn track(r: &mut dyn ReadSeek, t: &Elem, meta: &mut Meta) -> Result<(), MetaError> {
    let (mut kind, mut codec_id, mut w, mut h, mut frame_ns) =
        (0u64, String::new(), None, None, None);
    each(r, t.body, t.end.unwrap_or(t.body), &mut |r, c| {
        match c.id {
            0x83 => kind = uint(&bytes(r, c)?),
            0x86 => {
                codec_id = String::from_utf8_lossy(&bytes(r, c)?)
                    .trim_end_matches('\0')
                    .to_string()
            }
            0x23_E383 => frame_ns = Some(uint(&bytes(r, c)?)),
            0xE0 => each(r, c.body, c.end.unwrap_or(c.body), &mut |r, v| {
                match v.id {
                    0xB0 => w = u32::try_from(uint(&bytes(r, v)?)).ok(),
                    0xBA => h = u32::try_from(uint(&bytes(r, v)?)).ok(),
                    _ => {}
                }
                Ok(true)
            })?,
            _ => {}
        }
        Ok(true)
    })?;
    match kind {
        1 if meta.width.is_none() => {
            meta.width = w;
            meta.height = h;
            meta.video_codec = Some(codec(&codec_id));
            meta.fps = frame_ns
                .filter(|n| *n > 0)
                .map(|n| (1e11 / n as f64).round() / 100.0);
        }
        2 if meta.audio_codec.is_none() => meta.audio_codec = Some(codec(&codec_id)),
        _ => {}
    }
    Ok(())
}
