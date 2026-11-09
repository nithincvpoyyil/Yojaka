#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::TestEnv;
use yojaka_core::dupes::parse_rules;
use yojaka_core::exec::{
    self, ApplyOptions, DirTrash, RunStatus, Trasher, apply, history, undo::undo,
};
use yojaka_core::model::Fingerprint;
use yojaka_core::plan::{DupRef, Op, Plan};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink, YojakaError};

fn engine(env: &TestEnv) -> Engine {
    Engine::new(env.paths.clone(), Default::default())
}

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

/// Every file under `root` with its content hash: the "hash of the tree".
fn tree_hash(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let h = blake3::hash(&std::fs::read(&p).unwrap())
                    .to_hex()
                    .to_string();
                out.insert(p.strip_prefix(root).unwrap().to_path_buf(), h);
            }
        }
    }
    out
}

/// A tree with `groups` groups of three identical photos, plus some unique files.
struct Lab {
    env: TestEnv,
    root: PathBuf,
    trash: PathBuf,
}

impl Lab {
    fn new(groups: u8) -> Self {
        let env = TestEnv::new();
        let root = env.tmp.path().join("pics");
        for g in 1..=groups {
            let data = jpeg_bytes(g, 2000 + g as usize * 10);
            for d in ["a", "b", "c"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
                std::fs::write(root.join(d).join(format!("p{g}.jpg")), &data).unwrap();
            }
        }
        std::fs::write(root.join("a/unique.jpg"), jpeg_bytes(99, 777)).unwrap();
        let trash = env.tmp.path().join("trash");
        let root = root.canonicalize().unwrap();
        let lab = Self { env, root, trash };
        lab.scan();
        lab
    }

    fn scan(&self) {
        let o = ScanOptions {
            root: self.root.clone(),
            metadata: false,
            ..Default::default()
        };
        engine(&self.env)
            .scan(&o, &EventSink::null(), &CancelToken::new())
            .unwrap();
    }

    fn plan(&self) -> Plan {
        let mut r = engine(&self.env)
            .dupes(&self.root, false, &EventSink::null(), &CancelToken::new())
            .unwrap();
        r.apply_rules(&parse_rules(&["shortest-path"]).unwrap());
        Plan::from_dupes(&self.root, &r.groups)
    }

    fn trasher(&self) -> DirTrash {
        DirTrash(self.trash.clone())
    }

    fn run(&self, plan: &Plan, opts: ApplyOptions) -> yojaka_core::Result<exec::ApplyReport> {
        apply(
            plan,
            &self.env.paths,
            &self.trasher(),
            opts,
            &EventSink::null(),
            &CancelToken::new(),
        )
    }

    fn undo(&self, plan: &Plan) -> exec::UndoReport {
        undo(
            plan.id,
            &self.env.paths,
            &EventSink::null(),
            &CancelToken::new(),
        )
        .unwrap()
    }
}

#[test]
fn scan_dupes_apply_undo_gives_back_the_identical_tree() {
    let lab = Lab::new(5);
    let before = tree_hash(&lab.root);
    let plan = lab.plan();
    assert_eq!(plan.ops.len(), 10, "two extra copies in each of 5 groups");

    let report = lab.run(&plan, ApplyOptions::default()).unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(report.trashed, 10);
    assert_eq!(report.bytes, plan.summary.bytes);

    // One copy of every group is left, and the unique file is untouched.
    let after = tree_hash(&lab.root);
    assert_eq!(after.len(), before.len() - 10);
    assert!(after.contains_key(Path::new("a/unique.jpg")));
    for g in 1..=5 {
        assert!(after.contains_key(Path::new(&format!("a/p{g}.jpg"))));
    }
    assert_eq!(std::fs::read_dir(&lab.trash).unwrap().count(), 10);

    let u = lab.undo(&plan);
    assert!(u.is_clean(), "{u:?}");
    assert_eq!(u.restored, 10);
    assert_eq!(tree_hash(&lab.root), before);
    assert_eq!(std::fs::read_dir(&lab.trash).unwrap().count(), 0);

    // Undo again: nothing left to do.
    let again = lab.undo(&plan);
    assert_eq!(again.restored, 0);
    assert_eq!(again.already_undone, 10);
}

