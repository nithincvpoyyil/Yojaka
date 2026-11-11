//! The metadata pass: reads `pending` files from the index, extracts in parallel, writes the
//! results through one writer thread, then pairs Live Photos and resolves places.
//!
//! Cloud-only (dataless) files are never opened. Files are opened with `O_NOFOLLOW`.
//! Per-file failures are collected (`metadata_status='error'` + a `scan_error` row), never fatal.

use std::io::BufReader;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rayon::prelude::*;
use rusqlite::{Connection, params};

use super::{Meta, MetaError, exiftool, extract, extractor_for};
use crate::geo::{EmbeddedGeocoder, GeoPoint, Geocoder};
use crate::index::Index;
use crate::model::Format;
use crate::{
    CancelToken, Config, CoreEvent, EventSink, FileIssue, IssueKind, Result, TaskKind, TaskOutcome,
    YojakaError, livephoto,
};

const BATCH: usize = 256;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Default)]
pub struct MetaOptions {
    /// Also run `exiftool` on files the built-in extractors failed on. (AVI and MTS always use
    /// it when it is installed, because no built-in extractor reads them.)
    pub exiftool_fallback: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MetaSummary {
    pub extracted: u64,
    pub errors: u64,
    pub unsupported: u64,
    pub cloud_only: u64,
    pub live_pairs: u64,
    pub geocoded: u64,
    pub elapsed_ms: u64,
}

/// Opens `path` without following symlinks and extracts with the right extractor. A raw photo
/// is read by macOS ImageIO first ([`super::raw`]), then by the EXIF reader if that fails.
pub fn extract_path(path: &Path, format: Format, size: u64) -> Result<Meta, MetaError> {
    let file = crate::sys::macos::open_nofollow(path)?;
    if format.is_raw() {
        match super::raw::extract(&file, format) {
            Ok(m) => return Ok(m),
            Err(e) => tracing::debug!(
                path = %path.display(),
                "metadata: ImageIO could not read the raw file ({e}); trying the EXIF reader"
            ),
        }
    }
    let info = super::FileInfo { size, format };
    // The EXIF reader seeks to the start first, wherever ImageIO left the shared offset.
    extract(&mut BufReader::with_capacity(16 << 10, file), &info)
}

/// Raw photos read before ImageIO read their metadata (a Nikon NEF got its thumbnail's size;
/// Fuji, Canon CR3 and Olympus files got nothing) are read again once, on the next scan. No
/// scan from scratch is needed.
fn refresh_raw(conn: &mut Connection) -> rusqlite::Result<u64> {
    let version = super::raw::RAW_META_VERSION;
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta_kv WHERE key='raw_meta_version'",
            [],
            |r| r.get(0),
        )
        .ok();
    if stored.as_deref() == Some(version) {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let again = tx.execute(
        "UPDATE file SET metadata_status='pending' WHERE format LIKE 'raw-%' \
         AND is_cloud_only=0 AND metadata_status IN ('ok','error','unsupported')",
        [],
    )?;
    tx.execute(
        "INSERT INTO meta_kv(key,value) VALUES('raw_meta_version',?1) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [version],
    )?;
    tx.commit()?;
    Ok(again as u64)
}

enum Outcome {
    Ok(Box<Meta>),
    Err(MetaError),
}

struct Job {
    id: i64,
    path: PathBuf,
    format: Format,
    size: u64,
}

const INSERT: &str = "INSERT OR REPLACE INTO meta(file_id,width,height,taken_at,taken_at_src,\
camera_make,camera_model,lens,orientation,has_gps,gps_lat,gps_lon,frames,duration_ms,fps,\
rotation,video_codec,audio_codec,bitrate,container,content_id) \
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)";

fn store(tx: &rusqlite::Transaction, job: &Job, m: &Meta) -> rusqlite::Result<()> {
    let bitrate = m
        .duration_ms
        .filter(|d| *d > 0)
        .map(|d| (job.size.saturating_mul(8_000) / d) as i64);
    tx.prepare_cached(INSERT)?.execute(params![
        job.id,
        m.width,
        m.height,
        m.taken_at,
        m.taken_at_src.as_str(),
        m.camera_make,
        m.camera_model,
        m.lens,
        m.orientation,
        m.gps.is_some(),
        m.gps.map(|g| g.lat),
        m.gps.map(|g| g.lon),
        m.frames,
        m.duration_ms.map(|d| d as i64),
        m.fps,
        m.rotation,
        m.video_codec,
        m.audio_codec,
        bitrate,
        m.container,
        m.content_id,
    ])?;
    tx.prepare_cached("UPDATE file SET metadata_status='ok' WHERE id=?1")?
        .execute([job.id])?;
    Ok(())
}

fn mark_failed(
    tx: &rusqlite::Transaction,
    job: &Job,
    err: &MetaError,
    scan_id: Option<i64>,
) -> rusqlite::Result<&'static str> {
    let status = if matches!(err, MetaError::Unsupported) {
        "unsupported"
    } else {
        "error"
    };
    tx.execute("DELETE FROM meta WHERE file_id=?1", [job.id])?;
    tx.execute(
        "UPDATE file SET metadata_status=?2 WHERE id=?1",
        params![job.id, status],
    )?;
    if status == "error" {
        tx.execute(
            "INSERT INTO scan_error(scan_id,file_path,error_kind,error_message) VALUES(?1,?2,'BadMetadata',?3)",
            params![scan_id, job.path.as_os_str().as_bytes(), err.to_string()],
        )?;
    }
    Ok(status)
}

