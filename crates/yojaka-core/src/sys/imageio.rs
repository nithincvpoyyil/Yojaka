//! Pictures read by macOS itself: ImageIO, the framework behind Photos, Preview and `sips`.
//! Used for HEIC, AVIF and raw photos, which the `image` crate cannot read: their pixels for the
//! similar duplicate tier ([`decode_upright`]) and the metadata of raw photos ([`properties`]).
//!
//! Files are handed over already open (read-only, opened without following a symlink). ImageIO
//! reads them through `/dev/fd/N`, which is that same open file, and reads only what it needs.
//!
//! The ImageIO and Core Graphics bindings are `unsafe` functions, so this module allows
//! `unsafe` code; each block says why it is sound. Nothing here writes a file.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::File;
use std::os::fd::AsRawFd;

use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGColorSpaceModel, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyExifAuxDictionary, kCGImagePropertyExifAuxLensModel,
    kCGImagePropertyExifDateTimeDigitized, kCGImagePropertyExifDateTimeOriginal,
    kCGImagePropertyExifDictionary, kCGImagePropertyExifLensModel, kCGImagePropertyGPSDictionary,
    kCGImagePropertyGPSLatitude, kCGImagePropertyGPSLatitudeRef, kCGImagePropertyGPSLongitude,
    kCGImagePropertyGPSLongitudeRef, kCGImagePropertyOrientation, kCGImagePropertyPixelHeight,
    kCGImagePropertyPixelWidth, kCGImagePropertyTIFFDateTime, kCGImagePropertyTIFFDictionary,
    kCGImagePropertyTIFFMake, kCGImagePropertyTIFFModel,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailFromImageIfAbsent,
    kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceShouldCache,
    kCGImageSourceThumbnailMaxPixelSize, kCGImageSourceTypeIdentifierHint,
};

use crate::model::Format;

/// The macOS type of each format read through ImageIO here; `None` for the rest. ImageIO must
/// be told: it takes most raw files for plain TIFF otherwise (a Sony ARW then fails, and a
/// Nikon NEF is read as its 160×120 thumbnail).
pub fn type_id(format: Format) -> Option<&'static str> {
    use Format::*;
    Some(match format {
        Heic => "public.heic",
        Avif => "public.avif",
        RawDng => "com.adobe.raw-image",
        RawCr2 => "com.canon.cr2-raw-image",
        RawCr3 => "com.canon.cr3-raw-image",
        RawNef => "com.nikon.raw-image",
        RawArw => "com.sony.arw-raw-image",
        RawRaf => "com.fuji.raw-image",
        RawOrf => "com.olympus.or-raw-image",
        Jpeg | Png | Gif | Webp | Tiff | Bmp => return None,
        Mov | Mp4 | M4v | ThreeGp | Avi | Mkv | Webm | Mts => return None,
    })
}

/// A decoded picture: 8-bit red, green, blue per pixel, row by row, top row first.
#[derive(Debug)]
pub struct Rgb8 {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// Which pixels to read from a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pixels {
    /// The full picture. A small preview stored in the file is never used: it can be out of
    /// date after an edit, or have black bars. For HEIC and AVIF.
    Full,
    /// For raw photos: the camera's own preview inside the file. It is the same rendering as
    /// the JPEG the camera saves next to the raw, and about ten times faster to read than
    /// developing the raw (whose colours come out differently). Used only when its longer side
    /// is at least [`MIN_PREVIEW_SIDE`] and it has the picture's proportions; otherwise the raw
    /// is developed.
    RawPreview,
}

/// A raw preview smaller than this is not used (old cameras store a tiny one, often with black
/// bars).
pub const MIN_PREVIEW_SIDE: usize = 256;
/// A raw preview whose long side over short side differs from the picture's by more than this
/// is cropped or letterboxed, so it is not used.
const PREVIEW_SHAPE_TOLERANCE: f64 = 0.03;

/// Decodes the main picture in `file` (a file of type `type_id`, see [`type_id`]), turned
/// upright as its rotation flag says, and shrunk so its longer side is at most `max_side`
/// pixels (never enlarged). For a raw photo only the preview is read: a few MB of 10 to 60.
///
/// Colours are not converted: the values are the picture's own (for example Display P3), the
/// same as the `image` crate gives for other formats. Returns why it failed as text.
pub fn decode_upright(
    file: &File,
    type_id: &str,
    pixels: Pixels,
    max_side: usize,
) -> Result<Rgb8, String> {
    // ImageIO may hand Objective-C objects to the autorelease pool; worker threads have no
    // pool of their own, so each call gets one.
    objc2::rc::autoreleasepool(|_| decode_inner(file, type_id, pixels, max_side))
}