#[test]
fn a_file_changed_after_planning_is_skipped_and_reported() {
    let lab = Lab::new(2);
    let plan = lab.plan();
    let victim = plan
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Trash { path, .. } => Some(path.clone()),
            _ => None,
        })
        .unwrap();
    std::fs::write(&victim, b"edited by the user since").unwrap();

    let report = lab.run(&plan, ApplyOptions::default()).unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path, victim);
    assert!(report.skipped[0].reason.contains("changed"));
    assert_eq!(report.trashed, 3);
    assert_eq!(std::fs::read(&victim).unwrap(), b"edited by the user since");
}

#[test]
fn an_edit_that_keeps_size_and_mtime_is_still_caught_by_the_hash() {
    let lab = Lab::new(1);
    let plan = lab.plan();
    let (victim, expect) = plan
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Trash { path, expect, .. } => Some((path.clone(), expect.clone())),
            _ => None,
        })
        .unwrap();
    let mut bytes = std::fs::read(&victim).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&victim)
        .unwrap();
    use std::io::Write;
    (&f).write_all(&bytes).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(expect.mtime_ns as u64))
        .unwrap();
    drop(f);

    let report = lab.run(&plan, ApplyOptions::default()).unwrap();
    assert!(report.skipped.iter().any(|n| n.path == victim));
    assert!(victim.exists());
}

#[test]
fn if_the_kept_copy_is_gone_nothing_is_trashed() {
    let lab = Lab::new(1);
    let plan = lab.plan();
    std::fs::remove_file(lab.root.join("a/p1.jpg")).unwrap();
    let report = lab.run(&plan, ApplyOptions::default()).unwrap();
    assert_eq!(report.trashed, 0);
    assert_eq!(report.skipped.len(), 2);
    assert!(lab.root.join("b/p1.jpg").exists() && lab.root.join("c/p1.jpg").exists());
}

fn live_lab() -> (Lab, Plan) {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    let (photo, video) = (jpeg_bytes(6, 4000), mov_bytes(6, 9000));
    for d in ["a", "b"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::write(root.join(d).join("IMG_1.jpg"), &photo).unwrap();
        std::fs::write(root.join(d).join("IMG_1.mov"), &video).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let trash = env.tmp.path().join("trash");
    let lab = Lab { env, root, trash };
    lab.scan();
    let idx =
        yojaka_core::index::Index::open(&engine(&lab.env).index_path(&lab.root, false)).unwrap();
    use std::os::unix::ffi::OsStrExt;
    for (d, pair) in [("a", 100), ("b", 200)] {
        for ext in ["jpg", "mov"] {
            let p = lab.root.join(d).join(format!("IMG_1.{ext}"));
            let id: i64 = idx
                .connection()
                .query_row(
                    "SELECT id FROM file WHERE full_path = ?1",
                    [p.as_os_str().as_bytes()],
                    |r| r.get(0),
                )
                .unwrap();
            idx.connection()
                .execute(
                    "INSERT INTO meta(file_id, taken_at_src, pair_id) VALUES (?1, 'none', ?2)",
                    rusqlite::params![id, pair],
                )
                .unwrap();
        }
    }
    drop(idx);
    let plan = lab.plan();
    assert_eq!(plan.ops.len(), 2);
    (lab, plan)
}

#[test]
fn a_live_photo_pair_is_trashed_and_restored_together() {
    let (lab, plan) = live_lab();
    let before = tree_hash(&lab.root);
    let report = lab.run(&plan, ApplyOptions::default()).unwrap();
    assert_eq!(report.trashed, 2);
    assert_eq!(tree_hash(&lab.root).len(), 2);
    lab.undo(&plan);
    assert_eq!(tree_hash(&lab.root), before);
}

#[test]
fn if_one_half_of_a_pair_fails_validation_both_are_skipped() {
    let (lab, plan) = live_lab();
    let half = plan
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Trash { path, .. } if path.extension().is_some_and(|e| e == "mov") => {
                Some(path.clone())
            }
            _ => None,
        })
        .unwrap();
    std::fs::write(&half, b"changed").unwrap();
    let report = lab.run(&plan, ApplyOptions::default()).unwrap();
    assert_eq!(report.trashed, 0);
    assert_eq!(report.skipped.len(), 2);
    assert_eq!(tree_hash(&lab.root).len(), 4, "nothing moved");
}

