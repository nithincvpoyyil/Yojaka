//! Image metadata: EXIF (JPEG, TIFF and TIFF-based RAW, HEIC/AVIF, PNG, WebP), dimensions,
//! and animation frame counts. Only a capped head of the file is read; pixels are never decoded.

use std::io::{Cursor, Read, SeekFrom};

use exif::{In, Reader, Tag, Value};

use super::time::parse_exif_datetime;
use super::{DateSrc, Extractor, FileInfo, Meta, MetaError, ReadSeek};
use crate::geo::GeoPoint;
use crate::model::Format;

/// Head bytes read for formats whose EXIF sits near the start.
const SMALL_CAP: u64 = 1 << 20;
/// Head bytes read for TIFF-family and ISO-BMFF images, whose EXIF can sit further in.
const LARGE_CAP: u64 = 8 << 20;
/// Dimension sniffing only needs a header; JPEG SOF can follow a large APP1.
const DIM_CAP: usize = 512 << 10;

#[derive(Debug)]
pub(crate) struct ImageExtractor;

impl Extractor for ImageExtractor {
    fn formats(&self) -> &'static [Format] {
        use Format::*;
        &[
            Jpeg, Png, Gif, Heic, Webp, Tiff, Bmp, Avif, RawDng, RawCr2, RawCr3, RawNef, RawArw,
            RawRaf, RawOrf,
        ]
    }

    fn extract(&self, r: &mut dyn ReadSeek, info: &FileInfo) -> Result<Meta, MetaError> {
        let cap = match info.format {
            Format::Jpeg | Format::Png | Format::Gif | Format::Webp | Format::Bmp => SMALL_CAP,
            _ => LARGE_CAP,
        };
        // GIF frame counting walks the whole block stream, so give it more room.
        let cap = if info.format == Format::Gif {
            LARGE_CAP
        } else {
            cap
        };
        let mut head = Vec::new();
        r.seek(SeekFrom::Start(0))?;
        r.take(cap).read_to_end(&mut head)?;
        let truncated = (head.len() as u64) < info.size;
        if head.is_empty() {
            return Err(MetaError::Malformed("empty file".into()));
        }

        let mut meta = Meta {
            container: Some(info.format.as_str().to_string()),
            ..Meta::default()
        };
        let mut exif_error = None;
        match Reader::new().read_from_container(&mut Cursor::new(&head)) {
            Ok(exif) => apply_exif(&exif, &mut meta),
            // "no EXIF" is normal (PNG, GIF, screenshots). Only real damage is an error.
            Err(exif::Error::NotFound(_)) => {}
            Err(e) => exif_error = Some(e.to_string()),
        }

        if meta.width.is_none() || meta.height.is_none() {
            let size = imagesize::blob_size(&head[..head.len().min(DIM_CAP)])
                .ok()
                .or_else(|| imagesize::blob_size(&head).ok());
            if let Some(s) = size {
                meta.width = u32::try_from(s.width).ok();
                meta.height = u32::try_from(s.height).ok();
            }
        }
        match info.format {
            Format::Png => meta.frames = png_frames(&head),
            Format::Gif if !truncated => {
                meta.frames = Some(gif_frames(&head).ok_or_else(|| {
                    MetaError::Malformed("GIF data is damaged or cut short".into())
                })?);
            }
            Format::Webp if !truncated => meta.frames = webp_frames(&head),
            _ => {}
        }

        // A file we can read neither EXIF nor dimensions from is damaged or not an image.
        if meta.width.is_none() && meta.taken_at.is_none() && meta.camera_model.is_none() {
            return Err(MetaError::Malformed(
                exif_error.unwrap_or_else(|| "unreadable image header".into()),
            ));
        }
        Ok(meta)
    }
}

fn ascii(exif: &exif::Exif, tag: Tag) -> Option<String> {
    let f = exif.get_field(tag, In::PRIMARY)?;
    let Value::Ascii(parts) = &f.value else {
        return None;
    };
    let s = String::from_utf8_lossy(parts.first()?)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string();
    (!s.is_empty()).then_some(s)
}

fn uint(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)?.value.get_uint(0)
}

fn coord(exif: &exif::Exif, value: Tag, reference: Tag, negative: u8) -> Option<f64> {
    let f = exif.get_field(value, In::PRIMARY)?;
    let Value::Rational(r) = &f.value else {
        return None;
    };
    if r.len() < 3 || r.iter().any(|x| x.denom == 0) {
        return None;
    }
    let v = r[0].to_f64() + r[1].to_f64() / 60.0 + r[2].to_f64() / 3600.0;
    let sign = match ascii(exif, reference)?.bytes().next()? {
        b if b == negative => -1.0,
        _ => 1.0,
    };
    Some(sign * v)
}

