//! Scanning: walk → detect → batch → single DB writer.
//!
//! - Symlinks are never followed; only regular files are considered.
//! - Cloud-only (dataless) files are recorded but never opened.
//! - Rescans are incremental on `(size, mtime, ino)`; vanished files are pruned from the
//!   index (index rows only, never files on disk).

mod writer;

use std::ffi::OsStr;
use std::os::macos::fs::MetadataExt;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::index::Index;
use crate::model::{Format, MediaKind};
use crate::protect::Protected;
use crate::sys::macos::is_dataless;
use crate::{
    CancelToken, Config, CoreEvent, EventSink, FileIssue, IssueKind, Result, TaskKind, TaskOutcome,
    YojakaError, detect,
};
use writer::{FileRecord, Item};

const CHUNK: usize = 512;
const SUB_CHUNK: usize = 64;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
const BUNDLE_SUFFIXES: &[&str] = &[
    ".photoslibrary",
    ".aplibrary",
    ".app",
    ".bundle",
    ".fcpbundle",
];

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub root: PathBuf,
    /// Empty means all kinds.
    pub kinds: Vec<MediaKind>,
    pub sniff_all: bool,
    /// Keep the index in `<root>/.yojaka/index.db`.
    pub local_index: bool,
    /// Delete this folder's index first and rebuild it from scratch.
    pub force: bool,
    /// Read metadata (EXIF, video moov), pair Live Photos and resolve places after the file pass.
    pub metadata: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanSummary {
    pub scan_id: i64,
    pub status: ScanStatus,
    /// Regular files visited (media or not).
    pub files_seen: u64,
    /// Media files in the index after this scan (paths, so hard links count each).
    pub files_indexed: u64,
    pub new_or_changed: u64,
    pub unchanged: u64,
    pub pruned: u64,
    pub cloud_only: u64,
    pub bytes: u64,
    pub issues: u64,
    pub permission_denied: u64,
    pub elapsed_ms: u64,
    /// Present when [`ScanOptions::metadata`] was set.
    pub metadata: Option<crate::meta::MetaSummary>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    pub done: u64,
    pub bytes: u64,
}

struct Ctx<'a> {
    db: &'a Path,
    kinds: &'a [MediaKind],
    sniff_all: bool,
    cancel: &'a CancelToken,
    seen: AtomicU64,
    bytes: AtomicU64,
}

impl Ctx<'_> {
    fn kind_ok(&self, kind: MediaKind) -> bool {
        self.kinds.is_empty() || self.kinds.contains(&kind)
    }
}

fn kind_str(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Image => "image",
        MediaKind::Video => "video",
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn issue(path: &Path, kind: IssueKind, message: impl Into<String>) -> Item {
    Item::Issue(FileIssue {
        path: path.to_path_buf(),
        kind,
        message: message.into(),
    })
}

fn process(paths: &[PathBuf], ctx: &Ctx) -> Vec<Item> {
    let conn = Connection::open_with_flags(
        ctx.db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok();
    let mut lookup = conn.as_ref().and_then(|c| {
        c.prepare(
            "SELECT id,size_bytes,modified_at_ns,inode,is_cloud_only,media_kind FROM file \
             WHERE full_path=?1",
        )
        .ok()
    });
    let mut out = Vec::new();

    for path in paths {
        if ctx.cancel.is_cancelled() {
            break;
        }
        let md = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) => {
                out.push(issue(path, IssueKind::from_io(e.kind()), e.to_string()));
                continue;
            }
        };
        if !md.is_file() {
            continue;
        }
        ctx.seen.fetch_add(1, Ordering::Relaxed);

        let ext = path.extension().and_then(|e| e.to_str());
        if ext.and_then(Format::from_extension).is_none() && !ctx.sniff_all {
            continue; // not media: no database lookup, no I/O
        }

        let dataless = is_dataless(&md);
        let size = md.st_size() as i64;
        let mtime_ns = md.st_mtime() * 1_000_000_000 + md.st_mtime_nsec();
        let ino = md.st_ino() as i64;

        // Incremental: unchanged (size, mtime, ino) → just refresh last_seen_scan.
        if let Some(stmt) = lookup.as_mut() {
            let row = stmt
                .query_row(params![path.as_os_str().as_bytes()], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, bool>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })
                .optional()
                .ok()
                .flatten();
            if let Some((id, osize, omtime, oino, ocloud, okind)) = row {
                let kind_ok = match okind.as_str() {
                    "image" => ctx.kind_ok(MediaKind::Image),
                    _ => ctx.kind_ok(MediaKind::Video),
                };
                if osize == size
                    && omtime == mtime_ns
                    && oino == ino
                    && ocloud == dataless
                    && kind_ok
                {
                    ctx.bytes.fetch_add(size as u64, Ordering::Relaxed);
                    out.push(Item::Touch(id));
                    continue;
                }
            }
        }

        let format = match detect::detect(path, dataless, ctx.sniff_all) {
            Ok(Some(f)) if ctx.kind_ok(f.kind()) => f,
            Ok(_) => continue,
            Err(e) => {
                out.push(issue(path, IssueKind::from_io(e.kind()), e.to_string()));
                continue;
            }
        };
        ctx.bytes.fetch_add(size as u64, Ordering::Relaxed);
        out.push(Item::Upsert(FileRecord {
            path: path.as_os_str().as_bytes().to_vec(),
            parent: path
                .parent()
                .map_or_else(Vec::new, |p| p.as_os_str().as_bytes().to_vec()),
            name: path
                .file_name()
                .map_or_else(Vec::new, |n| n.as_bytes().to_vec()),
            ext: ext.map(str::to_ascii_lowercase),
            size,
            mtime_ns,
            ctime_ns: md.st_ctime() * 1_000_000_000 + md.st_ctime_nsec(),
            btime_ns: Some(md.st_birthtime() * 1_000_000_000 + md.st_birthtime_nsec()),
            dev: md.st_dev() as i64,
            ino,
            kind: kind_str(format.kind()),
            format: format.as_str(),
            cloud_only: dataless,
        }));
    }
    out
}

