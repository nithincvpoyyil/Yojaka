//! Builds an organize plan from the index (tech spec §6.4). Reads the index and the destination
//! folder's names; never changes a file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use super::template::{Facts, Template, Where, Ymd, sanitize};
use crate::config::LocationConfig;
use crate::geo::Place;
use crate::hash::full_hash;
use crate::index::Index;
use crate::model::{Fingerprint, MediaKind};
use crate::plan::{Op, OpMode, Plan, PlanKind, SkipReason, Warning};
use crate::{CancelToken, CoreEvent, EventSink, Result, TaskKind, TaskOutcome, YojakaError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Copy,
    Move,
}

#[derive(Debug, Clone)]
pub struct OrganizeOptions {
    pub dest: PathBuf,
    pub template: Template,
    pub transfer: Transfer,
    pub infer_location: bool,
    pub location: LocationConfig,
}

/// What the plan says about the library, for the screen and the CLI.
#[derive(Debug, Clone, Default)]
pub struct OrganizeStats {
    pub files: usize,
    /// Files that have a place (real, from a Live Photo partner, or inferred).
    pub with_place: usize,
    pub inferred: usize,
}

impl OrganizeStats {
    /// Share of files with a place, 0..=1.
    pub fn coverage(&self) -> f64 {
        if self.files == 0 {
            0.0
        } else {
            self.with_place as f64 / self.files as f64
        }
    }
}

#[derive(Debug)]
pub struct OrganizePlan {
    pub plan: Plan,
    pub stats: OrganizeStats,
}

#[derive(Debug, Clone)]
struct Row {
    path: PathBuf,
    parent: PathBuf,
    size: u64,
    mtime_ns: i64,
    dev: i64,
    kind: MediaKind,
    format: Option<String>,
    cloud_only: bool,
    taken_at: Option<i64>,
    make: Option<String>,
    model: Option<String>,
    place: Option<Place>,
    pair_id: Option<i64>,
}

fn load(index: &Path) -> Result<Vec<Row>> {
    let idx = Index::open(index)?;
    let mut stmt = idx.connection().prepare(
        "SELECT f.full_path, f.parent_folder, f.size_bytes, f.modified_at_ns, f.device_id, \
                f.media_kind, f.format, f.is_cloud_only, \
                m.taken_at, m.camera_make, m.camera_model, \
                m.place_country_code, m.place_country, m.place_state, m.place_district, \
                m.place_city, m.place_dist_km, m.pair_id \
         FROM file f LEFT JOIN meta m ON m.file_id = f.id \
         ORDER BY f.full_path",
    )?;
    let rows = stmt.query_map([], |r| {
        let path = |i: usize| -> rusqlite::Result<PathBuf> {
            Ok(PathBuf::from(std::ffi::OsString::from_vec(
                r.get::<_, Vec<u8>>(i)?,
            )))
        };
        let kind: String = r.get(5)?;
        let country: Option<String> = r.get(12)?;
        let city: Option<String> = r.get(15)?;
        let place = match (country, city) {
            (None, None) => None,
            (country, city) => Some(Place {
                city: city.unwrap_or_default(),
                admin1: r.get(13)?,
                admin2: r.get(14)?,
                country_code: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                country: country.unwrap_or_default(),
                dist_km: r.get::<_, Option<f64>>(16)?.unwrap_or(0.0),
            }),
        };
        Ok(Row {
            path: path(0)?,
            parent: path(1)?,
            size: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            mtime_ns: r.get(3)?,
            dev: r.get(4)?,
            kind: if kind == "video" {
                MediaKind::Video
            } else {
                MediaKind::Image
            },
            format: r.get(6)?,
            cloud_only: r.get::<_, i64>(7)? != 0,
            taken_at: r.get(8)?,
            make: r.get(9)?,
            model: r.get(10)?,
            place,
            pair_id: r.get(17)?,
        })
    })?;
    rows.collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

/// A single file, or the two halves of a Live Photo. `files[0]` is the one whose facts name the
/// folder (the photo).
struct Unit {
    files: Vec<usize>,
}

fn units(rows: &[Row]) -> Vec<Unit> {
    let mut by_pair: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        if let Some(p) = r.pair_id {
            by_pair.entry(p).or_default().push(i);
        }
    }
    let mut paired: HashSet<usize> = HashSet::new();
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if paired.contains(&i) {
            continue;
        }
        match r.pair_id.and_then(|p| by_pair.get(&p)) {
            Some(members) if members.len() == 2 => {
                let mut f = members.clone();
                // The photo first.
                f.sort_by_key(|&j| rows[j].kind != MediaKind::Image);
                paired.extend(f.iter().copied());
                out.push(Unit { files: f });
            }
            _ => out.push(Unit { files: vec![i] }),
        }
    }
    out
}

