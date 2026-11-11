//! Metadata extraction (Iteration 2).
//!
//! Extractors read from a `Read + Seek` source, never touch pixels, and read a bounded
//! amount: image headers are capped, and video extractors seek box by box so `mdat` is never
//! read. Failures are per-file ([`MetaError`]) and never fatal.

mod image;
mod isobmff;
mod matroska;
mod pass;
mod raw;
mod time;

pub mod exiftool;

use std::io::{Read, Seek};

use crate::geo::GeoPoint;
use crate::model::Format;

pub use pass::{FileReport, MetaOptions, MetaSummary, extract_path, report, run};
pub use time::{format_local, parse_exif_datetime};

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// What the scanner already knows about the file.
#[derive(Debug, Clone, Copy)]
pub struct FileInfo {
    pub size: u64,
    pub format: Format,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Unix seconds of the wall-clock time as recorded (no timezone applied).
    pub taken_at: Option<i64>,
    pub taken_at_src: DateSrc,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub orientation: Option<u16>,
    pub gps: Option<GeoPoint>,
    pub frames: Option<u32>,
    pub duration_ms: Option<u64>,
    pub fps: Option<f64>,
    pub rotation: Option<u16>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub container: Option<String>,
    /// Apple content identifier (Live Photo pairing).
    pub content_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DateSrc {
    Exif,
    QuickTime,
    /// Container creation time (`mvhd`, Matroska `DateUTC`): UTC, often unset.
    Container,
    #[default]
    None,
}

impl DateSrc {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exif => "exif",
            Self::QuickTime => "quicktime",
            Self::Container => "container",
            Self::None => "none",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("unsupported format")]
    Unsupported,
    #[error("malformed metadata: {0}")]
    Malformed(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait Extractor: Send + Sync {
    fn formats(&self) -> &'static [Format];
    fn extract(&self, r: &mut dyn ReadSeek, info: &FileInfo) -> Result<Meta, MetaError>;
}

/// The extractor responsible for `format`, if any. AVI/MTS have none (exiftool fallback only).
pub fn extractor_for(format: Format) -> Option<&'static dyn Extractor> {
    static IMAGE: image::ImageExtractor = image::ImageExtractor;
    static ISO: isobmff::IsoBmff = isobmff::IsoBmff;
    static MKV: matroska::Matroska = matroska::Matroska;
    [&IMAGE as &dyn Extractor, &ISO, &MKV]
        .into_iter()
        .find(|e| e.formats().contains(&format))
}

/// Extracts from `r` with the extractor for `info.format`.
pub fn extract(r: &mut dyn ReadSeek, info: &FileInfo) -> Result<Meta, MetaError> {
    extractor_for(info.format)
        .ok_or(MetaError::Unsupported)?
        .extract(r, info)
}

#[cfg(test)]
mod tests;