/// What a raw photo's file says about it, read by ImageIO without decoding the picture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Props {
    /// The picture's size as Finder shows it (the developed photo, not the sensor's edges),
    /// before its rotation flag is applied.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<u16>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    /// EXIF date text (`YYYY:MM:DD HH:MM:SS`): when taken, else digitized, else the file's.
    pub taken: Option<String>,
    /// Latitude and longitude in degrees; south and west are negative.
    pub gps: Option<(f64, f64)>,
}

/// Reads the size, date, camera, lens, place and rotation of the main picture in `file`.
/// ImageIO reads only the parts it needs (for the raw files measured, 0.03 to 0.8 MB).
pub fn properties(file: &File, type_id: &str) -> Result<Props, String> {
    objc2::rc::autoreleasepool(|_| properties_inner(file, type_id))
}

/// The option and property keys ImageIO exports.
struct Keys {
    cache: &'static CFString,
    hint: &'static CFString,
    always: &'static CFString,
    if_absent: &'static CFString,
    transform: &'static CFString,
    max_size: &'static CFString,
    width: &'static CFString,
    height: &'static CFString,
}

fn keys() -> Keys {
    // SAFETY: constant CFStrings exported by ImageIO; reading them has no side effects.
    unsafe {
        Keys {
            cache: kCGImageSourceShouldCache,
            hint: kCGImageSourceTypeIdentifierHint,
            always: kCGImageSourceCreateThumbnailFromImageAlways,
            if_absent: kCGImageSourceCreateThumbnailFromImageIfAbsent,
            transform: kCGImageSourceCreateThumbnailWithTransform,
            max_size: kCGImageSourceThumbnailMaxPixelSize,
            width: kCGImagePropertyPixelWidth,
            height: kCGImagePropertyPixelHeight,
        }
    }
}

type Dict = CFDictionary<CFString, CFType>;

/// An image source over `file`, and the index of its main picture.
fn open_source(
    file: &File,
    type_id: &str,
    k: &Keys,
) -> Result<(CFRetained<CGImageSource>, usize), String> {
    // `/dev/fd/N` opens the same file as descriptor N (see `man 4 fd`), so ImageIO reads the
    // file opened by the caller, not whatever the path points to now.
    let url = CFURL::from_file_path(format!("/dev/fd/{}", file.as_raw_fd()))
        .ok_or("the file cannot be given to macOS")?;
    let no: &CFType = CFBoolean::new(false).as_ref();
    let hint = CFString::from_str(type_id);
    let opts: CFRetained<Dict> =
        CFDictionary::from_slices(&[k.cache, k.hint], &[no, hint.as_ref()]);
    // SAFETY: the options have CFString keys, a CFBoolean for `kCGImageSourceShouldCache` and
    // a CFString for `kCGImageSourceTypeIdentifierHint`, as ImageIO documents.
    let source = unsafe { CGImageSource::with_url(&url, Some(opts.as_opaque())) }
        .ok_or("not a picture macOS can read")?;
    // SAFETY: `source` is a valid image source.
    let index = unsafe { source.primary_image_index() };
    Ok((source, index))
}

/// The property dictionary of the picture at `index`.
fn props_of(source: &CGImageSource, index: usize) -> Option<CFRetained<Dict>> {
    // SAFETY: `index` comes from this source; no options.
    let props = unsafe { source.properties_at_index(index, None) }?;
    // SAFETY: ImageIO property dictionaries have CFString keys and CFType values.
    Some(unsafe { CFRetained::cast_unchecked::<Dict>(props) })
}

/// A dictionary inside `d` (the EXIF, TIFF or GPS part).
fn sub(d: &Dict, key: &CFString) -> Option<CFRetained<Dict>> {
    let inner = d.get(key)?.downcast::<CFDictionary>().ok()?;
    // SAFETY: ImageIO's nested property dictionaries have CFString keys and CFType values.
    Some(unsafe { CFRetained::cast_unchecked::<Dict>(inner) })
}

fn text(d: &Dict, key: &CFString) -> Option<String> {
    let s = d.get(key)?.downcast::<CFString>().ok()?.to_string();
    let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!s.is_empty()).then(|| s.to_string())
}

