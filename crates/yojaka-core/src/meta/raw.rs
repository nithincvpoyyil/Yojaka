//! Raw photos (DNG, CR2, CR3, NEF, ARW, RAF, ORF): metadata read by macOS ImageIO
//! ([`crate::sys::imageio::properties`]).
//!
//! The EXIF reader used for other pictures cannot read Fuji RAF, Canon CR3 or Olympus ORF files
//! at all (no size, date, camera or place), and takes a Nikon NEF's size from its 160×120
//! thumbnail. ImageIO reads all of them and gives the size Finder shows, which is also the size
//! of the JPEG the camera saves with the raw. It reads only what it needs (0.03 to 0.8 MB of the
//! raw files measured, against an 8 MiB head before). When ImageIO cannot read a file, the pass
//! falls back to the EXIF reader.

use std::fs::File;

use super::time::parse_exif_datetime;
use super::{DateSrc, Meta, MetaError};
use crate::geo::GeoPoint;
use crate::model::Format;
use crate::sys::imageio::{self, Props};

/// Raise this to read every raw photo's metadata again on the next scan (see
/// `pass::refresh_raw`). 1: read by ImageIO.
pub(crate) const RAW_META_VERSION: &str = "1";

/// Reads the metadata of the raw photo open in `file`.
pub(crate) fn extract(file: &File, format: Format) -> Result<Meta, MetaError> {
    let type_id = imageio::type_id(format).ok_or(MetaError::Unsupported)?;
    to_meta(
        imageio::properties(file, type_id).map_err(MetaError::Malformed)?,
        format,
    )
}

fn to_meta(p: Props, format: Format) -> Result<Meta, MetaError> {
    let taken_at = p.taken.as_deref().and_then(parse_exif_datetime);
    let m = Meta {
        width: p.width,
        height: p.height,
        taken_at,
        taken_at_src: if taken_at.is_some() {
            DateSrc::Exif
        } else {
            DateSrc::None
        },
        camera_make: p.make,
        camera_model: p.model,
        lens: p.lens,
        orientation: p.orientation,
        gps: p.gps.and_then(|(lat, lon)| GeoPoint::checked(lat, lon)),
        container: Some(format.as_str().to_string()),
        ..Meta::default()
    };
    // Same rule as the EXIF reader: nothing at all means the file is damaged or not a photo.
    if m.width.is_none() && m.taken_at.is_none() && m.camera_model.is_none() {
        return Err(MetaError::Malformed(
            "macOS found no size, date or camera in the raw file".into(),
        ));
    }
    Ok(m)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn what_imageio_says_becomes_the_saved_metadata() {
        let p = Props {
            width: Some(6016),
            height: Some(4016),
            orientation: Some(1),
            make: Some("NIKON CORPORATION".into()),
            model: Some("NIKON D750".into()),
            lens: Some("35.0 mm f/1.8".into()),
            taken: Some("2017:01:15 21:38:43".into()),
            gps: Some((-33.9249, 18.4241)),
        };
        let m = to_meta(p, Format::RawNef).unwrap();
        assert_eq!((m.width, m.height), (Some(6016), Some(4016)));
        assert_eq!(m.taken_at, parse_exif_datetime("2017:01:15 21:38:43"));
        assert_eq!(m.taken_at_src, DateSrc::Exif);
        assert_eq!(m.camera_model.as_deref(), Some("NIKON D750"));
        assert_eq!(m.lens.as_deref(), Some("35.0 mm f/1.8"));
        assert_eq!(m.container.as_deref(), Some("raw-nef"));
        let g = m.gps.unwrap();
        assert!((g.lat + 33.9249).abs() < 1e-9 && (g.lon - 18.4241).abs() < 1e-9);
    }

    #[test]
    fn a_bad_date_or_place_is_left_out_and_nothing_at_all_is_an_error() {
        let p = Props {
            width: Some(100),
            height: Some(80),
            taken: Some("0000:00:00 00:00:00".into()),
            gps: Some((0.0, 0.0)),
            ..Props::default()
        };
        let m = to_meta(p, Format::RawDng).unwrap();
        assert_eq!((m.taken_at, m.taken_at_src), (None, DateSrc::None));
        assert_eq!(m.gps, None);
        assert!(matches!(
            to_meta(Props::default(), Format::RawArw),
            Err(MetaError::Malformed(_))
        ));
    }
}
