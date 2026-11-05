//! Library statistics, computed from the index. Hard links are counted once.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::Result;

#[derive(Debug, Clone, Default, Serialize)]
pub struct KindStats {
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatStat {
    pub format: String,
    pub kind: String,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastScan {
    pub id: i64,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub files_seen: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub root: PathBuf,
    pub last_scan: Option<LastScan>,
    pub photos: KindStats,
    pub videos: KindStats,
    pub cloud_only: u64,
    pub errors: u64,
    /// Sorted by count, most used first.
    pub formats: Vec<FormatStat>,
}

/// One row per physical file: paths sharing `(dev, ino)` collapse.
const UNIQUE: &str =
    "(SELECT media_kind, format, size_bytes, is_cloud_only FROM file GROUP BY device_id, inode)";

pub fn compute(conn: &Connection, root: &Path) -> Result<Stats> {
    let kind = |k: &str| -> Result<KindStats> {
        Ok(conn.query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(size_bytes),0) FROM {UNIQUE} WHERE media_kind=?1"
            ),
            [k],
            |r| {
                Ok(KindStats {
                    count: r.get::<_, i64>(0)? as u64,
                    bytes: r.get::<_, i64>(1)? as u64,
                })
            },
        )?)
    };
    let cloud_only = conn.query_row(
        &format!("SELECT COUNT(*) FROM {UNIQUE} WHERE is_cloud_only=1"),
        [],
        |r| r.get::<_, i64>(0),
    )? as u64;

    let mut stmt = conn.prepare(&format!(
        "SELECT format, media_kind, COUNT(*) AS n, COALESCE(SUM(size_bytes),0) FROM {UNIQUE} \
         GROUP BY media_kind, format ORDER BY n DESC, format"
    ))?;
    let formats = stmt
        .query_map([], |r| {
            Ok(FormatStat {
                format: r
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "unknown".into()),
                kind: r.get(1)?,
                count: r.get::<_, i64>(2)? as u64,
                bytes: r.get::<_, i64>(3)? as u64,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let last_scan = conn
        .query_row(
            "SELECT id,status,started_at,finished_at,COALESCE(files_seen,0),COALESCE(error_count,0) \
             FROM scan ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok(LastScan {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    started_at: r.get(2)?,
                    finished_at: r.get(3)?,
                    files_seen: r.get::<_, i64>(4)? as u64,
                    errors: r.get::<_, i64>(5)? as u64,
                })
            },
        )
        .ok();

    Ok(Stats {
        root: root.to_path_buf(),
        errors: last_scan.as_ref().map_or(0, |s| s.errors),
        last_scan,
        photos: kind("image")?,
        videos: kind("video")?,
        cloud_only,
        formats,
    })
}
