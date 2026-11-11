//! The exact tier: files with identical bytes. Also the shared loading and checking of index rows.
//!
//! Staged so that as few bytes as possible are read (tech spec §6.3):
//! 1. group by size (SQL), collapsing hard links so one file is never "a duplicate of itself";
//! 2. partial hash (size + first and last 16 KiB) of files that share a size;
//! 3. full hash of files that also share a partial hash.
//!
//! Hashes are cached in the index (`file.partial_hash` / `file.full_hash`); a rescan clears them
//! when a file's size or mtime changes. Cloud-only (dataless) files are never opened.

use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;
use rusqlite::Connection;

use super::{DupFile, DupGroup, DupItem, DupOptions, DupStats, Tier};
use crate::hash::{self, ContentHash};
use crate::index::Index;
use crate::model::{FileId, MediaKind};
use crate::{CancelToken, Config, CoreEvent, EventSink, FileIssue, IssueKind, Result, TaskKind};

/// Files hashed between progress reports, cache writes and cancel checks.
const CHUNK: usize = 64;

pub(super) struct Rec {
    pub id: i64,
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: i64,
    pub dev: i64,
    pub ino: i64,
    pub partial: Option<ContentHash>,
    pub full: Option<ContentHash>,
    pub kind: MediaKind,
    pub taken_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i64>,
    pub pair_id: Option<i64>,
    pub camera: Option<String>,
    /// Still a candidate (readable, unchanged, not a hard-link alias).
    pub ok: bool,
}

impl Rec {
    pub fn dup_file(&self) -> DupFile {
        DupFile {
            id: FileId(self.id),
            path: self.path.clone(),
            size: self.size,
            mtime_ns: self.mtime_ns,
            kind: self.kind,
            taken_at: self.taken_at,
            width: self.width,
            height: self.height,
            duration_ms: self.duration_ms,
            bitrate: self.bitrate,
            camera: self.camera.clone(),
            hash: self.full,
        }
    }
}

pub(super) struct Run<'a> {
    pub pool: rayon::ThreadPool,
    pub conn: Connection,
    pub sink: &'a EventSink,
    pub cancel: &'a CancelToken,
    pub stats: DupStats,
    pub issues: Vec<FileIssue>,
    /// Paths already reported, so a file that several tiers look at is listed once.
    reported: HashSet<PathBuf>,
}

impl<'a> Run<'a> {
    pub fn new(
        config: &Config,
        index_path: &Path,
        sink: &'a EventSink,
        cancel: &'a CancelToken,
    ) -> Result<Self> {
        let threads = if config.threads == 0 {
            std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(2).max(1))
        } else {
            config.threads
        };
        let pool = crate::pool::build("dupes-worker", threads)?;
        tracing::info!(threads, "dupes: starting");
        Ok(Self {
            pool,
            conn: Index::open(index_path)?.into_connection(),
            sink,
            cancel,
            stats: DupStats::default(),
            issues: Vec::new(),
            reported: HashSet::new(),
        })
    }

    pub fn issue(&mut self, path: &Path, kind: IssueKind, message: String) {
        if !self.reported.insert(path.to_path_buf()) {
            return;
        }
        tracing::warn!(path = %path.display(), "dupes: {message}");
        let issue = FileIssue {
            path: path.to_path_buf(),
            kind,
            message,
        };
        self.sink.important(CoreEvent::Issue(issue.clone()));
        self.issues.push(issue);
    }
}

/// Which exact group (by position in the returned list) each file belongs to.
pub(super) type ExactOf = HashMap<i64, u32>;

