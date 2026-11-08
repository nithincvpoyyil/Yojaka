#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeSet, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use common::TestEnv;
use proptest::prelude::*;
use yojaka_core::dupes::{DupFile, DupGroup, DupItem, DupReport, Tier, parse_rules};
use yojaka_core::hash::ContentHash;
use yojaka_core::model::{FileId, MediaKind};
use yojaka_core::plan::{Op, Plan, PlanKind};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink, YojakaError};

fn engine(env: &TestEnv) -> Engine {
    Engine::new(env.paths.clone(), Default::default())
}

fn scan(e: &Engine, root: &Path, metadata: bool) {
    let o = ScanOptions {
        root: root.to_path_buf(),
        metadata,
        ..Default::default()
    };
    e.scan(&o, &EventSink::null(), &CancelToken::new()).unwrap();
}

fn dupes(e: &Engine, root: &Path) -> DupReport {
    e.dupes(root, false, &EventSink::null(), &CancelToken::new())
        .unwrap()
}

fn open_index(e: &Engine, root: &Path) -> yojaka_core::index::Index {
    let root = root.canonicalize().unwrap();
    yojaka_core::index::Index::open(&e.index_path(&root, false)).unwrap()
}

/// A file the scanner takes for a JPEG; the rest is `seed`-dependent filler.
fn jpeg_bytes(seed: u8, len: usize) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
    v.extend((0..len).map(|i| (i as u8).wrapping_mul(seed).wrapping_add(seed)));
    v
}

fn mov_bytes(seed: u8, len: usize) -> Vec<u8> {
    let mut v = vec![0, 0, 0, 0x14];
    v.extend_from_slice(b"ftypqt  ");
    v.extend_from_slice(&[0; 8]);
    v.extend((0..len).map(|i| (i as u8).wrapping_mul(seed).wrapping_add(seed)));
    v
}

fn inode(p: &Path) -> (u64, u64) {
    let m = std::fs::metadata(p).unwrap();
    (m.dev(), m.ino())
}

#[test]
fn groups_match_the_fixture_manifest_exactly() {
    let env = TestEnv::with_fixture(11, 400);
    let e = engine(&env);
    let tree = env.tree.clone().unwrap();
    scan(&e, &tree, true);
    let report = dupes(&e, &tree);
    let m = env.manifest.as_ref().unwrap();

    // Expected: the manifest's byte-identical groups, with hard-link names collapsed to one file
    // and Live Photo halves left out (those are judged as pairs, tested separately).
    let live: HashSet<&str> = m
        .files
        .iter()
        .filter(|f| f.live_pair.is_some())
        .map(|f| f.path.as_str())
        .collect();
    let expected: BTreeSet<BTreeSet<(u64, u64)>> = m
        .expected
        .exact_groups
        .iter()
        .map(|g| {
            g.iter()
                .filter(|p| !live.contains(p.as_str()))
                .map(|p| inode(&tree.join(p)))
                .collect::<BTreeSet<_>>()
        })
        .filter(|g| g.len() > 1)
        .collect();
    assert!(!expected.is_empty(), "the fixture must contain duplicates");

    let actual: BTreeSet<BTreeSet<(u64, u64)>> = report
        .groups
        .iter()
        .filter(|g| g.members.iter().all(|i| !i.is_pair()))
        .map(|g| {
            g.members
                .iter()
                .map(|i| inode(&i.primary().path))
                .collect::<BTreeSet<_>>()
        })
        .collect();
    assert_eq!(actual, expected, "no missing groups and no false positives");

    // Each group has distinct inodes: a hard link is never its own duplicate.
    for g in &report.groups {
        let inodes: HashSet<_> = g
            .members
            .iter()
            .flat_map(|i| &i.files)
            .map(|f| inode(&f.path))
            .collect();
        assert_eq!(
            inodes.len(),
            g.members.iter().map(|i| i.files.len()).sum::<usize>()
        );
        assert_eq!(g.tier, Tier::Exact);
    }
    assert!(report.issues.is_empty(), "{:?}", report.issues);
}

