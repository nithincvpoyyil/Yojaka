//! Single pictures for tests that need real, decodable files rather than a whole tree.

use image::{ExtendedColorType, RgbImage};

use crate::GenError;
use crate::exif_writer::{ExifData, splice_into_jpeg};
use crate::rng::Rng;

/// A picture with detail at several scales, like the look-alike sets in a tree. The same seed
/// gives the same picture.
pub fn detailed(seed: u64, w: u32, h: u32) -> RgbImage {
    crate::tree::detailed_picture(&mut Rng::new(seed), w, h)
}

/// The picture as a JPEG with an EXIF orientation tag (1: stored upright; 6: stored turned a
/// quarter to the left, so a viewer turns it a quarter to the right).
pub fn jpeg(img: &RgbImage, quality: u8, orientation: u16) -> Result<Vec<u8>, GenError> {
    jpeg_at(img, quality, orientation, None)
}

/// Like [`jpeg`], with a GPS position (latitude, longitude; south and west negative).
pub fn jpeg_at(
    img: &RgbImage,
    quality: u8,
    orientation: u16,
    gps: Option<(f64, f64)>,
) -> Result<Vec<u8>, GenError> {
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| GenError::Image(e.to_string()))?;
    Ok(splice_into_jpeg(
        &buf,
        &ExifData {
            make: "Apple",
            model: "iPhone 15",
            orientation,
            date_time_original: "2024:05:01 10:00:00",
            gps,
        },
    ))
}

/// The same picture as HEIC, converted by macOS `sips` and cached in
/// `~/.cache/yojaka-fixtures`. `None` when `sips` is missing or fails.
pub fn heic_from_jpeg(jpeg: &[u8]) -> Option<Vec<u8>> {
    convert_with_sips(jpeg, "heic")
}

/// The same picture as AVIF (macOS 13 or later), like [`heic_from_jpeg`].
pub fn avif_from_jpeg(jpeg: &[u8]) -> Option<Vec<u8>> {
    convert_with_sips(jpeg, "avif")
}

fn convert_with_sips(jpeg: &[u8], format: &str) -> Option<Vec<u8>> {
    let cache = directories::BaseDirs::new()?
        .home_dir()
        .join(".cache")
        .join("yojaka-fixtures");
    let key = blake3::hash(jpeg).to_hex();
    let cached = cache.join(format!("{}.{format}", &key.as_str()[..32]));
    if let Ok(b) = std::fs::read(&cached) {
        return Some(b);
    }
    std::fs::create_dir_all(&cache).ok()?;
    // Tests run in parallel threads and processes: each converts through its own temporary
    // files and renames the result into place, so nobody reads a half-written file.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = format!("{}.{}.{n}", &key.as_str()[..32], std::process::id());
    let src = cache.join(format!("{tmp}.jpg"));
    let out = cache.join(format!("{tmp}.{format}"));
    std::fs::write(&src, jpeg).ok()?;
    let ok = std::process::Command::new("sips")
        .args(["-s", "format", format])
        .arg(&src)
        .arg("--out")
        .arg(&out)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&src);
    let bytes = std::fs::read(&out).ok().filter(|_| ok);
    if bytes.is_some() {
        let _ = std::fs::rename(&out, &cached);
    }
    let _ = std::fs::remove_file(&out);
    bytes
}
