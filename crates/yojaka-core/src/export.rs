//! CSV and JSON export of the index. GPS coordinates are left out unless asked for (FR-48);
//! place names are always included.

use std::io::Write;

use rusqlite::Connection;
use serde_json::{Map, Value, json};

use crate::meta::format_local;
use crate::{Result, YojakaError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExportOptions {
    pub include_gps: bool,
}

const COLUMNS: &[&str] = &[
    "path",
    "kind",
    "format",
    "size_bytes",
    "width",
    "height",
    "taken_at",
    "taken_at_src",
    "camera_make",
    "camera_model",
    "lens",
    "orientation",
    "duration_ms",
    "fps",
    "rotation",
    "video_codec",
    "audio_codec",
    "container",
    "frames",
    "live_photo_pair",
    "country_code",
    "country",
    "state",
    "city",
    "place_dist_km",
    "location_source",
    "metadata_status",
];
const GPS_COLUMNS: &[&str] = &["gps_lat", "gps_lon"];

const SQL: &str = "SELECT f.full_path, f.media_kind, f.format, f.size_bytes, m.width, m.height, m.taken_at, \
m.taken_at_src, m.camera_make, m.camera_model, m.lens, m.orientation, m.duration_ms, m.fps, m.rotation, \
m.video_codec, m.audio_codec, m.container, m.frames, m.pair_id, m.place_country_code, m.place_country, \
m.place_state, m.place_city, m.place_dist_km, m.loc_src, f.metadata_status, m.gps_lat, m.gps_lon \
FROM file f LEFT JOIN meta m ON m.file_id = f.id ORDER BY f.full_path";

/// A spreadsheet treats a cell starting with one of these as a formula. File names and EXIF
/// strings are untrusted, so such text cells are prefixed with `'`.
fn csv_cell(v: &Value) -> String {
    let (text, is_text) = match v {
        Value::Null => return String::new(),
        Value::String(s) => (s.clone(), true),
        other => (other.to_string(), false),
    };
    let text = if is_text && text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn rows(conn: &Connection, opts: ExportOptions) -> Result<Vec<Vec<(&'static str, Value)>>> {
    use std::os::unix::ffi::OsStrExt;
    let mut stmt = conn.prepare(SQL)?;
    let mut out = Vec::new();
    let mut q = stmt.query([])?;
    while let Some(r) = q.next()? {
        let path: Vec<u8> = r.get(0)?;
        let opt_s = |i: usize| -> rusqlite::Result<Value> {
            Ok(r.get::<_, Option<String>>(i)?
                .map_or(Value::Null, Value::String))
        };
        let opt_i = |i: usize| -> rusqlite::Result<Value> {
            Ok(r.get::<_, Option<i64>>(i)?
                .map_or(Value::Null, |v| json!(v)))
        };
        let opt_f = |i: usize| -> rusqlite::Result<Value> {
            Ok(r.get::<_, Option<f64>>(i)?
                .map_or(Value::Null, |v| json!(v)))
        };
        let taken = r
            .get::<_, Option<i64>>(6)?
            .map_or(Value::Null, |t| Value::String(format_local(t)));
        let mut row = vec![
            (
                "path",
                Value::String(
                    String::from_utf8_lossy(std::ffi::OsStr::from_bytes(&path).as_bytes())
                        .into_owned(),
                ),
            ),
            ("kind", opt_s(1)?),
            ("format", opt_s(2)?),
            ("size_bytes", opt_i(3)?),
            ("width", opt_i(4)?),
            ("height", opt_i(5)?),
            ("taken_at", taken),
            ("taken_at_src", opt_s(7)?),
            ("camera_make", opt_s(8)?),
            ("camera_model", opt_s(9)?),
            ("lens", opt_s(10)?),
            ("orientation", opt_i(11)?),
            ("duration_ms", opt_i(12)?),
            ("fps", opt_f(13)?),
            ("rotation", opt_i(14)?),
            ("video_codec", opt_s(15)?),
            ("audio_codec", opt_s(16)?),
            ("container", opt_s(17)?),
            ("frames", opt_i(18)?),
            ("live_photo_pair", opt_i(19)?),
            ("country_code", opt_s(20)?),
            ("country", opt_s(21)?),
            ("state", opt_s(22)?),
            ("city", opt_s(23)?),
            ("place_dist_km", opt_f(24)?),
            ("location_source", opt_s(25)?),
            ("metadata_status", opt_s(26)?),
        ];
        if opts.include_gps {
            row.push(("gps_lat", opt_f(27)?));
            row.push(("gps_lon", opt_f(28)?));
        }
        out.push(row);
    }
    Ok(out)
}

/// Writes every indexed file with its metadata. Returns the number of rows.
pub fn export(
    conn: &Connection,
    format: ExportFormat,
    opts: ExportOptions,
    out: &mut dyn Write,
) -> Result<u64> {
    let io = |e: std::io::Error| YojakaError::Internal(format!("write failed: {e}"));
    let rows = rows(conn, opts)?;
    match format {
        ExportFormat::Csv => {
            let mut header: Vec<&str> = COLUMNS.to_vec();
            if opts.include_gps {
                header.extend(GPS_COLUMNS);
            }
            writeln!(out, "{}", header.join(",")).map_err(io)?;
            for row in &rows {
                let cells: Vec<String> = row.iter().map(|(_, v)| csv_cell(v)).collect();
                writeln!(out, "{}", cells.join(",")).map_err(io)?;
            }
        }
        ExportFormat::Json => {
            writeln!(out, "[").map_err(io)?;
            for (i, row) in rows.iter().enumerate() {
                let obj: Map<String, Value> = row
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect();
                let sep = if i + 1 < rows.len() { "," } else { "" };
                writeln!(out, "  {}{sep}", Value::Object(obj)).map_err(io)?;
            }
            writeln!(out, "]").map_err(io)?;
        }
    }
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_are_quoted_and_defused() {
        assert_eq!(csv_cell(&json!("a,b")), "\"a,b\"");
        assert_eq!(csv_cell(&json!("say \"hi\"")), "\"say \"\"hi\"\"\"");
        assert_eq!(
            csv_cell(&json!("=HYPERLINK(\"x\")")),
            "\"'=HYPERLINK(\"\"x\"\")\""
        );
        assert_eq!(csv_cell(&json!("@evil")), "'@evil");
        assert_eq!(csv_cell(&json!(-5)), "-5"); // numbers are not text
        assert_eq!(csv_cell(&Value::Null), "");
    }
}