#[test]
fn only_candidates_are_read_and_a_second_run_reads_nothing() {
    let env = TestEnv::with_fixture(12, 300);
    let e = engine(&env);
    let tree = env.tree.clone().unwrap();
    scan(&e, &tree, true);

    let first = dupes(&e, &tree);
    let s = &first.stats;
    assert!(s.candidate_files > 0);
    assert!(s.partial_hashed > 0 && s.full_hashed > 0);
    // Bytes read: whole files for the full stage, two 16 KiB chunks at most for the partial stage.
    assert!(
        s.bytes_read <= s.candidate_bytes + s.partial_hashed * 32 * 1024,
        "{s:?}"
    );

    // Files with a unique size have no hash at all: they were never opened.
    let idx = open_index(&e, &tree);
    let touched_loners: i64 = idx
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM file f WHERE (partial_hash IS NOT NULL OR full_hash IS NOT NULL) \
             AND (SELECT COUNT(DISTINCT device_id||':'||inode) FROM file g WHERE g.size_bytes=f.size_bytes) = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(touched_loners, 0);
    let total: i64 = idx
        .connection()
        .query_row("SELECT COUNT(*) FROM file", [], |r| r.get(0))
        .unwrap();
    assert!((s.candidate_files as i64) < total);

    // Hashes are cached, so the second run reads nothing and finds the same thing.
    let second = dupes(&e, &tree);
    assert_eq!(second.stats.bytes_read, 0);
    assert_eq!(second.stats.partial_hashed + second.stats.full_hashed, 0);
    let paths = |r: &DupReport| -> BTreeSet<Vec<PathBuf>> {
        r.groups
            .iter()
            .map(|g| g.members.iter().map(|i| i.primary().path.clone()).collect())
            .collect()
    };
    assert_eq!(paths(&first), paths(&second));
}

#[test]
fn hard_links_are_never_grouped_together() {
    let env = TestEnv::new();
    let dir = env.tmp.path().join("pics");
    std::fs::create_dir_all(&dir).unwrap();
    let data = jpeg_bytes(3, 5000);
    std::fs::write(dir.join("a.jpg"), &data).unwrap();
    std::fs::hard_link(dir.join("a.jpg"), dir.join("b.jpg")).unwrap();
    std::fs::write(dir.join("other.jpg"), jpeg_bytes(9, 5000)).unwrap();
    let e = engine(&env);
    scan(&e, &dir, false);
    let r = dupes(&e, &dir);
    assert!(
        r.groups.is_empty(),
        "two names of one file are not duplicates"
    );
    assert_eq!(r.stats.hardlinks_skipped, 1);

    // A real copy joins them: one group of two, not three.
    std::fs::write(dir.join("c.jpg"), &data).unwrap();
    scan(&e, &dir, false);
    let r = dupes(&e, &dir);
    assert_eq!(r.groups.len(), 1);
    assert_eq!(r.groups[0].members.len(), 2);
}

#[test]
fn same_size_and_same_ends_but_different_middle_is_not_a_duplicate() {
    let env = TestEnv::new();
    let dir = env.tmp.path().join("pics");
    std::fs::create_dir_all(&dir).unwrap();
    let a = jpeg_bytes(5, 200_000);
    let mut b = a.clone();
    b[100_000] ^= 0xFF;
    std::fs::write(dir.join("a.jpg"), &a).unwrap();
    std::fs::write(dir.join("b.jpg"), &b).unwrap();
    let e = engine(&env);
    scan(&e, &dir, false);
    let r = dupes(&e, &dir);
    assert!(r.groups.is_empty());
    assert_eq!(
        r.stats.full_hashed, 2,
        "the partial hash cannot tell them apart"
    );
}

#[test]
fn a_file_changed_since_the_scan_is_reported_not_trusted() {
    let env = TestEnv::new();
    let dir = env.tmp.path().join("pics");
    std::fs::create_dir_all(&dir).unwrap();
    let data = jpeg_bytes(4, 3000);
    std::fs::write(dir.join("a.jpg"), &data).unwrap();
    std::fs::write(dir.join("b.jpg"), &data).unwrap();
    let e = engine(&env);
    scan(&e, &dir, false);
    // b grows after the scan.
    let mut grown = data.clone();
    grown.push(1);
    std::fs::write(dir.join("b.jpg"), grown).unwrap();
    let r = dupes(&e, &dir);
    assert!(r.groups.is_empty());
    assert_eq!(r.issues.len(), 1);
    assert!(r.issues[0].path.ends_with("b.jpg"));
}

#[test]
fn cancelling_stops_and_reports_cancelled() {
    let env = TestEnv::with_fixture(13, 200);
    let e = engine(&env);
    let tree = env.tree.clone().unwrap();
    scan(&e, &tree, false);
    let cancel = CancelToken::new();
    cancel.cancel();
    let err = e
        .dupes(&tree, false, &EventSink::null(), &cancel)
        .unwrap_err();
    assert!(matches!(err, YojakaError::Cancelled));
}

#[test]
fn dupes_before_a_scan_is_a_clear_error() {
    let env = TestEnv::new();
    let dir = env.tmp.path().join("pics");
    std::fs::create_dir_all(&dir).unwrap();
    let e = engine(&env);
    let err = e
        .dupes(&dir, false, &EventSink::null(), &CancelToken::new())
        .unwrap_err();
    assert!(matches!(err, YojakaError::NoIndex(_)));
}