fn apply_exif(exif: &exif::Exif, m: &mut Meta) {
    m.camera_make = ascii(exif, Tag::Make);
    m.camera_model = ascii(exif, Tag::Model);
    m.lens = ascii(exif, Tag::LensModel);
    m.orientation = uint(exif, Tag::Orientation).and_then(|o| u16::try_from(o).ok());

    for tag in [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime] {
        if let Some(t) = ascii(exif, tag).and_then(|s| parse_exif_datetime(&s)) {
            m.taken_at = Some(t);
            m.taken_at_src = DateSrc::Exif;
            break;
        }
    }

    m.width = uint(exif, Tag::PixelXDimension).or_else(|| uint(exif, Tag::ImageWidth));
    m.height = uint(exif, Tag::PixelYDimension).or_else(|| uint(exif, Tag::ImageLength));

    let lat = coord(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S');
    let lon = coord(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W');
    if let (Some(lat), Some(lon)) = (lat, lon) {
        m.gps = GeoPoint::checked(lat, lon);
    }
}

/// APNG: the `acTL` chunk (before the first `IDAT`) holds the frame count.
fn png_frames(b: &[u8]) -> Option<u32> {
    let mut pos = 8;
    while pos + 8 <= b.len() {
        let len = u32::from_be_bytes(b[pos..pos + 4].try_into().ok()?) as usize;
        match &b[pos + 4..pos + 8] {
            b"acTL" => {
                let n = b.get(pos + 8..pos + 12)?;
                return Some(u32::from_be_bytes(n.try_into().ok()?));
            }
            b"IDAT" | b"IEND" => return None,
            _ => {}
        }
        pos = pos.checked_add(12)?.checked_add(len)?;
    }
    None
}

/// Counts image descriptors (`0x2C`) by walking the block structure.
fn gif_frames(b: &[u8]) -> Option<u32> {
    let flags = *b.get(10)?;
    let mut pos = 13;
    if flags & 0x80 != 0 {
        pos += 3 * (1 << ((flags & 7) + 1));
    }
    let skip_sub_blocks = |mut p: usize| -> Option<usize> {
        loop {
            let n = usize::from(*b.get(p)?);
            p += 1;
            if n == 0 {
                return Some(p);
            }
            p += n;
        }
    };
    let mut frames = 0u32;
    loop {
        match *b.get(pos)? {
            0x3B => return Some(frames),
            0x21 => pos = skip_sub_blocks(pos + 2)?,
            0x2C => {
                frames += 1;
                let f = *b.get(pos + 9)?;
                pos += 10;
                if f & 0x80 != 0 {
                    pos += 3 * (1 << ((f & 7) + 1));
                }
                pos = skip_sub_blocks(pos + 1)?;
            }
            _ => return None,
        }
    }
}

/// Animated WebP: count `ANMF` chunks; a still image has 1 frame.
fn webp_frames(b: &[u8]) -> Option<u32> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    let mut pos = 12;
    let mut frames = 0u32;
    let mut animated = false;
    while pos + 8 <= b.len() {
        let len = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().ok()?) as usize;
        match &b[pos..pos + 4] {
            b"VP8X" => animated = b.get(pos + 8)? & 0x02 != 0,
            b"ANMF" => frames += 1,
            _ => {}
        }
        pos = pos.checked_add(8)?.checked_add(len + (len & 1))?;
    }
    Some(if animated { frames } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bytes: &[u8], format: Format) -> Result<Meta, MetaError> {
        ImageExtractor.extract(
            &mut Cursor::new(bytes.to_vec()),
            &FileInfo {
                size: bytes.len() as u64,
                format,
            },
        )
    }

    /// 2-frame 1x1 GIF.
    fn gif2() -> Vec<u8> {
        let mut g = b"GIF89a".to_vec();
        g.extend([1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
        for _ in 0..2 {
            g.extend([0x21, 0xF9, 4, 0, 0, 0, 0, 0]); // graphic control ext
            g.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]); // image descriptor
            g.extend([2, 2, 0x4C, 0x01, 0]); // lzw min size + one sub-block + terminator
        }
        g.push(0x3B);
        g
    }

    #[test]
    fn gif_dimensions_and_frames() {
        let m = run(&gif2(), Format::Gif).unwrap();
        assert_eq!((m.width, m.height, m.frames), (Some(1), Some(1), Some(2)));
    }

    #[test]
    fn truncated_gif_is_an_error() {
        let mut g = gif2();
        g.truncate(g.len() - 8);
        assert!(matches!(run(&g, Format::Gif), Err(MetaError::Malformed(_))));
        // header only
        assert!(run(&gif2()[..20], Format::Gif).is_err());
    }

    #[test]
    fn png_dimensions_and_apng_frames() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let chunk = |t: &[u8; 4], d: &[u8]| {
            let mut c = (d.len() as u32).to_be_bytes().to_vec();
            c.extend(t);
            c.extend(d);
            c.extend([0, 0, 0, 0]); // crc is not checked
            c
        };
        let mut ihdr = 7u32.to_be_bytes().to_vec();
        ihdr.extend(5u32.to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"acTL", &[0, 0, 0, 3, 0, 0, 0, 0]));
        png.extend(chunk(b"IDAT", &[]));
        png.extend(chunk(b"IEND", &[]));
        let m = run(&png, Format::Png).unwrap();
        assert_eq!((m.width, m.height, m.frames), (Some(7), Some(5), Some(3)));
    }

    #[test]
    fn garbage_is_a_malformed_error_not_a_panic() {
        assert!(matches!(
            run(&[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3], Format::Jpeg),
            Err(MetaError::Malformed(_))
        ));
        assert!(run(&[], Format::Png).is_err());
        assert!(run(b"not an image at all", Format::Tiff).is_err());
    }

    #[test]
    fn webp_still_is_one_frame() {
        let mut w = b"RIFF".to_vec();
        w.extend(30u32.to_le_bytes());
        w.extend(b"WEBPVP8X");
        w.extend(10u32.to_le_bytes());
        w.extend([0, 0, 0, 0, 9, 0, 0, 4, 0, 0]); // canvas 10 x 5
        assert_eq!(webp_frames(&w), Some(1));
    }
}