/// Trashes, then "crashes" (panics) right after the Nth file has been moved but before the
/// journal says so.
#[derive(Debug)]
struct CrashAfter {
    inner: DirTrash,
    left: AtomicUsize,
}

impl Trasher for CrashAfter {
    fn trash(&self, path: &Path) -> yojaka_core::Result<PathBuf> {
        let to = self.inner.trash(path)?;
        if self.left.fetch_sub(1, Ordering::SeqCst) == 1 {
            panic!("simulated crash");
        }
        Ok(to)
    }
    fn find(&self, original: &Path, expect: &Fingerprint) -> Option<PathBuf> {
        self.inner.find(original, expect)
    }
}

#[test]
fn a_crash_mid_apply_resumes_and_undo_restores_everything() {
    let lab = Lab::new(4);
    let before = tree_hash(&lab.root);
    let plan = lab.plan();

    let crashy = CrashAfter {
        inner: lab.trasher(),
        left: AtomicUsize::new(3),
    };
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        apply(
            &plan,
            &lab.env.paths,
            &crashy,
            ApplyOptions::default(),
            &EventSink::null(),
            &CancelToken::new(),
        )
    }));
    assert!(crashed.is_err());
    assert_eq!(tree_hash(&lab.root).len(), before.len() - 3);

    // Starting again without --resume is refused.
    assert!(lab.run(&plan, ApplyOptions::default()).is_err());

    let summary = history::summarize(&lab.env.paths, plan.id).unwrap();
    assert_eq!(summary.status, RunStatus::Incomplete);

    let report = lab
        .run(
            &plan,
            ApplyOptions {
                resume: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(report.already_done, 2, "two were fully recorded");
    assert_eq!(tree_hash(&lab.root).len(), before.len() - 8);
    assert_eq!(
        history::summarize(&lab.env.paths, plan.id).unwrap().status,
        RunStatus::Applied
    );

    let u = lab.undo(&plan);
    assert!(u.is_clean(), "{u:?}");
    assert_eq!(u.restored, 8);
    assert_eq!(tree_hash(&lab.root), before);
    assert_eq!(
        history::summarize(&lab.env.paths, plan.id).unwrap().status,
        RunStatus::Undone
    );
}

#[test]
fn cancelling_stops_cleanly_and_resume_finishes() {
    let lab = Lab::new(3);
    let plan = lab.plan();
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = apply(
        &plan,
        &lab.env.paths,
        &lab.trasher(),
        ApplyOptions::default(),
        &EventSink::null(),
        &cancel,
    )
    .unwrap();
    assert!(r.cancelled);
    assert_eq!(r.trashed, 0);
    let r = lab
        .run(
            &plan,
            ApplyOptions {
                resume: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(r.trashed, 6);
}

#[test]
fn undo_never_overwrites_a_file_that_has_appeared_since() {
    let lab = Lab::new(1);
    let plan = lab.plan();
    lab.run(&plan, ApplyOptions::default()).unwrap();
    let victim = plan
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Trash { path, .. } => Some(path.clone()),
            _ => None,
        })
        .unwrap();
    std::fs::write(&victim, b"new file in the way").unwrap();
    let u = lab.undo(&plan);
    assert_eq!(u.restored, 1);
    assert_eq!(u.failed.len(), 1);
    assert_eq!(std::fs::read(&victim).unwrap(), b"new file in the way");
    assert_eq!(std::fs::read_dir(&lab.trash).unwrap().count(), 1);
}

#[test]
fn bad_plans_are_refused_before_anything_runs() {
    let lab = Lab::new(1);
    let good = lab.plan();

    // A path outside the plan's folder.
    let mut p = good.clone();
    if let Op::Trash { path, .. } = &mut p.ops[0] {
        *path = lab.env.tmp.path().join("elsewhere.jpg");
    }
    assert!(matches!(
        lab.run(&p, ApplyOptions::default()),
        Err(YojakaError::Internal(_))
    ));

    // `..` in a path.
    let mut p = good.clone();
    if let Op::Trash { path, .. } = &mut p.ops[0] {
        *path = lab.root.join("a/../b/p1.jpg");
    }
    assert!(lab.run(&p, ApplyOptions::default()).is_err());

    // A file that the plan also keeps.
    let mut p = good.clone();
    let kept: PathBuf = match &p.ops[0] {
        Op::Trash { reason, .. } => reason.kept.clone(),
        _ => unreachable!(),
    };
    if let Op::Trash { path, .. } = &mut p.ops[1] {
        *path = kept;
    }
    assert!(lab.run(&p, ApplyOptions::default()).is_err());

    // Permanent delete needs the flag.
    let mut p = good.clone();
    if let Op::Trash {
        id,
        path,
        expect,
        reason,
    } = p.ops[0].clone()
    {
        p.ops[0] = Op::Delete {
            id,
            path,
            expect,
            reason: DupRef { ..reason },
        };
    }
    assert!(lab.run(&p, ApplyOptions::default()).is_err());

    assert_eq!(
        tree_hash(&lab.root).len(),
        4,
        "no refused plan touched anything"
    );
    assert!(
        history::list(&lab.env.paths).unwrap().is_empty(),
        "and none left a journal"
    );
}

#[test]
fn permanent_delete_removes_the_file_and_cannot_be_undone() {
    let lab = Lab::new(1);
    let mut plan = lab.plan();
    plan.mode = yojaka_core::plan::OpMode::DeletePermanent;
    plan.ops = plan
        .ops
        .into_iter()
        .map(|o| match o {
            Op::Trash {
                id,
                path,
                expect,
                reason,
            } => Op::Delete {
                id,
                path,
                expect,
                reason,
            },
            o => o,
        })
        .collect();
    let r = lab
        .run(
            &plan,
            ApplyOptions {
                permanent: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(r.deleted, 2);
    assert!(!lab.trash.exists());
    let u = lab.undo(&plan);
    assert_eq!(u.restored, 0);
    assert_eq!(u.not_undoable.len(), 2);
}

#[test]
fn history_lists_runs_newest_first_and_finds_by_prefix() {
    let lab = Lab::new(2);
    let plan = lab.plan();
    lab.run(&plan, ApplyOptions::default()).unwrap();
    let all = history::list(&lab.env.paths).unwrap();
    assert_eq!(all.len(), 1);
    let s = &all[0];
    assert_eq!((s.ops, s.done, s.status), (4, 4, RunStatus::Applied));
    assert_eq!(s.bytes, plan.summary.bytes);
    let prefix = &plan.id.to_string()[..8];
    assert_eq!(
        history::find(&lab.env.paths, prefix).unwrap().plan_id,
        plan.id
    );
    assert!(history::find(&lab.env.paths, "zzzz").is_err());
}

/// Uses the real Trash, so it only runs on request:
/// `cargo nextest run -p yojaka-core --run-ignored only -E 'test(real_trash)'`.
/// It trashes one small temp file, then puts it back, leaving the Trash as it was.
#[test]
#[ignore = "touches the real Trash"]
fn real_trash_round_trip() {
    use yojaka_core::exec::SystemTrash;
    use yojaka_core::exec::trash::restore;
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("yojaka-trash-test.jpg");
    std::fs::write(&file, b"real trash round trip").unwrap();
    let to = SystemTrash.trash(&file).unwrap();
    assert!(!file.exists());
    assert!(to.exists(), "{} should be in the Trash", to.display());
    assert!(to.to_string_lossy().contains(".Trash"));
    restore(&to, &file).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"real trash round trip");
    assert!(!to.exists());
}