/// `root` must already be canonical and checked against [`Protected`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    config: &Config,
    own_dirs: &[PathBuf],
    protected: &Protected,
    root: &Path,
    opts: &ScanOptions,
    index_path: &Path,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<ScanSummary> {
    let started = Instant::now();
    let conn = Index::open(index_path)?.into_connection();
    // The last finished scan is the best guess for how many files this one will see.
    let expected: Option<u64> = conn
        .query_row(
            "SELECT files_seen FROM scan WHERE status='complete' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten()
        .filter(|n| *n > 0)
        .map(|n| n as u64);
    conn.execute(
        "INSERT INTO scan(root_path,started_at,status) VALUES(?1,?2,'running')",
        params![root.as_os_str().as_bytes(), now_secs()],
    )?;
    let scan_id = conn.last_insert_rowid();
    sink.important(CoreEvent::TaskStarted {
        task: TaskKind::Scan,
        label: format!("Scanning {}", root.display()),
    });

    let threads = if config.threads == 0 {
        std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(2).max(1))
    } else {
        config.threads
    };
    let pool = crate::pool::build("scan-worker", threads)?;
    tracing::info!(root = %root.display(), threads, expected = ?expected, force = opts.force, "scan: starting");

    let (tx, rx) = crossbeam_channel::bounded::<Vec<Item>>(8);
    let writer = std::thread::Builder::new()
        .name("scan-writer".into())
        .spawn(move || {
            tracing::debug!("thread scan-writer started");
            let r = writer::run(conn, rx, scan_id);
            match &r {
                Ok((_, st)) => tracing::debug!(issues = st.issues, "thread scan-writer stopped"),
                Err(e) => tracing::error!("thread scan-writer failed: {e}"),
            }
            r
        })
        .map_err(|e| YojakaError::Internal(format!("cannot start the writer thread: {e}")))?;

    // Directory filter: the walker asks before reading each directory. A rejected directory is
    // still yielded as an entry, but nothing below it is read.
    let protected_hits: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::default();
    let skip_hidden = config.scan.skip_hidden;
    let descend = {
        let skip_names = config.scan.skip_dirs.clone();
        let own = own_dirs.to_vec();
        let protected = protected.clone();
        let hits = protected_hits.clone();
        move |e: &dua_core::Entry| -> bool {
            let name = e.file_name.to_string_lossy().to_ascii_lowercase();
            let path = e.path();
            if BUNDLE_SUFFIXES.iter().any(|s| name.ends_with(s))
                || own.iter().any(|o| path.starts_with(o))
            {
                return false;
            }
            if e.depth == 0 {
                // The user picked this folder: the name rules below apply inside it only, so a
                // folder called "Library" or "node_modules" can still be scanned on purpose.
                return true;
            }
            if (skip_hidden && name.starts_with('.'))
                || name == ".yojaka"
                || skip_names.iter().any(|s| s.eq_ignore_ascii_case(&name))
            {
                return false;
            }
            if let Some(reason) = protected.reason(&path) {
                if let Ok(mut h) = hits.lock() {
                    h.push((path, reason));
                }
                return false;
            }
            true
        }
    };
    // Unordered (`Completion`) with a bounded result queue: memory stays flat and a slow consumer
    // only makes the walker wait. Do not switch to an ordered walk combined with any kind of
    // waiting: that is the deadlock in docs/adr/0005-directory-walker.md.
    let mut walker = dua_core::walk(
        root,
        threads,
        dua_core::Order::Completion,
        dua_core::Options::default().skip_metadata(),
        descend,
    );

    let ctx = Ctx {
        db: index_path,
        kinds: &opts.kinds,
        sniff_all: opts.sniff_all,
        cancel,
        seen: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
    };

    let mut chunk: Vec<PathBuf> = Vec::with_capacity(CHUNK);
    let mut extra: Vec<Item> = Vec::new();
    let mut last_emit = Instant::now();
    let mut last_dir: Option<Arc<Path>> = None;
    let mut writer_alive = true;

    let flush = |chunk: &mut Vec<PathBuf>, extra: &mut Vec<Item>| -> bool {
        let mut items: Vec<Item> = pool.install(|| {
            chunk
                .par_chunks(SUB_CHUNK)
                .flat_map_iter(|c| process(c, &ctx))
                .collect()
        });
        items.append(extra);
        chunk.clear();
        for it in &items {
            if let Item::Issue(i) = it {
                tracing::warn!(path = %i.path.display(), kind = ?i.kind, "{}", i.message);
                sink.important(CoreEvent::Issue(i.clone()));
            }
        }
        items.is_empty() || tx.send(items).is_ok()
    };
    let emit = |dir: Option<&Arc<Path>>| {
        let done = ctx.seen.load(Ordering::Relaxed);
        sink.progress(CoreEvent::Progress {
            task: TaskKind::Scan,
            done,
            // The last scan's count is only a guess. Once passed, the size is unknown again.
            total: expected.filter(|&e| e > done),
            bytes: ctx.bytes.load(Ordering::Relaxed),
            current: dir.map(|d| d.to_path_buf()),
        });
    };

    while let Some(entry) = walker.next_cancellable(cancel.flag()) {
        if cancel.is_cancelled() {
            break;
        }
        match entry {
            Ok(e) if e.file_type.is_file() && !(skip_hidden && is_hidden(&e.file_name)) => {
                chunk.push(e.path());
                last_dir = Some(e.parent_path.clone());
                if chunk.len() >= CHUNK && !flush(&mut chunk, &mut extra) {
                    writer_alive = false;
                    break;
                }
            }
            Ok(_) => {} // directories, symlinks, hidden files and other non-files
            Err(err) => {
                let path = err
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
                let kind = IssueKind::from_io(err.io_error().kind());
                tracing::warn!(path = %path.display(), ?kind, "scan: walk error: {err}");
                extra.push(issue(&path, kind, err.to_string()));
            }
        }
        // Time-based, not per batch: a quiet stretch means the walk itself is waiting.
        if last_emit.elapsed() >= PROGRESS_EVERY {
            last_emit = Instant::now();
            emit(last_dir.as_ref());
        }
    }
    drop(walker); // stops and joins the walker threads
    tracing::debug!(
        files_seen = ctx.seen.load(Ordering::Relaxed),
        cancelled = cancel.is_cancelled(),
        "scan: walk finished"
    );
    if writer_alive && !cancel.is_cancelled() && !flush(&mut chunk, &mut extra) {
        writer_alive = false;
    }
    if !cancel.is_cancelled() {
        // The walk is over, so the real size is known. The UI shows "finishing up" from here.
        let done = ctx.seen.load(Ordering::Relaxed);
        sink.important(CoreEvent::Progress {
            task: TaskKind::Scan,
            done,
            total: Some(done),
            bytes: ctx.bytes.load(Ordering::Relaxed),
            current: None,
        });
    }
    if let Ok(hits) = protected_hits.lock() {
        for (path, reason) in hits.iter() {
            extra.push(issue(
                &path.clone(),
                IssueKind::Protected,
                format!("skipped: {reason}"),
            ));
        }
    }
    if writer_alive && !extra.is_empty() {
        for it in &extra {
            if let Item::Issue(i) = it {
                sink.important(CoreEvent::Issue(i.clone()));
            }
        }
        let _ = tx.send(std::mem::take(&mut extra));
    }
    drop(tx);

    let (conn, wstats) = writer
        .join()
        .map_err(|_| YojakaError::Internal("index writer thread panicked".into()))??;

    let cancelled = cancel.is_cancelled();
    tracing::debug!(cancelled, issues = wstats.issues, "scan: writer joined");
    let pruned = if cancelled {
        0 // a partial scan has not seen everything: never prune on it
    } else {
        conn.execute(
            "DELETE FROM file WHERE last_seen_scan_id < ?1",
            params![scan_id],
        )? as u64
    };
    let (indexed, cloud_only, bytes): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(is_cloud_only),0), COALESCE(SUM(size_bytes),0) \
         FROM file WHERE last_seen_scan_id=?1",
        params![scan_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let status = if cancelled {
        ScanStatus::Partial
    } else {
        ScanStatus::Complete
    };
    let seen = ctx.seen.load(Ordering::Relaxed);
    conn.execute(
        "UPDATE scan SET finished_at=?1,status=?2,files_seen=?3,error_count=?4 WHERE id=?5",
        params![
            now_secs(),
            if cancelled { "partial" } else { "complete" },
            seen as i64,
            wstats.issues as i64,
            scan_id
        ],
    )?;

    sink.important(CoreEvent::TaskFinished {
        task: TaskKind::Scan,
        outcome: if cancelled {
            TaskOutcome::Cancelled
        } else {
            TaskOutcome::Complete
        },
    });
    Ok(ScanSummary {
        scan_id,
        status,
        files_seen: seen,
        files_indexed: indexed as u64,
        new_or_changed: wstats.upserted,
        unchanged: wstats.touched,
        pruned,
        cloud_only: cloud_only as u64,
        bytes: bytes as u64,
        issues: wstats.issues,
        permission_denied: wstats.permission_denied,
        elapsed_ms: started.elapsed().as_millis() as u64,
        metadata: None,
    })
}