/// Writes IMG_n.jpg + IMG_n.mov into `dir` and marks them as one Live Photo in the index.
fn live_pair(dir: &Path, n: u8, photo: &[u8], video: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("IMG_{n}.jpg")), photo).unwrap();
    std::fs::write(dir.join(format!("IMG_{n}.mov")), video).unwrap();
}

fn mark_pair(e: &Engine, root: &Path, dir: &Path, n: u8, pair_id: i64) {
    let idx = open_index(e, root);
    for ext in ["jpg", "mov"] {
        let path = dir.canonicalize().unwrap().join(format!("IMG_{n}.{ext}"));
        use std::os::unix::ffi::OsStrExt;
        let id: i64 = idx
            .connection()
            .query_row(
                "SELECT id FROM file WHERE full_path = ?1",
                [path.as_os_str().as_bytes()],
                |r| r.get(0),
            )
            .unwrap();
        idx.connection()
            .execute(
                "INSERT INTO meta(file_id, taken_at_src, pair_id) VALUES (?1, 'none', ?2)",
                rusqlite::params![id, pair_id],
            )
            .unwrap();
    }
}

#[test]
fn a_duplicated_live_photo_is_one_group_of_pairs() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    let (photo, video) = (jpeg_bytes(6, 4000), mov_bytes(6, 9000));
    live_pair(&root.join("a"), 1, &photo, &video);
    live_pair(&root.join("b"), 1, &photo, &video);
    let e = engine(&env);
    scan(&e, &root, false);
    mark_pair(&e, &root, &root.join("a"), 1, 100);
    mark_pair(&e, &root, &root.join("b"), 1, 200);

    let r = dupes(&e, &root);
    assert_eq!(
        r.groups.len(),
        1,
        "one group of pairs, not an image group and a video group"
    );
    let g = &r.groups[0];
    assert_eq!(g.members.len(), 2);
    for item in &g.members {
        assert!(item.is_pair());
        assert_eq!(item.files[0].kind, MediaKind::Image);
        assert_eq!(item.files[1].kind, MediaKind::Video);
    }
    assert_eq!(g.potential_reclaim(), (photo.len() + video.len()) as u64);

    // The plan trashes both halves of exactly one pair.
    let mut g = g.clone();
    g.apply_rules(&parse_rules::<&str>(&[]).unwrap());
    let plan = Plan::from_dupes(&root, &[g]);
    assert_eq!(plan.ops.len(), 2);
    assert_eq!(plan.summary.live_pairs, 1);
    let dirs: HashSet<_> = plan
        .ops
        .iter()
        .map(|o| match o {
            Op::Trash { path, .. } => path.parent().unwrap().to_path_buf(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(dirs.len(), 1, "both halves come from the same copy");
}

#[test]
fn a_pair_is_not_matched_with_a_lone_image_or_a_pair_with_another_video() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    let photo = jpeg_bytes(7, 4000);
    live_pair(&root.join("a"), 1, &photo, &mov_bytes(7, 9000));
    // Same photo, different video.
    live_pair(&root.join("b"), 1, &photo, &mov_bytes(8, 9000));
    // Same photo, no video at all.
    std::fs::write(root.join("lone.jpg"), &photo).unwrap();
    let e = engine(&env);
    scan(&e, &root, false);
    mark_pair(&e, &root, &root.join("a"), 1, 100);
    mark_pair(&e, &root, &root.join("b"), 1, 200);
    let r = dupes(&e, &root);
    assert!(
        r.groups.is_empty(),
        "deleting any of these would lose a video or a photo: {:?}",
        r.groups
    );
}

// ---- plan safety properties ----

fn file(id: i64, path: String, size: u64, kind: MediaKind) -> DupFile {
    DupFile {
        id: FileId(id),
        path: PathBuf::from(path),
        size,
        mtime_ns: id,
        kind,
        taken_at: None,
        width: None,
        height: None,
        duration_ms: None,
        bitrate: None,
        hash: ContentHash([id as u8; 32]),
    }
}

fn arb_group(gi: u32) -> impl Strategy<Value = DupGroup> {
    (2usize..6, any::<bool>(), 1u64..10_000).prop_map(move |(n, pairs, size)| {
        let members = (0..n)
            .map(|i| {
                let base = i64::from(gi) * 100 + i as i64 * 2;
                let mut files = vec![file(
                    base,
                    format!("/p/{gi}/{i}/IMG.jpg"),
                    size,
                    MediaKind::Image,
                )];
                if pairs {
                    files.push(file(
                        base + 1,
                        format!("/p/{gi}/{i}/IMG.mov"),
                        size * 3,
                        MediaKind::Video,
                    ));
                }
                DupItem { files }
            })
            .collect();
        DupGroup::new(gi + 1, Tier::Exact, members)
    })
}

#[derive(Debug, Clone)]
enum Action {
    Toggle(usize, usize),
    KeepOnly(usize, usize),
    KeepAll(usize),
    Rule(u8),
    MarkEverything(usize),
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0usize..4, 0usize..6).prop_map(|(g, i)| Action::Toggle(g, i)),
        (0usize..4, 0usize..6).prop_map(|(g, i)| Action::KeepOnly(g, i)),
        (0usize..4).prop_map(Action::KeepAll),
        (0u8..4).prop_map(Action::Rule),
        // Bypasses the API on purpose: the plan builder must still refuse.
        (0usize..4).prop_map(Action::MarkEverything),
    ]
}