fn number(d: &Dict, key: &CFString) -> Option<f64> {
    d.get(key)?.downcast::<CFNumber>().ok()?.as_f64()
}

/// A positive whole number that fits in `T` (sizes, orientation).
fn whole<T: TryFrom<i64>>(d: &Dict, key: &CFString) -> Option<T> {
    let v = number(d, key)?;
    (v.fract() == 0.0 && v > 0.0)
        .then(|| T::try_from(v as i64).ok())
        .flatten()
}

fn properties_inner(file: &File, type_id: &str) -> Result<Props, String> {
    let k = keys();
    let (source, index) = open_source(file, type_id, &k)?;
    let p = props_of(&source, index).ok_or("macOS found no picture in the file")?;
    // SAFETY: constant CFStrings exported by ImageIO.
    let (orientation, exif_d, tiff_d, gps_d, aux_d) = unsafe {
        (
            kCGImagePropertyOrientation,
            kCGImagePropertyExifDictionary,
            kCGImagePropertyTIFFDictionary,
            kCGImagePropertyGPSDictionary,
            kCGImagePropertyExifAuxDictionary,
        )
    };
    let exif = sub(&p, exif_d);
    let tiff = sub(&p, tiff_d);
    let aux = sub(&p, aux_d);
    // SAFETY: constant CFStrings exported by ImageIO.
    let taken = unsafe {
        [
            (exif.as_deref(), kCGImagePropertyExifDateTimeOriginal),
            (exif.as_deref(), kCGImagePropertyExifDateTimeDigitized),
            (tiff.as_deref(), kCGImagePropertyTIFFDateTime),
        ]
    }
    .into_iter()
    .find_map(|(d, key)| text(d?, key));
    // SAFETY: constant CFStrings exported by ImageIO.
    let (make, model, lens, aux_lens) = unsafe {
        (
            kCGImagePropertyTIFFMake,
            kCGImagePropertyTIFFModel,
            kCGImagePropertyExifLensModel,
            kCGImagePropertyExifAuxLensModel,
        )
    };
    Ok(Props {
        width: whole(&p, k.width),
        height: whole(&p, k.height),
        orientation: whole(&p, orientation),
        make: tiff.as_deref().and_then(|d| text(d, make)),
        model: tiff.as_deref().and_then(|d| text(d, model)),
        lens: exif
            .as_deref()
            .and_then(|d| text(d, lens))
            .or_else(|| aux.as_deref().and_then(|d| text(d, aux_lens))),
        taken,
        gps: sub(&p, gps_d).and_then(|g| gps(&g)),
    })
}

/// ImageIO gives GPS as positive degrees plus `N`/`S` and `E`/`W`.
fn gps(g: &Dict) -> Option<(f64, f64)> {
    // SAFETY: constant CFStrings exported by ImageIO.
    let (lat, lat_ref, lon, lon_ref) = unsafe {
        (
            kCGImagePropertyGPSLatitude,
            kCGImagePropertyGPSLatitudeRef,
            kCGImagePropertyGPSLongitude,
            kCGImagePropertyGPSLongitudeRef,
        )
    };
    let sign = |r: Option<String>, negative: &str| match r {
        Some(r) if r.eq_ignore_ascii_case(negative) => -1.0,
        _ => 1.0,
    };
    let lat = number(g, lat)?.abs() * sign(text(g, lat_ref), "S");
    let lon = number(g, lon)?.abs() * sign(text(g, lon_ref), "W");
    Some((lat, lon))
}

fn decode_inner(
    file: &File,
    type_id: &str,
    pixels: Pixels,
    max_side: usize,
) -> Result<Rgb8, String> {
    let k = keys();
    let (source, index) = open_source(file, type_id, &k)?;
    let yes: &CFType = CFBoolean::new(true).as_ref();
    let no: &CFType = CFBoolean::new(false).as_ref();
    let max = CFNumber::new_isize(isize::try_from(max_side).unwrap_or(isize::MAX));

    // Upright (`transform`), at most `max_side`, never kept in ImageIO's cache.
    let thumbnail = |from_full: bool| {
        let from = if from_full { k.always } else { k.if_absent };
        let opts: CFRetained<Dict> = CFDictionary::from_slices(
            &[from, k.transform, k.max_size, k.cache],
            &[yes, yes, max.as_ref(), no],
        );
        // SAFETY: CFString keys with CFBoolean / CFNumber values, the types ImageIO documents
        // for these keys; `index` comes from this source.
        unsafe { source.thumbnail_at_index(index, Some(opts.as_opaque())) }
    };
    let image = match pixels {
        Pixels::Full => thumbnail(true),
        Pixels::RawPreview => {
            // `if_absent`: the preview stored in the file, or one made from the raw if there is
            // none. Its size is checked against the picture's own.
            let full_shape = props_of(&source, index).and_then(|p| {
                let (w, h): (u32, u32) = (whole(&p, k.width)?, whole(&p, k.height)?);
                let (w, h) = (f64::from(w), f64::from(h));
                Some(w.max(h) / w.min(h))
            });
            match thumbnail(false) {
                Some(preview) if preview_fits(&preview, full_shape) => Some(preview),
                _ => thumbnail(true),
            }
        }
    }
    .ok_or("macOS could not decode the picture")?;
    to_rgb8(&image)
}