/// The exact tier. Reads (and caches hashes for) only files whose size is shared with another
/// file. Cancelling returns `Err(Cancelled)`; hashes saved so far are kept for the next run.
pub(super) fn run_exact(run: &mut Run<'_>, opts: DupOptions) -> Result<(Vec<DupGroup>, ExactOf)> {
    let cancel = run.cancel;
    let mut recs = load_recs(&run.conn, SAME_SIZE)?;
    cancel.check()?;
    check_unchanged(run, &mut recs);
    collapse_hard_links(run, &mut recs, true);

    // Stage 1: size.
    let mut cands = group_idx(&recs, |r| r.size);
    run.stats.candidate_files = cands.iter().map(|g| g.len() as u64).sum();
    run.stats.candidate_bytes = cands.iter().flatten().map(|&i| recs[i].size).sum();

    // Stage 2: partial hash.
    let todo: Vec<usize> = cands
        .iter()
        .flatten()
        .copied()
        .filter(|&i| opts.rehash || recs[i].partial.is_none())
        .collect();
    run.stats.cached_hashes += (cands.iter().map(Vec::len).sum::<usize>() - todo.len()) as u64;
    hash_stage(run, &mut recs, todo, Stage::Partial)?;
    let by_partial = group_by_key(&recs, cands.drain(..).flatten(), |r| {
        r.partial.map(|p| (r.size, p))
    });

    // Stage 3: full hash.
    let todo: Vec<usize> = by_partial
        .iter()
        .flatten()
        .copied()
        .filter(|&i| opts.rehash || recs[i].full.is_none())
        .collect();
    run.stats.cached_hashes += (by_partial.iter().map(Vec::len).sum::<usize>() - todo.len()) as u64;
    hash_stage(run, &mut recs, todo, Stage::Full)?;
    let by_full = group_by_key(&recs, by_partial.into_iter().flatten(), |r| r.full);

    let groups = build_groups(&run.conn, &recs, &by_full)?;
    let mut exact_of = ExactOf::new();
    for (n, g) in groups.iter().enumerate() {
        for f in g.members.iter().flat_map(|m| &m.files) {
            exact_of.insert(f.id.0, n as u32);
        }
    }
    Ok((groups, exact_of))
}

/// Rows with a size shared by at least two distinct files (two names for one inode do not count).
const SAME_SIZE: &str = "f.size_bytes IN ( \
    SELECT size_bytes FROM file WHERE is_cloud_only = 0 AND size_bytes > 0 \
    GROUP BY size_bytes HAVING COUNT(DISTINCT device_id || ':' || inode) > 1)";