/// Runs the whole pass over `index_path`. Safe to run repeatedly: only `pending` files are read.
pub fn run(
    config: &Config,
    index_path: &Path,
    opts: MetaOptions,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<MetaSummary> {
    let started = Instant::now();
    let mut conn = Index::open(index_path)?.into_connection();
    let raw_again = refresh_raw(&mut conn)?;
    if raw_again > 0 {
        tracing::info!(
            files = raw_again,
            "metadata: raw photos are read again (ImageIO)"
        );
    }
    let cloud_only = conn.execute(
        "UPDATE file SET metadata_status='cloud' WHERE metadata_status='pending' AND is_cloud_only=1",
        [],
    )? as u64;
    let mut summary = MetaSummary {
        cloud_only,
        ..MetaSummary::default()
    };
    let scan_id: Option<i64> = conn.query_row("SELECT MAX(id) FROM scan", [], |r| r.get(0))?;

    let jobs: Vec<Job> = {
        let mut stmt = conn.prepare(
            "SELECT id, full_path, format, size_bytes FROM file WHERE metadata_status='pending' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            let (id, path, fmt, size) = row?;
            let format = serde_json::from_value::<Format>(serde_json::Value::String(fmt));
            if let Ok(format) = format {
                jobs.push(Job {
                    id,
                    path: PathBuf::from(std::ffi::OsString::from_vec(path)),
                    format,
                    size: size.max(0) as u64,
                });
            }
        }
        jobs
    };

    let total = jobs.len() as u64;
    if total > 0 {
        sink.important(CoreEvent::TaskStarted {
            task: TaskKind::Metadata,
            label: format!("Reading metadata of {total} files"),
        });
    }
    let have_exiftool = jobs
        .iter()
        .any(|j| needs_exiftool(j.format) || opts.exiftool_fallback)
        && exiftool::available();

    let threads = if config.threads == 0 {
        std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(2).max(1))
    } else {
        config.threads
    };
    let pool = crate::pool::build("meta-worker", threads)?;
    tracing::info!(
        files = total,
        threads,
        exiftool = have_exiftool,
        "metadata: starting"
    );

    let done = AtomicU64::new(0);
    let mut last_emit = Instant::now();
    let mut retry_with_exiftool: Vec<Job> = Vec::new();
    let mut cancelled = false;

    for chunk in jobs.chunks(BATCH) {
        if cancel.is_cancelled() {
            cancelled = true;
            break;
        }
        let results: Vec<Outcome> = pool.install(|| {
            chunk
                .par_iter()
                .map(|j| {
                    if cancel.is_cancelled() {
                        return Outcome::Err(MetaError::Malformed("cancelled".into()));
                    }
                    if extractor_for(j.format).is_none() {
                        return Outcome::Err(MetaError::Unsupported);
                    }
                    match extract_path(&j.path, j.format, j.size) {
                        Ok(m) => Outcome::Ok(Box::new(m)),
                        Err(e) => Outcome::Err(e),
                    }
                })
                .collect()
        });
        if cancel.is_cancelled() {
            cancelled = true; // the batch may hold placeholder results; leave it all pending
            break;
        }
        let tx = conn.transaction()?;
        for (job, outcome) in chunk.iter().zip(results) {
            match outcome {
                Outcome::Ok(m) => {
                    store(&tx, job, &m)?;
                    summary.extracted += 1;
                }
                Outcome::Err(e) => {
                    let wants_exiftool = have_exiftool
                        && (needs_exiftool(job.format)
                            || (opts.exiftool_fallback && !matches!(e, MetaError::Unsupported)));
                    if wants_exiftool {
                        retry_with_exiftool.push(Job {
                            id: job.id,
                            path: job.path.clone(),
                            format: job.format,
                            size: job.size,
                        });
                        continue;
                    }
                    finish_failure(&tx, job, &e, scan_id, sink, &mut summary)?;
                }
            }
        }
        tx.commit()?;
        done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        if last_emit.elapsed() >= PROGRESS_EVERY {
            last_emit = Instant::now();
            sink.progress(CoreEvent::Progress {
                task: TaskKind::Metadata,
                done: done.load(Ordering::Relaxed),
                total: Some(total),
                bytes: 0,
                current: chunk
                    .last()
                    .and_then(|j| j.path.parent().map(Path::to_path_buf)),
            });
        }
    }

    if !cancelled && total > 0 {
        // Every file has been read. Exiftool retries, Live Photo pairing and places follow
        // without countable progress; the UI shows "finishing up" from here.
        sink.important(CoreEvent::Progress {
            task: TaskKind::Metadata,
            done: done.load(Ordering::Relaxed),
            total: Some(total),
            bytes: 0,
            current: None,
        });
    }

    if !cancelled && !retry_with_exiftool.is_empty() {
        for batch in retry_with_exiftool.chunks(100) {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }
            let paths: Vec<PathBuf> = batch.iter().map(|j| j.path.clone()).collect();
            tracing::debug!(files = paths.len(), "metadata: exiftool retry batch");
            let results = exiftool::extract_batch(&paths);
            let tx = conn.transaction()?;
            for (job, (_, r)) in batch.iter().zip(results) {
                match r {
                    Ok(m) => {
                        store(&tx, job, &m)?;
                        summary.extracted += 1;
                    }
                    Err(e) => finish_failure(&tx, job, &e, scan_id, sink, &mut summary)?,
                }
            }
            tx.commit()?;
        }
    }

    if !cancelled {
        let t = Instant::now();
        let stats = livephoto::pair(&mut conn)?;
        summary.live_pairs = stats.total();
        tracing::debug!(
            pairs = summary.live_pairs,
            ms = t.elapsed().as_millis() as u64,
            "metadata: live photos paired"
        );
        let t = Instant::now();
        summary.geocoded = geocode(&mut conn, &EmbeddedGeocoder, sink, cancel)?;
        tracing::debug!(
            places = summary.geocoded,
            ms = t.elapsed().as_millis() as u64,
            "metadata: places resolved"
        );
    }
    if total > 0 {
        sink.important(CoreEvent::TaskFinished {
            task: TaskKind::Metadata,
            outcome: if cancelled {
                TaskOutcome::Cancelled
            } else {
                TaskOutcome::Complete
            },
        });
    }
    summary.elapsed_ms = started.elapsed().as_millis() as u64;
    tracing::info!(?summary, cancelled, "metadata: finished");
    Ok(summary)
}

