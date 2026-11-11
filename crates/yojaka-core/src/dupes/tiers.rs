//! Runs the tiers in order of confidence and turns candidate file ids into groups.
//!
//! The likely and name tiers read only the index. Every candidate is `stat`ed before it is
//! shown (a stale index must not decide what is a duplicate), hard links are collapsed, and
//! files that are already identical (same exact group) count once.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use super::exact::{self, ExactOf, Rec, Run};
use super::{DupGroup, DupItem, DupOptions, DupReport, Tier, name, similar};
use crate::model::MediaKind;
use crate::{CancelToken, Config, EventSink, Result};

/// Videos count as the same clip when their lengths differ by at most this.
const DURATION_SLACK_MS: i64 = 1000;
/// Name tier: the larger file may be at most this many percent bigger than the smaller one.
const SIZE_SLACK_PERCENT: u64 = 5;

pub(super) fn find(
    config: &Config,
    opts: DupOptions,
    index_path: &Path,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<DupReport> {
    let mut run = Run::new(config, index_path, sink, cancel)?;

    // Always: the other tiers must know which files are already identical.
    let (exact_groups, exact_of) = exact::run_exact(&mut run, opts)?;
    run.stats.potential_reclaim = exact_groups.iter().map(DupGroup::potential_reclaim).sum();
    let mut groups = if opts.exact { exact_groups } else { Vec::new() };
    run.stats.exact_groups = groups.len() as u64;

    // Member sets of the non-exact groups so far: a later, weaker tier does not repeat them.
    let mut shown: Vec<HashSet<i64>> = Vec::new();
    if opts.likely {
        cancel.check()?;
        let ids = likely_candidates(&run.conn)?;
        let found = materialize(&mut run, Tier::Likely, ids, &exact_of, &mut shown)?;
        run.stats.likely_groups = found.len() as u64;
        groups.extend(found);
    }
    if opts.similar {
        cancel.check()?;
        let max_bits = opts
            .similar_distance
            .unwrap_or(config.dupes.similar_distance)
            .min(32);
        let ids = similar::candidates(&mut run, max_bits)?;
        let found = materialize(&mut run, Tier::Similar, ids, &exact_of, &mut shown)?;
        run.stats.similar_groups = found.len() as u64;
        groups.extend(found);
    }
    if opts.name {
        cancel.check()?;
        let ids = name_candidates(&run.conn)?;
        let found = materialize(&mut run, Tier::Name, ids, &exact_of, &mut shown)?;
        run.stats.name_groups = found.len() as u64;
        groups.extend(found);
    }

    for (n, g) in groups.iter_mut().enumerate() {
        g.id = n as u32 + 1;
    }
    run.stats.groups = groups.len() as u64;
    Ok(DupReport {
        groups,
        stats: run.stats,
        issues: run.issues,
    })
}

/// Runs of two or more consecutive rows with an equal key. `rows` must be sorted by key.
fn runs<K: PartialEq, T>(rows: Vec<(K, T)>) -> Vec<Vec<T>> {
    let mut out: Vec<Vec<T>> = Vec::new();
    let mut current: Vec<T> = Vec::new();
    let mut key: Option<K> = None;
    for (k, v) in rows {
        if key.as_ref() != Some(&k) {
            if current.len() > 1 {
                out.push(std::mem::take(&mut current));
            }
            current.clear();
            key = Some(k);
        }
        current.push(v);
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// Splits values sorted ascending into runs where each value is within `near` of the run's
/// first value; runs of one are dropped.
fn near_runs<T>(sorted: Vec<(u64, T)>, near: impl Fn(u64, u64) -> bool) -> Vec<Vec<T>> {
    let mut out: Vec<Vec<T>> = Vec::new();
    let mut current: Vec<T> = Vec::new();
    let mut anchor = 0u64;
    for (v, item) in sorted {
        if current.is_empty() || !near(anchor, v) {
            if current.len() > 1 {
                out.push(std::mem::take(&mut current));
            }
            current.clear();
            anchor = v;
        }
        current.push(item);
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// Likely tier candidates, from the index only.
///
/// Photos: same date taken (from EXIF), camera, width and height. Videos: same recorded date,
/// width and height, and lengths within a second. Photos and videos are never mixed. The video
/// half of a Live Photo is not looked at on its own: it follows its photo.
fn likely_candidates(conn: &Connection) -> Result<Vec<Vec<i64>>> {
    const BASE: &str = "FROM file f JOIN meta m ON m.file_id = f.id \
        WHERE f.is_cloud_only = 0 AND f.size_bytes > 0 AND m.taken_at IS NOT NULL \
          AND m.width IS NOT NULL AND m.height IS NOT NULL";
    let mut out;
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT f.id, m.taken_at, COALESCE(m.camera_model, ''), m.width, m.height {BASE} \
               AND f.media_kind = 'image' AND m.taken_at_src = 'exif' \
             ORDER BY m.taken_at, 3, m.width, m.height, f.id"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    (
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ),
                    r.get::<_, i64>(0)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out = runs(rows);
    }
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT f.id, m.taken_at, m.width, m.height, m.duration_ms {BASE} \
               AND f.media_kind = 'video' AND m.taken_at_src != 'none' \
               AND m.duration_ms IS NOT NULL AND m.pair_id IS NULL \
             ORDER BY m.taken_at, m.width, m.height, m.duration_ms, f.id"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    (
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ),
                    (r.get::<_, i64>(4)?.max(0) as u64, r.get::<_, i64>(0)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for same in runs(rows) {
            out.extend(near_runs(same, |a, b| {
                b.saturating_sub(a) <= DURATION_SLACK_MS as u64
            }));
        }
    }
    Ok(out)
}

/// Name tier candidates: same kind, same normalized name, sizes within 5% of the smallest.
fn name_candidates(conn: &Connection) -> Result<Vec<Vec<i64>>> {
    // (key, (size, id)): 32 bytes a file, so a million files stay small. The key is a hash of
    // the kind and the normalized name.
    let mut rows: Vec<(u64, (u64, i64))> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT f.id, f.file_name, f.size_bytes, f.media_kind \
             FROM file f LEFT JOIN meta m ON m.file_id = f.id \
             WHERE f.is_cloud_only = 0 AND f.size_bytes > 0 \
               AND NOT (f.media_kind = 'video' AND m.pair_id IS NOT NULL)",
        )?;
        let mut q = stmt.query([])?;
        while let Some(r) = q.next()? {
            let file_name: Vec<u8> = r.get(1)?;
            let kind: String = r.get(3)?;
            let mut h = blake3::Hasher::new();
            h.update(kind.as_bytes());
            h.update(&[0]);
            h.update(name::normalize(&file_name).as_bytes());
            let mut key = [0u8; 8];
            key.copy_from_slice(&h.finalize().as_bytes()[..8]);
            rows.push((
                u64::from_le_bytes(key),
                (u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0), r.get(0)?),
            ));
        }
    }
    rows.sort_unstable();
    let mut out = Vec::new();
    for same in runs(rows) {
        out.extend(near_runs(same, |smallest, size| {
            u128::from(size) * 100 <= u128::from(smallest) * u128::from(100 + SIZE_SLACK_PERCENT)
        }));
    }
    Ok(out)
}

