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
pub struct VideoItem {
    pub path: PathBuf,
    pub duration_ms: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileItem {
    pub path: PathBuf,
    pub kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodecStat {
    pub codec: String,
    pub count: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct VideoStats {
    /// Sum of durations of all videos whose duration is known.
    pub total_duration_ms: u64,
    pub with_duration: u64,
    pub longest: Vec<VideoItem>,
    pub largest: Vec<VideoItem>,
    pub codecs: Vec<CodecStat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct YearStat {
    pub year: i32,
    pub count: u64,
    /// January..December.
    pub months: [u64; 12],
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraStat {
    pub camera: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CountryStat {
    pub country_code: String,
    pub country: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CityStat {
    pub city: String,
    pub state: Option<String>,
    pub country: String,
    pub count: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LocationStats {
    /// Files with GPS coordinates (the coordinates themselves are never part of stats).
    pub with_gps: u64,
    /// Files with a resolved place (own GPS, or shared by a Live Photo partner).
    pub with_place: u64,
    pub without_place: u64,
    pub countries: Vec<CountryStat>,
    pub cities: Vec<CityStat>,
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
    pub video: VideoStats,
    /// Live Photo pairs (each counted once).
    pub live_photo_pairs: u64,
    pub by_year: Vec<YearStat>,
    /// Files without an embedded date.
    pub undated: u64,
    pub cameras: Vec<CameraStat>,
    pub largest_files: Vec<FileItem>,
    pub location: LocationStats,
    /// Files whose metadata has not been read yet.
    pub metadata_pending: u64,
    /// Files whose metadata could not be read (damaged, truncated, unsupported container).
    pub metadata_errors: u64,
}

/// How many entries the "top N" lists hold.
pub const TOP_N: usize = 10;
/// Precision guard for city statistics (see FR-45).
#[derive(Debug, Clone, Copy)]
pub struct StatsOptions {
    pub far_km: f64,
}

impl Default for StatsOptions {
    fn default() -> Self {
        Self { far_km: 100.0 }
    }
}

/// One row per physical file: paths sharing `(dev, ino)` collapse.
const UNIQUE: &str = "(SELECT id, media_kind, format, size_bytes, is_cloud_only, full_path, \
metadata_status FROM file GROUP BY device_id, inode)";

fn path_of(raw: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(raw))
}

/// Location coverage plus the most common countries and cities (at most `limit` of each).
/// Cities farther than `far_km` from their nearest place are left out (precision guard).
pub fn location(conn: &Connection, far_km: f64, limit: usize) -> Result<LocationStats> {
    let q =
        |sql: &str| -> Result<u64> { Ok(conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as u64) };
    let total_unique = q(&format!("SELECT COUNT(*) FROM {UNIQUE}"))?;
    let with_gps = q(&format!(
        "SELECT COUNT(*) FROM {UNIQUE} u JOIN meta m ON m.file_id=u.id WHERE m.has_gps=1"
    ))?;
    let with_place = q(&format!(
        "SELECT COUNT(*) FROM {UNIQUE} u JOIN meta m ON m.file_id=u.id WHERE m.place_dist_km IS NOT NULL"
    ))?;
    let countries = {
        let mut st = conn.prepare(&format!(
            "SELECT m.place_country_code, m.place_country, COUNT(*) AS n FROM {UNIQUE} u \
             JOIN meta m ON m.file_id=u.id WHERE m.place_dist_km IS NOT NULL AND m.place_dist_km <= ?1 \
             GROUP BY 1, 2 ORDER BY n DESC, 2 LIMIT {limit}"
        ))?;
        st.query_map([crate::geo::COUNTRY_MAX_KM], |r| {
            Ok(CountryStat {
                country_code: r.get(0)?,
                country: r.get(1)?,
                count: r.get::<_, i64>(2)? as u64,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    let cities = {
        let mut st = conn.prepare(&format!(
            "SELECT m.place_city, m.place_state, m.place_country, COUNT(*) AS n FROM {UNIQUE} u \
             JOIN meta m ON m.file_id=u.id WHERE m.place_dist_km IS NOT NULL AND m.place_dist_km <= ?1 \
             GROUP BY 1, 2, 3 ORDER BY n DESC, 1 LIMIT {limit}"
        ))?;
        st.query_map([far_km], |r| {
            Ok(CityStat {
                city: r.get(0)?,
                state: r.get(1)?,
                country: r.get(2)?,
                count: r.get::<_, i64>(3)? as u64,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    Ok(LocationStats {
        with_gps,
        with_place,
        without_place: total_unique.saturating_sub(with_place),
        countries,
        cities,
    })
}

pub fn compute(conn: &Connection, root: &Path, opts: StatsOptions) -> Result<Stats> {
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

    let q =
        |sql: &str| -> Result<u64> { Ok(conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as u64) };
    let metadata_pending = q(&format!(
        "SELECT COUNT(*) FROM {UNIQUE} WHERE metadata_status = 'pending'"
    ))?;
    let metadata_errors = q(&format!(
        "SELECT COUNT(*) FROM {UNIQUE} WHERE metadata_status = 'error'"
    ))?;
    let live_photo_pairs = q("SELECT COUNT(DISTINCT pair_id) FROM meta WHERE pair_id IS NOT NULL")?;
    let undated = q(&format!(
        "SELECT COUNT(*) FROM {UNIQUE} u JOIN meta m ON m.file_id=u.id WHERE m.taken_at IS NULL"
    ))?;

    // videos
    let video_items = |order: &str| -> Result<Vec<VideoItem>> {
        let mut st = conn.prepare(&format!(
            "SELECT u.full_path, m.duration_ms, u.size_bytes FROM {UNIQUE} u JOIN meta m ON m.file_id=u.id \
             WHERE u.media_kind='video' AND m.duration_ms IS NOT NULL ORDER BY {order} LIMIT {TOP_N}"
        ))?;
        let rows = st.query_map([], |r| {
            Ok(VideoItem {
                path: path_of(r.get(0)?),
                duration_ms: r.get::<_, i64>(1)? as u64,
                size_bytes: r.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    };
    let (total_ms, with_duration): (i64, i64) = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(m.duration_ms),0), COUNT(m.duration_ms) FROM {UNIQUE} u \
             JOIN meta m ON m.file_id=u.id WHERE u.media_kind='video'"
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let codecs = {
        let mut st = conn.prepare(&format!(
            "SELECT m.video_codec, COUNT(*) AS n FROM {UNIQUE} u JOIN meta m ON m.file_id=u.id \
             WHERE u.media_kind='video' AND m.video_codec IS NOT NULL GROUP BY m.video_codec \
             ORDER BY n DESC, m.video_codec"
        ))?;
        st.query_map([], |r| {
            Ok(CodecStat {
                codec: r.get(0)?,
                count: r.get::<_, i64>(1)? as u64,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    let video = VideoStats {
        total_duration_ms: total_ms as u64,
        with_duration: with_duration as u64,
        longest: video_items("m.duration_ms DESC, u.full_path")?,
        largest: video_items("u.size_bytes DESC, u.full_path")?,
        codecs,
    };

    // dates
    let mut by_year: Vec<YearStat> = Vec::new();
    {
        let mut st = conn.prepare(&format!(
            "SELECT CAST(strftime('%Y', m.taken_at,'unixepoch') AS INTEGER), \
             CAST(strftime('%m', m.taken_at,'unixepoch') AS INTEGER), COUNT(*) \
             FROM {UNIQUE} u JOIN meta m ON m.file_id=u.id WHERE m.taken_at IS NOT NULL \
             GROUP BY 1, 2 ORDER BY 1, 2"
        ))?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, i64>(1)? as usize,
                r.get::<_, i64>(2)? as u64,
            ))
        })?;
        for row in rows {
            let (y, m, n) = row?;
            if by_year.last().is_none_or(|b| b.year != y) {
                by_year.push(YearStat {
                    year: y,
                    count: 0,
                    months: [0; 12],
                });
            }
            if let Some(b) = by_year.last_mut() {
                b.count += n;
                if (1..=12).contains(&m) {
                    b.months[m - 1] += n;
                }
            }
        }
    }

    // cameras: "Make Model", without repeating the make when the model already starts with it
    let cameras = {
        let mut st = conn.prepare(&format!(
            "SELECT m.camera_make, m.camera_model, COUNT(*) AS n FROM {UNIQUE} u \
             JOIN meta m ON m.file_id=u.id WHERE m.camera_make IS NOT NULL OR m.camera_model IS NOT NULL \
             GROUP BY m.camera_make, m.camera_model"
        ))?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)? as u64,
            ))
        })?;
        let mut merged: std::collections::BTreeMap<String, u64> = Default::default();
        for row in rows {
            let (make, model, n) = row?;
            let name = match (make, model) {
                (Some(a), Some(b)) if b.to_lowercase().starts_with(&a.to_lowercase()) => b,
                (Some(a), Some(b)) => format!("{a} {b}"),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => continue,
            };
            *merged.entry(name).or_default() += n;
        }
        let mut v: Vec<CameraStat> = merged
            .into_iter()
            .map(|(camera, count)| CameraStat { camera, count })
            .collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.camera.cmp(&b.camera)));
        v.truncate(TOP_N);
        v
    };

    let largest_files = {
        let mut st = conn.prepare(&format!(
            "SELECT full_path, media_kind, size_bytes FROM {UNIQUE} ORDER BY size_bytes DESC, full_path LIMIT {TOP_N}"
        ))?;
        st.query_map([], |r| {
            Ok(FileItem {
                path: path_of(r.get(0)?),
                kind: r.get(1)?,
                size_bytes: r.get::<_, i64>(2)? as u64,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    let location = location(conn, opts.far_km, TOP_N)?;

    Ok(Stats {
        video,
        live_photo_pairs,
        by_year,
        undated,
        cameras,
        largest_files,
        metadata_pending,
        metadata_errors,
        location,
        root: root.to_path_buf(),
        errors: last_scan.as_ref().map_or(0, |s| s.errors),
        last_scan,
        photos: kind("image")?,
        videos: kind("video")?,
        cloud_only,
        formats,
    })
}
