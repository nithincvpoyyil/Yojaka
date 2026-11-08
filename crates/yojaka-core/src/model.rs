//! Core domain types (shapes). Filled in by later iterations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Jpeg,
    Png,
    Gif,
    Heic,
    Webp,
    Tiff,
    Bmp,
    Avif,
    RawDng,
    RawCr2,
    RawCr3,
    RawNef,
    RawArw,
    RawRaf,
    RawOrf,
    Mov,
    Mp4,
    M4v,
    ThreeGp,
    Avi,
    Mkv,
    Webm,
    Mts,
}

impl Format {
    pub fn kind(self) -> MediaKind {
        use Format::*;
        match self {
            Jpeg | Png | Gif | Heic | Webp | Tiff | Bmp | Avif | RawDng | RawCr2 | RawCr3
            | RawNef | RawArw | RawRaf | RawOrf => MediaKind::Image,
            Mov | Mp4 | M4v | ThreeGp | Avi | Mkv | Webm | Mts => MediaKind::Video,
        }
    }

    /// The kebab-case name stored in the index (same as the serde name).
    pub fn as_str(self) -> &'static str {
        use Format::*;
        match self {
            Jpeg => "jpeg",
            Png => "png",
            Gif => "gif",
            Heic => "heic",
            Webp => "webp",
            Tiff => "tiff",
            Bmp => "bmp",
            Avif => "avif",
            RawDng => "raw-dng",
            RawCr2 => "raw-cr2",
            RawCr3 => "raw-cr3",
            RawNef => "raw-nef",
            RawArw => "raw-arw",
            RawRaf => "raw-raf",
            RawOrf => "raw-orf",
            Mov => "mov",
            Mp4 => "mp4",
            M4v => "m4v",
            ThreeGp => "three-gp",
            Avi => "avi",
            Mkv => "mkv",
            Webm => "webm",
            Mts => "mts",
        }
    }

    /// Case-insensitive lookup by file extension (no leading dot).
    pub fn from_extension(ext: &str) -> Option<Self> {
        use Format::*;
        Some(match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jpe" => Jpeg,
            "png" => Png,
            "gif" => Gif,
            "heic" | "heif" => Heic,
            "webp" => Webp,
            "tif" | "tiff" => Tiff,
            "bmp" => Bmp,
            "avif" => Avif,
            "dng" => RawDng,
            "cr2" => RawCr2,
            "cr3" => RawCr3,
            "nef" => RawNef,
            "arw" => RawArw,
            "raf" => RawRaf,
            "orf" => RawOrf,
            "mov" => Mov,
            "mp4" => Mp4,
            "m4v" => M4v,
            "3gp" => ThreeGp,
            "avi" => Avi,
            "mkv" => Mkv,
            "webm" => Webm,
            "mts" | "m2ts" => Mts,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    pub mtime_ns: i64,
    pub hash: Option<crate::hash::ContentHash>,
}

pub use crate::geo::Place;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_case_insensitively() {
        assert_eq!(Format::from_extension("JPG"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("heif"), Some(Format::Heic));
        assert_eq!(Format::from_extension("MoV"), Some(Format::Mov));
        assert_eq!(Format::from_extension("txt"), None);
        assert_eq!(Format::from_extension(""), None);
    }

    #[test]
    fn kinds() {
        assert_eq!(Format::RawCr3.kind(), MediaKind::Image);
        assert_eq!(Format::Mts.kind(), MediaKind::Video);
    }

    #[test]
    fn as_str_matches_serde_names() {
        for f in [Format::Jpeg, Format::RawCr3, Format::ThreeGp, Format::Mts] {
            assert_eq!(
                serde_json::to_string(&f).unwrap(),
                format!("\"{}\"", f.as_str())
            );
        }
    }

    #[test]
    fn format_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&Format::ThreeGp).unwrap(),
            "\"three-gp\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKind::Image).unwrap(),
            "\"image\""
        );
    }
}