fn needs_exiftool(f: Format) -> bool {
    matches!(f, Format::Avi | Format::Mts)
}

fn finish_failure(
    tx: &rusqlite::Transaction,
    job: &Job,
    err: &MetaError,
    scan_id: Option<i64>,
    sink: &EventSink,
    summary: &mut MetaSummary,
) -> Result<()> {
    match mark_failed(tx, job, err, scan_id)? {
        "unsupported" => summary.unsupported += 1,
        _ => {
            summary.errors += 1;
            tracing::warn!(path = %job.path.display(), "metadata: {err}");
            sink.important(CoreEvent::Issue(FileIssue {
                path: job.path.clone(),
                kind: IssueKind::BadMetadata,
                message: err.to_string(),
            }));
        }
    }
    Ok(())
}

/// Fills `meta.place_*` for rows with GPS that have no place yet, then lets Live Photo partners
/// share one place (FR-47). Places are recomputed from scratch when the embedded data changes.
pub(crate) fn geocode(
    conn: &mut Connection,
    geocoder: &dyn Geocoder,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<u64> {
    let version = crate::geo::EmbeddedGeocoder::DATA_VERSION.to_string();
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta_kv WHERE key='geodata_version'",
            [],
            |r| r.get(0),
        )
        .ok();
    let tx = conn.transaction()?;
    if stored.as_deref() != Some(version.as_str()) {
        tx.execute(
            "UPDATE meta SET place_country_code=NULL,place_country=NULL,place_state=NULL,\
             place_district=NULL,place_city=NULL,place_dist_km=NULL,loc_src=NULL",
            [],
        )?;
        tx.execute(
            "INSERT INTO meta_kv(key,value) VALUES('geodata_version',?1) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [&version],
        )?;
    }
    // Borrowed places are recomputed every run so they can never go stale.
    tx.execute(
        "UPDATE meta SET place_country_code=NULL,place_country=NULL,place_state=NULL,\
         place_district=NULL,place_city=NULL,place_dist_km=NULL,loc_src=NULL WHERE loc_src='pair'",
        [],
    )?;
    let todo: Vec<(i64, f64, f64)> = {
        let mut stmt = tx.prepare(
            "SELECT file_id,gps_lat,gps_lon FROM meta WHERE has_gps=1 AND place_dist_km IS NULL",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    let mut n = 0u64;
    let todo_len = todo.len() as u64;
    let mut last_emit = Instant::now();
    if !todo.is_empty() {
        sink.important(CoreEvent::TaskStarted {
            task: TaskKind::Geocode,
            label: format!("Resolving {} places", todo.len()),
        });
        let mut set = tx.prepare(
            "UPDATE meta SET place_country_code=?2,place_country=?3,place_state=?4,place_district=?5,\
             place_city=?6,place_dist_km=?7,loc_src='gps' WHERE file_id=?1",
        )?;
        for (i, (id, lat, lon)) in todo.into_iter().enumerate() {
            if cancel.is_cancelled() {
                break;
            }
            if last_emit.elapsed() >= PROGRESS_EVERY {
                last_emit = Instant::now();
                sink.progress(CoreEvent::Progress {
                    task: TaskKind::Geocode,
                    done: i as u64,
                    total: Some(todo_len),
                    bytes: 0,
                    current: None,
                });
            }
            let Some(p) = GeoPoint::checked(lat, lon).and_then(|g| geocoder.lookup(g)) else {
                continue;
            };
            set.execute(params![
                id,
                p.country_code,
                p.country,
                p.admin1,
                p.admin2,
                p.city,
                p.dist_km
            ])?;
            n += 1;
        }
        if !cancel.is_cancelled() {
            sink.important(CoreEvent::Progress {
                task: TaskKind::Geocode,
                done: todo_len,
                total: Some(todo_len),
                bytes: 0,
                current: None,
            });
        }
        sink.important(CoreEvent::TaskFinished {
            task: TaskKind::Geocode,
            outcome: TaskOutcome::Complete,
        });
    }
    // A Live Photo pair shares one place: the photo's, or the video's if the photo has none.
    for (from, to) in [("img", "vid"), ("vid", "img")] {
        tx.execute(
            &format!(
                "UPDATE meta SET place_country_code=p.place_country_code,place_country=p.place_country,\
                 place_state=p.place_state,place_district=p.place_district,place_city=p.place_city,\
                 place_dist_km=p.place_dist_km,loc_src='pair' \
                 FROM (SELECT m.pair_id AS pid, m.* FROM meta m JOIN file f ON f.id=m.file_id \
                       WHERE m.pair_id IS NOT NULL AND m.place_dist_km IS NOT NULL AND \
                       (f.media_kind = '{}')) AS p \
                 WHERE meta.pair_id = p.pid AND meta.file_id != p.file_id AND meta.place_dist_km IS NULL \
                 AND meta.file_id IN (SELECT id FROM file WHERE media_kind = '{}')",
                if from == "img" { "image" } else { "video" },
                if to == "img" { "image" } else { "video" },
            ),
            [],
        )?;
    }
    tx.commit()?;
    Ok(n)
}

/// What `yojaka info FILE` shows.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileReport {
    pub path: PathBuf,
    pub format: Format,
    pub size_bytes: u64,
    pub taken_at: Option<String>,
    pub taken_at_src: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub orientation: Option<u16>,
    pub frames: Option<u32>,
    pub duration_ms: Option<u64>,
    pub fps: Option<f64>,
    pub rotation: Option<u16>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub container: Option<String>,
    pub content_id: Option<String>,
    /// Raw coordinates: shown here (the detail view) but never in stats or default exports.
    pub gps: Option<GeoPoint>,
    pub place: Option<crate::geo::Place>,
}

/// Reads one file directly. Cloud-only files are refused rather than downloaded.
pub fn report(file: &Path, config: &Config) -> Result<FileReport> {
    use std::os::macos::fs::MetadataExt;
    let md = std::fs::symlink_metadata(file).map_err(|e| YojakaError::io(file, e))?;
    if !md.is_file() {
        return Err(YojakaError::Internal(format!(
            "{} is not a regular file",
            file.display()
        )));
    }
    if crate::sys::macos::is_dataless(&md) {
        return Err(YojakaError::Internal(format!(
            "{} is stored in iCloud and not downloaded; Yojaka does not open such files",
            file.display()
        )));
    }
    let format = crate::detect::detect(file, false, true)
        .map_err(|e| YojakaError::io(file, e))?
        .ok_or_else(|| {
            YojakaError::Internal(format!("{} is not a supported media file", file.display()))
        })?;
    let size = md.st_size();
    let meta = match extract_path(file, format, size) {
        Ok(m) => m,
        Err(MetaError::Unsupported) if exiftool::available() => exiftool::extract_one(file)
            .map_err(|e| YojakaError::Internal(format!("{}: {e}", file.display())))?,
        Err(e) if config.scan.exiftool_fallback && exiftool::available() => {
            exiftool::extract_one(file)
                .map_err(|_| YojakaError::Internal(format!("{}: {e}", file.display())))?
        }
        Err(e) => return Err(YojakaError::Internal(format!("{}: {e}", file.display()))),
    };
    let place = meta.gps.and_then(|g| EmbeddedGeocoder.lookup(g));
    Ok(FileReport {
        path: file.to_path_buf(),
        format,
        size_bytes: size,
        taken_at: meta.taken_at.map(super::time::format_local),
        taken_at_src: meta.taken_at_src.as_str(),
        width: meta.width,
        height: meta.height,
        camera_make: meta.camera_make,
        camera_model: meta.camera_model,
        lens: meta.lens,
        orientation: meta.orientation,
        frames: meta.frames,
        duration_ms: meta.duration_ms,
        fps: meta.fps,
        rotation: meta.rotation,
        video_codec: meta.video_codec,
        audio_codec: meta.audio_codec,
        container: meta.container,
        content_id: meta.content_id,
        gps: meta.gps,
        place,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::geo::Place;

    struct Fixed;
    impl Geocoder for Fixed {
        fn lookup(&self, p: GeoPoint) -> Option<Place> {
            Some(Place {
                city: format!("City{}", p.lat as i32),
                admin1: None,
                admin2: None,
                country_code: "XX".into(),
                country: "Xland".into(),
                dist_km: 1.0,
            })
        }
    }

    fn add(c: &Connection, kind: &str, gps: Option<(f64, f64)>, pair: Option<i64>) -> i64 {
        c.execute(
            "INSERT INTO file(full_path,parent_folder,file_name,size_bytes,modified_at_ns,device_id,inode,\
             media_kind,format,last_seen_scan_id) VALUES(randomblob(8),x'00',x'00',1,0,1,abs(random()),?1,'x',1)",
            [kind],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO meta(file_id,taken_at_src,has_gps,gps_lat,gps_lon,pair_id) VALUES(?1,'none',?2,?3,?4,?5)",
            params![id, gps.is_some(), gps.map(|g| g.0), gps.map(|g| g.1), pair],
        )
        .unwrap();
        id
    }

    fn city(c: &Connection, id: i64) -> (Option<String>, Option<String>) {
        c.query_row(
            "SELECT place_city, loc_src FROM meta WHERE file_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn live_photo_halves_share_one_place() {
        let mut idx = Index::open_in_memory().unwrap();
        let conn = idx.connection_mut_for_tests();
        conn.execute(
            "INSERT INTO scan(root_path,started_at,status) VALUES(x'2f',0,'complete')",
            [],
        )
        .unwrap();
        // photo has GPS, video has none; and the reverse; and an unpaired file without GPS
        let (img1, vid1) = (
            add(conn, "image", Some((10.0, 20.0)), Some(1)),
            add(conn, "video", None, Some(1)),
        );
        let (img2, vid2) = (
            add(conn, "image", None, Some(2)),
            add(conn, "video", Some((30.0, 40.0)), Some(2)),
        );
        let lonely = add(conn, "image", None, None);

        let n = geocode(conn, &Fixed, &EventSink::null(), &CancelToken::new()).unwrap();
        assert_eq!(n, 2, "only files with GPS are looked up");
        assert_eq!(
            city(conn, img1),
            (Some("City10".into()), Some("gps".into()))
        );
        assert_eq!(
            city(conn, vid1),
            (Some("City10".into()), Some("pair".into()))
        );
        assert_eq!(
            city(conn, vid2),
            (Some("City30".into()), Some("gps".into()))
        );
        assert_eq!(
            city(conn, img2),
            (Some("City30".into()), Some("pair".into()))
        );
        assert_eq!(city(conn, lonely), (None, None));
        // the borrowed half still has no GPS of its own
        let has: bool = conn
            .query_row("SELECT has_gps FROM meta WHERE file_id=?1", [vid1], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!has);

        // Unpairing removes the borrowed place on the next run.
        conn.execute("UPDATE meta SET pair_id=NULL", []).unwrap();
        geocode(conn, &Fixed, &EventSink::null(), &CancelToken::new()).unwrap();
        assert_eq!(city(conn, vid1), (None, None));
    }
}