/// A raw preview is used when it is big enough and has the picture's proportions (so it is
/// not cropped or letterboxed). When the file does not say its size, size alone decides.
fn preview_fits(preview: &CGImage, full_shape: Option<f64>) -> bool {
    size_fits(
        CGImage::width(Some(preview)),
        CGImage::height(Some(preview)),
        full_shape,
    )
}

fn size_fits(w: usize, h: usize, full_shape: Option<f64>) -> bool {
    if w.max(h) < MIN_PREVIEW_SIDE || w.min(h) == 0 {
        return false;
    }
    let shape = w.max(h) as f64 / w.min(h) as f64;
    full_shape.is_none_or(|full| (shape - full).abs() <= PREVIEW_SHAPE_TOLERANCE * full)
}

fn to_rgb8(image: &CGImage) -> Result<Rgb8, String> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    if width == 0 || height == 0 {
        return Err("the picture is empty".into());
    }
    let own_space = CGImage::color_space(Some(image))
        .filter(|s| CGColorSpace::model(Some(s)) == CGColorSpaceModel::RGB);
    let rgbx = match own_space.and_then(|s| draw(image, width, height, &s)) {
        Some(px) => px,
        // Grey, CMYK or an RGB space Core Graphics cannot draw into: draw in sRGB instead.
        None => {
            // SAFETY: a constant CFString exported by Core Graphics.
            let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
                .ok_or("no sRGB colour space")?;
            draw(image, width, height, &srgb).ok_or("could not draw the picture")?
        }
    };
    let pixels = rgbx
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[r, g, b, _]| [*r, *g, *b])
        .collect();
    Ok(Rgb8 {
        width,
        height,
        pixels,
    })
}