fn ymd(secs: i64) -> Option<Ymd> {
    let d = time::OffsetDateTime::from_unix_timestamp(secs).ok()?.date();
    Some(Ymd {
        year: d.year(),
        month: u8::from(d.month()),
        day: d.day(),
    })
}

fn guarded(p: &Place, loc: &LocationConfig) -> Where {
    let g = p.guarded(loc.near_km, loc.far_km);
    let nonempty = |s: &str| (!s.trim().is_empty()).then(|| s.to_string());
    let country = g.country.as_deref().and_then(nonempty);
    Where {
        country_code: country.as_ref().and_then(|_| nonempty(&p.country_code)),
        state: country
            .as_ref()
            .and_then(|_| p.admin1.as_deref().and_then(nonempty)),
        district: g
            .city
            .as_ref()
            .and_then(|_| p.admin2.as_deref().and_then(nonempty)),
        city: g.city.as_deref().and_then(nonempty),
        country,
    }
}

/// Key for "is this destination taken": case-insensitive and NFC, like APFS.
fn norm_key(p: &Path) -> String {
    p.to_string_lossy().nfc().collect::<String>().to_lowercase()
}

fn with_suffix(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// The nearest existing ancestor of `p` (the volume a new path will land on).
fn existing_ancestor(p: &Path) -> Option<&Path> {
    p.ancestors().find(|a| a.symlink_metadata().is_ok())
}

fn same_content(a: &Path, b: &Path, size: u64) -> bool {
    let Ok(mb) = std::fs::symlink_metadata(b) else {
        return false;
    };
    if !mb.is_file() || mb.len() != size {
        return false;
    }
    matches!((full_hash(a, size), full_hash(b, size)), (Ok(x), Ok(y)) if x.hash == y.hash)
}

/// Places for files that have none: from a geotagged file of the same camera within the window,
/// or (for a camera with no geotagged file at all) from the same folder.
fn infer(
    rows: &[Row],
    units: &[Unit],
    unit_place: &[Option<Place>],
    window_secs: i64,
) -> HashMap<usize, Place> {
    type Pts = Vec<(i64, usize)>; // (taken_at, unit)
    let mut by_cam: HashMap<String, Pts> = HashMap::new();
    let mut by_folder: HashMap<&Path, Pts> = HashMap::new();
    for (u, unit) in units.iter().enumerate() {
        let r = &rows[unit.files[0]];
        let (Some(t), Some(_)) = (r.taken_at, &unit_place[u]) else {
            continue;
        };
        if let Some(c) = cam_key(r) {
            by_cam.entry(c).or_default().push((t, u));
        }
        by_folder
            .entry(r.parent.as_path())
            .or_default()
            .push((t, u));
    }
    for v in by_cam.values_mut().chain(by_folder.values_mut()) {
        v.sort_unstable();
    }
    let nearest = |pts: &Pts, t: i64| -> Option<usize> {
        let i = pts.partition_point(|p| p.0 < t);
        [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter_map(|j| pts.get(j))
            .filter(|p| (p.0 - t).abs() <= window_secs)
            .min_by_key(|p| (p.0 - t).abs())
            .map(|p| p.1)
    };
    let mut out = HashMap::new();
    for (u, unit) in units.iter().enumerate() {
        let r = &rows[unit.files[0]];
        let Some(t) = r.taken_at else { continue };
        if unit_place[u].is_some() {
            continue;
        }
        let src = match cam_key(r).and_then(|c| by_cam.get(&c)) {
            Some(pts) => nearest(pts, t),
            None => by_folder
                .get(r.parent.as_path())
                .and_then(|p| nearest(p, t)),
        };
        if let Some(s) = src
            && let Some(p) = &unit_place[s]
        {
            out.insert(u, p.clone());
        }
    }
    out
}

fn cam_key(r: &Row) -> Option<String> {
    match (&r.make, &r.model) {
        (None, None) => None,
        (a, b) => Some(format!(
            "{}|{}",
            a.as_deref().unwrap_or("").to_lowercase(),
            b.as_deref().unwrap_or("").to_lowercase()
        )),
    }
}

pub fn plan(
    index: &Path,
    root: &Path,
    opts: &OrganizeOptions,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<OrganizePlan> {
    sink.important(CoreEvent::TaskStarted {
        task: TaskKind::Plan,
        label: "Planning".into(),
    });
    let result = build(index, root, opts, sink, cancel);
    sink.important(CoreEvent::TaskFinished {
        task: TaskKind::Plan,
        outcome: match &result {
            Ok(_) => TaskOutcome::Complete,
            Err(YojakaError::Cancelled) => TaskOutcome::Cancelled,
            Err(e) => TaskOutcome::Failed(e.to_string()),
        },
    });
    result
}

fn build(
    index: &Path,
    root: &Path,
    opts: &OrganizeOptions,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<OrganizePlan> {
    let rows = load(index)?;
    let units = units(&rows);
    let loc = &opts.location;

    // Place per unit: the photo's, else the video's (FR-47).
    let mut unit_place: Vec<Option<Place>> = units
        .iter()
        .map(|u| u.files.iter().find_map(|&i| rows[i].place.clone()))
        .collect();
    let mut inferred_units: HashSet<usize> = HashSet::new();
    if opts.infer_location {
        let window = i64::from(loc.infer_window_minutes) * 60;
        for (u, p) in infer(&rows, &units, &unit_place, window) {
            unit_place[u] = Some(p);
            inferred_units.insert(u);
        }
    }

    let mut plan = Plan::empty(
        root,
        PlanKind::Organize,
        match opts.transfer {
            Transfer::Copy => OpMode::Copy,
            Transfer::Move => OpMode::Move,
        },
    );
    plan.dest = Some(opts.dest.clone());
    let mut stats = OrganizeStats::default();
    let mut taken: HashSet<String> = HashSet::new();
    let mut moves: Vec<Op> = Vec::new();
    let mut skips: Vec<Op> = Vec::new();
    let mut dirs: BTreeMap<PathBuf, ()> = BTreeMap::new();
    let mut next_id = 1u64;
    let dest_dev = existing_ancestor(&opts.dest).and_then(|a| {
        use std::os::unix::fs::MetadataExt;
        a.metadata().ok().map(|m| m.dev() as i64)
    });

    for (ui, unit) in units.iter().enumerate() {
        if ui % 1000 == 0 {
            cancel.check()?;
            sink.progress(CoreEvent::Progress {
                task: TaskKind::Plan,
                done: ui as u64,
                total: Some(units.len() as u64),
                bytes: 0,
                current: None,
            });
        }
        let primary = &rows[unit.files[0]];
        let place = unit_place[ui].as_ref().map(|p| guarded(p, loc));
        let inferred = inferred_units.contains(&ui);
        stats.files += unit.files.len();
        if place.is_some() {
            stats.with_place += unit.files.len();
            if inferred {
                stats.inferred += unit.files.len();
            }
        }
        // Date: what the file recorded, else its modification time.
        let (date, fallback) = match primary.taken_at.and_then(ymd) {
            Some(d) => (Some(d), false),
            None => (
                ymd(primary.mtime_ns.div_euclid(1_000_000_000)).filter(|_| primary.mtime_ns > 0),
                true,
            ),
        };
        let base = Facts {
            name: String::new(),
            parent: primary
                .parent
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            kind: primary.kind,
            format: primary.format.clone(),
            camera_make: primary.make.clone(),
            camera_model: primary.model.clone(),
            date,
            place,
        };

        // Where each half goes, before any " (n)" suffix.
        struct Item {
            row: usize,
            folder: PathBuf,
            name: String,
        }
        let mut items: Vec<Item> = Vec::new();
        let (mut undated, mut no_location) = (false, false);
        for &fi in &unit.files {
            let r = &rows[fi];
            if r.cloud_only {
                skips.push(Op::Skip {
                    id: 0,
                    path: r.path.clone(),
                    why: SkipReason::Dataless,
                });
                continue;
            }
            let mut f = base.clone();
            f.name = r
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rendered = opts.template.render(&f, &loc.no_location_dir);
            undated |= rendered.undated;
            no_location |= rendered.no_location;
            let (name, dirs_) = rendered
                .segments
                .split_last()
                .map_or_else(|| (sanitize(&f.name), &[][..]), |(n, d)| (n.clone(), d));
            let mut folder = opts.dest.clone();
            folder.extend(dirs_);
            items.push(Item {
                row: fi,
                folder,
                name,
            });
        }
        if items.is_empty() {
            continue;
        }
        if undated {
            plan.summary.undated += items.len();
        }
        if no_location {
            plan.summary.no_location += items.len();
        }
        if fallback && date.is_some() && opts.template.uses_date() {
            plan.summary.date_fallback += items.len();
        }

        // Files already in place (or already there with the same content) are skipped.
        let mut todo: Vec<&Item> = Vec::new();
        for it in &items {
            let r = &rows[it.row];
            let dest = it.folder.join(&it.name);
            let key = norm_key(&dest);
            let already = norm_key(&r.path) == key
                || (!taken.contains(&key)
                    && dest.symlink_metadata().is_ok()
                    && same_content(&r.path, &dest, r.size));
            if already {
                taken.insert(key);
                skips.push(Op::Skip {
                    id: 0,
                    path: r.path.clone(),
                    why: SkipReason::AlreadyThere,
                });
            } else {
                todo.push(it);
            }
        }
        if todo.is_empty() {
            continue;
        }

        // The smallest suffix at which every remaining half has a free name (halves share it).
        let mut suffix = 0u32;
        let chosen: Vec<PathBuf> = loop {
            let cand: Vec<PathBuf> = todo
                .iter()
                .map(|it| it.folder.join(with_suffix(&it.name, suffix)))
                .collect();
            let mut keys: HashSet<String> = HashSet::new();
            let free = cand.iter().all(|c| {
                let k = norm_key(c);
                !taken.contains(&k) && c.symlink_metadata().is_err() && keys.insert(k)
            });
            if free {
                break cand;
            }
            suffix += 1;
        };
        for (it, dst) in todo.iter().zip(chosen) {
            let r = &rows[it.row];
            taken.insert(norm_key(&dst));
            if let Some(parent) = dst.parent() {
                let mut p = Some(parent);
                while let Some(d) = p {
                    if d.exists() {
                        break;
                    }
                    dirs.insert(d.to_path_buf(), ());
                    p = d.parent();
                }
            }
            let pair = (unit.files.len() == 2).then_some(ui as u32);
            let expect = Fingerprint {
                size: r.size,
                mtime_ns: r.mtime_ns,
                hash: None,
            };
            if opts.transfer == Transfer::Move && dest_dev.is_some_and(|d| d != r.dev) {
                plan.summary.cross_volume += 1;
            }
            if inferred {
                plan.summary.inferred += 1;
            }
            plan.summary.bytes += r.size;
            moves.push(match opts.transfer {
                Transfer::Move => Op::Move {
                    id: 0,
                    src: r.path.clone(),
                    dst,
                    expect,
                    inferred,
                    pair,
                },
                Transfer::Copy => Op::Copy {
                    id: 0,
                    src: r.path.clone(),
                    dst,
                    expect,
                    inferred,
                    pair,
                },
            });
        }
        if unit.files.len() == 2 {
            plan.summary.live_pairs += 1;
        }
    }

    // Folders first (shallowest first), then the transfers, then the skips.
    let mut mk: Vec<PathBuf> = dirs.into_keys().collect();
    mk.sort_by_key(|p| p.components().count());
    for path in mk {
        plan.ops.push(Op::MkDir { id: 0, path });
    }
    plan.ops.extend(moves);
    plan.summary.skipped = skips.len();
    plan.ops.extend(skips);
    for op in &mut plan.ops {
        let id = match op {
            Op::Move { id, .. }
            | Op::Copy { id, .. }
            | Op::Trash { id, .. }
            | Op::Delete { id, .. }
            | Op::MkDir { id, .. }
            | Op::Skip { id, .. } => id,
        };
        *id = next_id;
        next_id += 1;
    }
    plan.summary.ops = plan.ops.len();
    if plan.summary.cross_volume > 0 {
        plan.warnings.push(Warning {
            message: format!(
                "{} files are on another volume: each is copied, checked, then removed from the source",
                plan.summary.cross_volume
            ),
        });
    }
    if plan.summary.date_fallback > 0 {
        plan.warnings.push(Warning {
            message: format!(
                "{} files have no recorded date; their modification date is used",
                plan.summary.date_fallback
            ),
        });
    }
    Ok(OrganizePlan { plan, stats })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn row(
        name: &str,
        folder: &str,
        t: Option<i64>,
        cam: Option<&str>,
        place: Option<&str>,
    ) -> Row {
        Row {
            path: PathBuf::from(folder).join(name),
            parent: PathBuf::from(folder),
            size: 1,
            mtime_ns: 1,
            dev: 1,
            kind: MediaKind::Image,
            format: None,
            cloud_only: false,
            taken_at: t,
            make: cam.map(|_| "Sony".to_string()),
            model: cam.map(str::to_string),
            place: place.map(|c| Place {
                city: c.into(),
                admin1: None,
                admin2: None,
                country_code: "IN".into(),
                country: "India".into(),
                dist_km: 1.0,
            }),
            pair_id: None,
        }
    }

    fn run(rows: &[Row], window: i64) -> HashMap<usize, String> {
        let u: Vec<Unit> = (0..rows.len()).map(|i| Unit { files: vec![i] }).collect();
        let places: Vec<Option<Place>> = rows.iter().map(|r| r.place.clone()).collect();
        infer(rows, &u, &places, window)
            .into_iter()
            .map(|(k, v)| (k, v.city))
            .collect()
    }

    #[test]
    fn a_file_borrows_the_place_of_the_nearest_geotagged_file_of_its_camera() {
        let rows = vec![
            row("gps1.jpg", "/a", Some(1000), Some("A7"), Some("Panaji")),
            row("gps2.jpg", "/a", Some(9000), Some("A7"), Some("Margao")),
            row("plain.jpg", "/a", Some(8000), Some("A7"), None),
            row("far.jpg", "/a", Some(100_000), Some("A7"), None),
        ];
        let got = run(&rows, 7200);
        assert_eq!(
            got.get(&2).map(String::as_str),
            Some("Margao"),
            "nearest in time"
        );
        assert!(!got.contains_key(&3), "outside the window");
        assert!(
            !got.contains_key(&0),
            "files with a place are never touched"
        );
    }

    #[test]
    fn another_camera_does_not_lend_a_place_unless_this_one_has_none_at_all() {
        let rows = vec![
            row(
                "phone.jpg",
                "/trip",
                Some(1000),
                Some("iPhone"),
                Some("Panaji"),
            ),
            row("dslr.jpg", "/trip", Some(1500), Some("A7"), None),
            row("elsewhere.jpg", "/other", Some(1500), Some("A7"), None),
        ];
        let got = run(&rows, 7200);
        // The DSLR has no geotagged file of its own, so the same folder may lend one.
        assert_eq!(got.get(&1).map(String::as_str), Some("Panaji"));
        assert!(!got.contains_key(&2), "a different folder lends nothing");
    }

    #[test]
    fn a_camera_with_its_own_geotagged_files_ignores_the_folder() {
        let rows = vec![
            row(
                "phone.jpg",
                "/trip",
                Some(1000),
                Some("iPhone"),
                Some("Panaji"),
            ),
            row("a7_gps.jpg", "/x", Some(500_000), Some("A7"), Some("Paris")),
            row("a7.jpg", "/trip", Some(1200), Some("A7"), None),
        ];
        assert!(run(&rows, 7200).is_empty());
    }

    #[test]
    fn files_without_a_date_get_nothing() {
        let rows = vec![
            row("g.jpg", "/a", Some(1000), Some("A7"), Some("Panaji")),
            row("nodate.jpg", "/a", None, Some("A7"), None),
        ];
        assert!(run(&rows, 7200).is_empty());
    }
}