/// Loads, checks and groups candidate files of one non-exact tier.
fn materialize(
    run: &mut Run<'_>,
    tier: Tier,
    id_groups: Vec<Vec<i64>>,
    exact_of: &ExactOf,
    shown: &mut Vec<HashSet<i64>>,
) -> Result<Vec<DupGroup>> {
    if id_groups.is_empty() {
        return Ok(Vec::new());
    }
    // The candidates and, for Live Photos, their other halves.
    run.conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS cand(id INTEGER PRIMARY KEY); DELETE FROM cand;",
    )?;
    {
        let tx = run.conn.transaction()?;
        {
            let mut ins = tx.prepare("INSERT OR IGNORE INTO cand(id) VALUES (?1)")?;
            for id in id_groups.iter().flatten() {
                ins.execute([id])?;
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO cand(id) \
             SELECT b.file_id FROM meta a JOIN meta b ON b.pair_id = a.pair_id \
             WHERE a.pair_id IS NOT NULL AND a.file_id IN (SELECT id FROM cand)",
            [],
        )?;
        tx.commit()?;
    }
    let mut recs = exact::load_recs(&run.conn, "f.id IN (SELECT id FROM temp.cand)")?;
    run.conn.execute_batch("DELETE FROM cand;")?;
    run.cancel.check()?;
    exact::check_unchanged(run, &mut recs);
    exact::collapse_hard_links(run, &mut recs, false);

    let by_id: HashMap<i64, usize> = recs.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut halves: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, r) in recs.iter().enumerate() {
        if let Some(p) = r.pair_id {
            halves.entry(p).or_default().push(i);
        }
    }
    // One thing to keep or delete: a file, or both halves of a Live Photo (photo first).
    let item_of = |r: &Rec| -> Option<DupItem> {
        if !r.ok {
            return None;
        }
        let Some(pair) = r.pair_id else {
            return Some(DupItem {
                files: vec![r.dup_file()],
            });
        };
        let both = halves.get(&pair).filter(|h| h.len() == 2)?;
        let (a, b) = (&recs[both[0]], &recs[both[1]]);
        let (photo, video) = match (a.kind, b.kind) {
            (MediaKind::Image, MediaKind::Video) => (a, b),
            (MediaKind::Video, MediaKind::Image) => (b, a),
            _ => return None,
        };
        // The pair is listed under its photo, and only when both halves can be acted on.
        (photo.id == r.id && video.ok).then(|| DupItem {
            files: vec![photo.dup_file(), video.dup_file()],
        })
    };

    let mut groups: Vec<DupGroup> = Vec::new();
    for ids in id_groups {
        let mut members: Vec<DupItem> = ids
            .iter()
            .filter_map(|id| by_id.get(id))
            .filter_map(|&i| item_of(&recs[i]))
            .collect();
        members.sort_by(|a, b| a.primary().path.cmp(&b.primary().path));
        // Identical files are the exact tier's business: here they count as one.
        let mut seen_exact: HashSet<u32> = HashSet::new();
        members.retain(|m| match exact_of.get(&m.primary().id.0) {
            Some(g) => seen_exact.insert(*g),
            None => true,
        });
        if members.len() < 2 {
            continue;
        }
        let set: HashSet<i64> = members.iter().map(|m| m.primary().id.0).collect();
        if shown.iter().any(|earlier| set.is_subset(earlier)) {
            continue;
        }
        shown.push(set);
        groups.push(DupGroup::new(0, tier, members));
    }
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

#[cfg(test)]
mod tests {
    use super::{near_runs, runs};

    #[test]
    fn runs_keep_only_repeated_keys() {
        let rows = vec![(1, 'a'), (1, 'b'), (2, 'c'), (3, 'd'), (3, 'e'), (3, 'f')];
        assert_eq!(runs(rows), vec![vec!['a', 'b'], vec!['d', 'e', 'f']]);
        assert!(runs::<i32, char>(vec![]).is_empty());
        assert!(runs(vec![(1, 'a')]).is_empty());
    }

    #[test]
    fn near_runs_measure_from_the_first_of_a_run() {
        let near = |a: u64, b: u64| b - a <= 1000;
        // 0 and 900 belong together; 1800 is too far from 0 and starts again.
        let got = near_runs(
            vec![(0, 1), (900, 2), (1800, 3), (2500, 4), (9000, 5)],
            near,
        );
        assert_eq!(got, vec![vec![1, 2], vec![3, 4]]);
    }
}