/// Draws `image` at its own size into 8-bit RGBX memory in colour space `space`.
fn draw(image: &CGImage, width: usize, height: usize, space: &CGColorSpace) -> Option<Vec<u8>> {
    let row = width.checked_mul(4)?;
    let mut buf = vec![0u8; row.checked_mul(height)?];
    let info = CGImageAlphaInfo::NoneSkipLast.0 | CGImageByteOrderInfo::OrderDefault.0;
    {
        // SAFETY: `buf` holds exactly `row * height` bytes, which is what a context of this
        // width, height, 8 bits per component and 4 bytes per pixel uses. The context is
        // dropped at the end of this block, before `buf` is read or freed.
        let ctx = unsafe {
            CGBitmapContextCreate(
                buf.as_mut_ptr().cast::<c_void>(),
                width,
                height,
                8,
                row,
                Some(space),
                info,
            )
        }?;
        let rect = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        );
        CGContext::draw_image(Some(&ctx), rect, Some(image));
    }
    Some(buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_raw_preview_must_be_big_enough_and_have_the_pictures_shape() {
        let sony = Some(6000.0 / 4000.0);
        // What the cameras measured here give: 512×341 or 512×342, upright or sideways.
        assert!(size_fits(512, 341, sony));
        assert!(size_fits(342, 512, sony));
        // The 160×120 thumbnail with black bars that older cameras store for a 3:2 picture.
        assert!(!size_fits(160, 120, sony));
        // Big enough but letterboxed (4:3 around a 3:2 picture).
        assert!(!size_fits(512, 384, sony));
        assert!(!size_fits(0, 512, None));
        // The file did not say its size: size alone decides.
        assert!(size_fits(512, 384, None));
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fake_heic = vec![0, 0, 0, 24];
        fake_heic.extend(b"ftypheic\0\0\0\0mif1heic");
        fake_heic.extend([0u8; 64]);
        let files: [&[u8]; 4] = [
            b"",
            b"not a picture at all",
            &fake_heic,
            // A TIFF header with nothing behind it (what most raw files start with).
            b"II*\0\x08\0\0\0\0\0",
        ];
        for (type_id, pixels) in [
            ("public.heic", Pixels::Full),
            ("public.avif", Pixels::Full),
            ("com.sony.arw-raw-image", Pixels::RawPreview),
            ("com.adobe.raw-image", Pixels::RawPreview),
        ] {
            for (i, bytes) in files.iter().enumerate() {
                let p = tmp.path().join(format!("{i}.bin"));
                std::fs::write(&p, bytes).unwrap();
                let file = File::open(&p).unwrap();
                assert!(decode_upright(&file, type_id, pixels, 512).is_err());
                // Properties of garbage: an error or nothing, never a crash.
                if let Ok(p) = properties(&file, type_id) {
                    assert_eq!((p.width, p.taken), (None, None));
                }
            }
        }
    }

    #[test]
    fn every_heic_avif_and_raw_format_has_a_type_and_nothing_else_does() {
        use Format::*;
        for f in [
            Heic, Avif, RawDng, RawCr2, RawCr3, RawNef, RawArw, RawRaf, RawOrf,
        ] {
            assert!(type_id(f).is_some(), "{f:?}");
        }
        for f in [Jpeg, Png, Gif, Webp, Tiff, Bmp, Mov, Mp4] {
            assert_eq!(type_id(f), None, "{f:?}");
        }
    }

    #[test]
    fn properties_give_size_date_camera_rotation_and_place() {
        use fixturegen::pictures;
        let tmp = tempfile::tempdir().unwrap();
        // Stored sideways (orientation 6), taken in Cape Town: south and east.
        let img = pictures::detailed(3, 320, 240);
        let jpeg = pictures::jpeg_at(&img, 90, 6, Some((-33.9249, 18.4241))).unwrap();
        let path = tmp.path().join("a.jpg");
        std::fs::write(&path, &jpeg).unwrap();
        let p = properties(&File::open(&path).unwrap(), "public.jpeg").unwrap();
        assert_eq!((p.width, p.height), (Some(320), Some(240)));
        assert_eq!(p.orientation, Some(6));
        assert_eq!(p.make.as_deref(), Some("Apple"));
        assert_eq!(p.model.as_deref(), Some("iPhone 15"));
        assert_eq!(p.taken.as_deref(), Some("2024:05:01 10:00:00"));
        let (lat, lon) = p.gps.unwrap();
        assert!(
            (lat + 33.9249).abs() < 1e-3 && (lon - 18.4241).abs() < 1e-3,
            "{lat} {lon}"
        );
        // New York: north and west.
        let jpeg = pictures::jpeg_at(&img, 90, 1, Some((40.7128, -74.006))).unwrap();
        std::fs::write(&path, &jpeg).unwrap();
        let (lat, lon) = properties(&File::open(&path).unwrap(), "public.jpeg")
            .unwrap()
            .gps
            .unwrap();
        assert!(
            (lat - 40.7128).abs() < 1e-3 && (lon + 74.006).abs() < 1e-3,
            "{lat} {lon}"
        );
        // No GPS, no place.
        let jpeg = pictures::jpeg(&img, 90, 1).unwrap();
        std::fs::write(&path, &jpeg).unwrap();
        assert_eq!(
            properties(&File::open(&path).unwrap(), "public.jpeg")
                .unwrap()
                .gps,
            None
        );
    }

    #[test]
    fn imageio_reads_the_open_file_not_what_the_path_points_to_now() {
        use fixturegen::pictures;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.jpg");
        let landscape = pictures::jpeg(&pictures::detailed(4, 320, 240), 90, 1).unwrap();
        std::fs::write(&path, &landscape).unwrap();
        let file = File::open(&path).unwrap();
        // Replaced after it was opened: the open file is still the landscape picture.
        std::fs::remove_file(&path).unwrap();
        let portrait = pictures::jpeg(&pictures::detailed(5, 240, 320), 90, 1).unwrap();
        std::fs::write(&path, &portrait).unwrap();
        let p = properties(&file, "public.jpeg").unwrap();
        assert_eq!((p.width, p.height), (Some(320), Some(240)));
        let pic = decode_upright(&file, "public.jpeg", Pixels::Full, 512).unwrap();
        assert_eq!((pic.width, pic.height), (320, 240));
    }
}