proptest! {
    #[test]
    fn no_plan_deletes_a_whole_group_or_splits_a_pair(
        mut groups in proptest::collection::vec(0u32..4, 1..4)
            .prop_flat_map(|gs| gs.into_iter().enumerate()
                .map(|(i, _)| arb_group(i as u32)).collect::<Vec<_>>()),
        actions in proptest::collection::vec(arb_action(), 0..30),
    ) {
        let rules = [
            "oldest-modified", "newest-modified", "shortest-path", "prefer-path:/p/1",
        ];
        for a in actions {
            match a {
                Action::Toggle(g, i) => if let Some(g) = groups.get_mut(g) {
                    let was_valid = g.keeper().is_some();
                    g.toggle(i);
                    if was_valid {
                        prop_assert!(g.keeper().is_some(), "toggle may never mark the last copy");
                    }
                },
                Action::KeepOnly(g, i) => if let Some(g) = groups.get_mut(g) {
                    let n = g.members.len();
                    g.set_keeper(i % n);
                },
                Action::KeepAll(g) => if let Some(g) = groups.get_mut(g) { g.keep_all(); },
                Action::Rule(r) => {
                    let rules = parse_rules(&[rules[r as usize]]).unwrap();
                    for g in &mut groups { g.apply_rules(&rules); prop_assert!(g.keeper().is_some()); }
                }
                Action::MarkEverything(g) => if let Some(g) = groups.get_mut(g) {
                    g.delete.iter_mut().for_each(|d| *d = true);
                },
            }
        }

        let plan = Plan::from_dupes(Path::new("/p"), &groups);
        prop_assert_eq!(plan.kind, PlanKind::Dedupe);
        let planned: HashSet<&Path> = plan.ops.iter().map(|o| match o {
            Op::Trash { path, .. } => path.as_path(),
            _ => panic!("a dedupe plan only trashes"),
        }).collect();
        prop_assert_eq!(planned.len(), plan.ops.len(), "no file is planned twice");

        for g in &groups {
            let kept_files: Vec<&DupFile> = g.members.iter().zip(&g.delete)
                .filter(|(_, d)| !**d).flat_map(|(m, _)| &m.files).collect();
            let gone = |f: &DupFile| planned.contains(f.path.as_path());
            if g.delete.iter().all(|d| *d) {
                // Illegal state forced in: nothing from that group may be planned.
                prop_assert!(g.members.iter().flat_map(|m| &m.files).all(|f| !gone(f)));
            } else {
                prop_assert!(!kept_files.is_empty());
                prop_assert!(kept_files.iter().all(|f| !gone(f)), "a kept file was planned");
            }
            // Pairs: both halves or neither.
            for m in &g.members {
                let n = m.files.iter().filter(|f| gone(f)).count();
                prop_assert!(n == 0 || n == m.files.len(), "a Live Photo pair was split");
            }
        }

        let reclaim: u64 = plan.ops.iter().map(|o| match o {
            Op::Trash { expect, .. } => expect.size,
            _ => 0,
        }).sum();
        prop_assert_eq!(plan.summary.bytes, reclaim);
        prop_assert_eq!(plan.summary.ops, plan.ops.len());
    }
}

#[test]
fn plan_round_trips_through_json_and_never_overwrites() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    let data = jpeg_bytes(2, 3000);
    for d in ["a", "b"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::write(root.join(d).join("x.jpg"), &data).unwrap();
    }
    let e = engine(&env);
    scan(&e, &root, false);
    let mut r = dupes(&e, &root);
    r.apply_rules(&parse_rules(&["shortest-path"]).unwrap());
    let plan = Plan::from_dupes(&root, &r.groups);
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(plan.summary.bytes, data.len() as u64);

    let out = env.tmp.path().join("plan.json");
    plan.save(&out).unwrap();
    assert_eq!(Plan::load(&out).unwrap(), plan);
    // Never overwrite a file.
    assert!(plan.save(&out).is_err());
    // The plan is only a proposal: both copies are still there.
    assert!(root.join("a/x.jpg").exists() && root.join("b/x.jpg").exists());
}
