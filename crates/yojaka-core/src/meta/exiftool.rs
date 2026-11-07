//! Optional `exiftool` fallback for containers the built-in extractors do not read (AVI, MTS)
//! and, with `--exiftool-fallback`, for files they failed on. Paths are passed after `--`, so a
//! file name can never be mistaken for an option, and nothing is run through a shell.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::time::parse_exif_datetime;
use super::{DateSrc, Meta, MetaError};
use crate::geo::GeoPoint;

const FIELDS: &[&str] = &[
    "-ImageWidth",
    "-ImageHeight",
    "-Duration",
    "-VideoFrameRate",
    "-DateTimeOriginal",
    "-CreateDate",
    "-Make",
    "-Model",
    "-GPSLatitude",
    "-GPSLongitude",
    "-VideoCodec",
    "-Compression",
    "-AudioCodec",
    "-AudioFormat",
];

/// Whether an `exiftool` binary can be run.
pub fn available() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Runs one `exiftool -json` call for all `paths`; the result has an entry per path.
pub fn extract_batch(paths: &[PathBuf]) -> Vec<(PathBuf, Result<Meta, MetaError>)> {
    let fail = |msg: String| {
        paths
            .iter()
            .map(|p| (p.clone(), Err(MetaError::Malformed(msg.clone()))))
            .collect()
    };
    let out = match Command::new("exiftool")
        .args(["-json", "-n", "-fast2"])
        .args(FIELDS)
        .arg("--")
        .args(paths)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(e) => return fail(format!("exiftool could not be run: {e}")),
    };
    let rows: Vec<serde_json::Value> = match serde_json::from_slice(&out.stdout) {
        Ok(r) => r,
        Err(e) => return fail(format!("exiftool output was not JSON: {e}")),
    };
    let mut by_path: HashMap<&str, &serde_json::Value> = HashMap::new();
    for row in &rows {
        if let Some(p) = row.get("SourceFile").and_then(|v| v.as_str()) {
            by_path.insert(p, row);
        }
    }
    paths
        .iter()
        .map(|p| {
            let key = p.to_string_lossy();
            let r = by_path
                .get(key.as_ref())
                .map(|row| parse_row(row))
                .unwrap_or_else(|| Err(MetaError::Malformed("exiftool returned nothing".into())));
            (p.clone(), r)
        })
        .collect()
}

pub fn extract_one(path: &Path) -> Result<Meta, MetaError> {
    extract_batch(&[path.to_path_buf()])
        .into_iter()
        .next()
        .map_or(Err(MetaError::Unsupported), |(_, r)| r)
}

fn text(row: &serde_json::Value, k: &str) -> Option<String> {
    let v = row.get(k)?;
    let s = v.as_str().map_or_else(|| v.to_string(), str::to_string);
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn parse_row(row: &serde_json::Value) -> Result<Meta, MetaError> {
    if let Some(err) = row.get("Error").and_then(|v| v.as_str()) {
        return Err(MetaError::Malformed(format!("exiftool: {err}")));
    }
    let num = |k: &str| row.get(k).and_then(serde_json::Value::as_f64);
    let mut m = Meta {
        width: num("ImageWidth").map(|v| v as u32),
        height: num("ImageHeight").map(|v| v as u32),
        duration_ms: num("Duration")
            .filter(|d| *d >= 0.0)
            .map(|d| (d * 1000.0).round() as u64),
        fps: num("VideoFrameRate").filter(|f| *f > 0.0),
        camera_make: text(row, "Make"),
        camera_model: text(row, "Model"),
        video_codec: text(row, "VideoCodec")
            .or_else(|| text(row, "Compression"))
            .map(|s| s.to_ascii_lowercase()),
        audio_codec: text(row, "AudioCodec")
            .or_else(|| text(row, "AudioFormat"))
            .map(|s| s.to_ascii_lowercase()),
        ..Meta::default()
    };
    for k in ["DateTimeOriginal", "CreateDate"] {
        if let Some(t) = text(row, k).and_then(|s| parse_exif_datetime(&s)) {
            m.taken_at = Some(t);
            m.taken_at_src = DateSrc::Container;
            break;
        }
    }
    if let (Some(la), Some(lo)) = (num("GPSLatitude"), num("GPSLongitude")) {
        m.gps = GeoPoint::checked(la, lo);
    }
    if m.width.is_none() && m.duration_ms.is_none() && m.taken_at.is_none() {
        return Err(MetaError::Malformed(
            "exiftool found no usable metadata".into(),
        ));
    }
    Ok(m)
}