/// Non-empty, not cloud-only files matching `filter` (SQL over `file f` and `meta m`).
pub(super) fn load_recs(conn: &Connection, filter: &str) -> Result<Vec<Rec>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT f.id, f.full_path, f.size_bytes, f.modified_at_ns, f.device_id, f.inode, \
                f.partial_hash, f.full_hash, f.media_kind, \
                m.taken_at, m.width, m.height, m.duration_ms, m.bitrate, m.pair_id, \
                COALESCE(m.camera_model, m.camera_make) \
         FROM file f LEFT JOIN meta m ON m.file_id = f.id \
         WHERE f.is_cloud_only = 0 AND f.size_bytes > 0 AND {filter} \
         ORDER BY f.id"
    ))?;
    let blob = |r: &rusqlite::Row<'_>, i: usize| -> rusqlite::Result<Option<ContentHash>> {
        Ok(r.get::<_, Option<Vec<u8>>>(i)?
            .and_then(|b| ContentHash::from_blob(&b)))
    };
    let rows = stmt.query_map([], |r| {
        let kind: String = r.get(8)?;
        let dim = |i: usize| -> rusqlite::Result<Option<u32>> {
            Ok(r.get::<_, Option<i64>>(i)?
                .and_then(|v| u32::try_from(v).ok()))
        };
        Ok(Rec {
            id: r.get(0)?,
            path: PathBuf::from(std::ffi::OsString::from_vec(r.get::<_, Vec<u8>>(1)?)),
            size: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            mtime_ns: r.get(3)?,
            dev: r.get(4)?,
            ino: r.get(5)?,
            partial: blob(r, 6)?,
            full: blob(r, 7)?,
            kind: if kind == "video" {
                MediaKind::Video
            } else {
                MediaKind::Image
            },
            taken_at: r.get(9)?,
            width: dim(10)?,
            height: dim(11)?,
            duration_ms: r.get(12)?,
            bitrate: r.get(13)?,
            pair_id: r.get(14)?,
            camera: r.get(15)?,
            ok: true,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Drops files that vanished or changed since the scan (a stale index must not decide what is
/// a duplicate). Only `stat` is used; nothing is opened.
pub(super) fn check_unchanged(run: &mut Run<'_>, recs: &mut [Rec]) {
    let verdicts: Vec<Option<(IssueKind, String)>> = run.pool.install(|| {
        recs.par_iter()
            .map(|r| match std::fs::symlink_metadata(&r.path) {
                Err(e) => Some((IssueKind::from_io(e.kind()), e.to_string())),
                Ok(md) => {
                    let mtime = md.mtime() * 1_000_000_000 + md.mtime_nsec();
                    if !md.is_file() || md.len() != r.size || mtime != r.mtime_ns {
                        Some((
                            IssueKind::Changed,
                            "changed since the last scan; rescan to include it".into(),
                        ))
                    } else {
                        None
                    }
                }
            })
            .collect()
    });
    for (r, v) in recs.iter_mut().zip(verdicts) {
        if let Some((kind, msg)) = v {
            r.ok = false;
            run.issue(&r.path.clone(), kind, msg);
        }
    }
}

/// Keeps the lowest id of each `(device, inode)`; the other names are not duplicates of it.
/// `count` adds the dropped names to the statistics (only the exact tier does, so a name is
/// counted once).
pub(super) fn collapse_hard_links(run: &mut Run<'_>, recs: &mut [Rec], count: bool) {
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    for r in recs.iter_mut().filter(|r| r.ok) {
        if !seen.insert((r.dev, r.ino)) {
            r.ok = false;
            if count {
                run.stats.hardlinks_skipped += 1;
            }
        }
    }
}

/// Groups of two or more usable records with the same key, as indices into `recs`.
fn group_idx<K: std::hash::Hash + Eq>(recs: &[Rec], key: impl Fn(&Rec) -> K) -> Vec<Vec<usize>> {
    group_by_key(recs, 0..recs.len(), |r| Some(key(r)))
}

fn group_by_key<K: std::hash::Hash + Eq>(
    recs: &[Rec],
    idxs: impl IntoIterator<Item = usize>,
    key: impl Fn(&Rec) -> Option<K>,
) -> Vec<Vec<usize>> {
    let mut map: HashMap<K, Vec<usize>> = HashMap::new();
    for i in idxs {
        let r = &recs[i];
        if r.ok
            && let Some(k) = key(r)
        {
            map.entry(k).or_default().push(i);
        }
    }
    let mut groups: Vec<Vec<usize>> = map.into_values().filter(|g| g.len() > 1).collect();
    groups.sort_by_key(|g| g[0]);
    groups
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Partial,
    Full,
}

fn hash_stage(run: &mut Run<'_>, recs: &mut [Rec], todo: Vec<usize>, stage: Stage) -> Result<()> {
    if todo.is_empty() {
        return Ok(());
    }
    let total = todo.len() as u64;
    let mut done = 0u64;
    let mut last_emit = Instant::now();
    for chunk in todo.chunks(CHUNK) {
        run.cancel.check()?;
        let results: Vec<(usize, std::io::Result<hash::Hashed>)> = run.pool.install(|| {
            chunk
                .par_iter()
                .map(|&i| {
                    let r = &recs[i];
                    let h = match stage {
                        Stage::Partial => hash::partial_hash(&r.path, r.size),
                        Stage::Full => hash::full_hash(&r.path, r.size),
                    };
                    (i, h)
                })
                .collect()
        });
        let tx = run.conn.transaction()?;
        {
            let column = match stage {
                Stage::Partial => "partial_hash",
                Stage::Full => "full_hash",
            };
            // Guarded by size and mtime so a rescan that raced with us is not overwritten.
            let mut save = tx.prepare(&format!(
                "UPDATE file SET {column} = ?1 WHERE id = ?2 AND size_bytes = ?3 AND modified_at_ns = ?4"
            ))?;
            for (i, res) in &results {
                let r = &recs[*i];
                if let Ok(h) = res {
                    save.execute(rusqlite::params![
                        &h.hash.0[..],
                        r.id,
                        i64::try_from(r.size).unwrap_or(i64::MAX),
                        r.mtime_ns
                    ])?;
                }
            }
        }
        tx.commit()?;
        for (i, res) in results {
            match res {
                Ok(h) => {
                    run.stats.bytes_read += h.bytes_read;
                    match stage {
                        Stage::Partial => {
                            recs[i].partial = Some(h.hash);
                            run.stats.partial_hashed += 1;
                        }
                        Stage::Full => {
                            recs[i].full = Some(h.hash);
                            run.stats.full_hashed += 1;
                        }
                    }
                }
                Err(e) => {
                    recs[i].ok = false;
                    let path = recs[i].path.clone();
                    run.issue(&path, IssueKind::from_io(e.kind()), e.to_string());
                }
            }
        }
        done += chunk.len() as u64;
        if last_emit.elapsed().as_millis() >= 100 || done == total {
            last_emit = Instant::now();
            run.sink.progress(CoreEvent::Progress {
                task: TaskKind::Hash,
                done,
                total: Some(total),
                bytes: run.stats.bytes_read,
                current: chunk.last().map(|&i| recs[i].path.clone()),
            });
        }
    }
    Ok(())
}

#[derive(Hash, PartialEq, Eq)]
enum Key {
    Single(i64),
    Pair(i64),
}

/// Turns same-hash file groups into groups of items. A Live Photo pair is one item whose
/// identity is the hashes of both halves, so a pair only ever matches another pair with the same
/// image *and* the same video; it is never split and never traded for a lone image.
fn build_groups(conn: &Connection, recs: &[Rec], by_full: &[Vec<usize>]) -> Result<Vec<DupGroup>> {
    let mut pairs: HashMap<i64, Vec<i64>> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT pair_id, file_id FROM meta WHERE pair_id IS NOT NULL")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (p, f) = row?;
            pairs.entry(p).or_default().push(f);
        }
    }
    let by_id: HashMap<i64, usize> = recs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.ok && r.full.is_some())
        .map(|(i, r)| (r.id, i))
        .collect();

    let dup_file = Rec::dup_file;

    let mut seen: HashSet<Key> = HashSet::new();
    let mut by_sig: HashMap<Vec<Option<ContentHash>>, Vec<DupItem>> = HashMap::new();
    for &i in by_full.iter().flatten() {
        let r = &recs[i];
        let key = match r.pair_id {
            Some(p) => Key::Pair(p),
            None => Key::Single(r.id),
        };
        let item = match key {
            Key::Single(_) => {
                if !seen.insert(key) {
                    continue;
                }
                DupItem {
                    files: vec![dup_file(r)],
                }
            }
            Key::Pair(p) => {
                if !seen.insert(key) {
                    continue;
                }
                let Some(ids) = pairs.get(&p).filter(|ids| ids.len() == 2) else {
                    continue;
                };
                let (Some(&a), Some(&b)) = (by_id.get(&ids[0]), by_id.get(&ids[1])) else {
                    continue;
                };
                let (img, vid) = match (recs[a].kind, recs[b].kind) {
                    (MediaKind::Image, MediaKind::Video) => (a, b),
                    (MediaKind::Video, MediaKind::Image) => (b, a),
                    _ => continue,
                };
                DupItem {
                    files: vec![dup_file(&recs[img]), dup_file(&recs[vid])],
                }
            }
        };
        let sig: Vec<Option<ContentHash>> = item.files.iter().map(|f| f.hash).collect();
        by_sig.entry(sig).or_default().push(item);
    }

    let mut groups: Vec<Vec<DupItem>> = by_sig.into_values().filter(|g| g.len() > 1).collect();
    for g in &mut groups {
        g.sort_by(|a, b| a.primary().path.cmp(&b.primary().path));
    }
    let mut groups: Vec<DupGroup> = groups
        .into_iter()
        .map(|g| DupGroup::new(0, Tier::Exact, g))
        .collect();
    groups.sort_by(|a, b| {
        b.potential_reclaim()
            .cmp(&a.potential_reclaim())
            .then_with(|| {
                a.members[0]
                    .primary()
                    .path
                    .cmp(&b.members[0].primary().path)
            })
    });
    Ok(groups)
}
