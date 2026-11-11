#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::TestEnv;
use fixturegen::Spec;
use yojaka_core::Config;
use yojaka_core::exec::{self, ApplyOptions, DirTrash, apply, history};
use yojaka_core::organize::{OrganizeOptions, Template, Transfer};
use yojaka_core::plan::{Op, Plan};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink};

fn engine(env: &TestEnv) -> Engine {
    Engine::new(env.paths.clone(), Default::default())
}

fn tree_hash(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let md = std::fs::symlink_metadata(&p).unwrap();
            if md.is_dir() {
                stack.push(p);
            } else {
                // A symlink counts by where it points (it dangles once its target has moved).
                let bytes = if md.file_type().is_symlink() {
                    std::fs::read_link(&p)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                        .into_bytes()
                } else {
                    std::fs::read(&p).unwrap()
                };
                let h = blake3::hash(&bytes).to_hex().to_string();
                out.insert(p.strip_prefix(root).unwrap().to_path_buf(), h);
            }
        }
    }
    out
}

/// Every directory under `root` (so a left-over empty folder shows up).
#[allow(dead_code)]
fn dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.push(e.path().strip_prefix(root).unwrap().to_path_buf());
                stack.push(e.path());
            }
        }
    }
    out.sort();
    out
}

struct Lab {
    env: TestEnv,
    root: PathBuf,
    dest: PathBuf,
}

impl Lab {
    fn new() -> Self {
        let env = TestEnv::with_spec(Spec {
            seed: 31,
            files: 200,
            noise: 10,
            videos: 0.15,
            live_photos: 12,
            ..Default::default()
        });
        let root = env.tree.clone().unwrap().canonicalize().unwrap();
        let dest = env.tmp.path().join("sorted");
        let lab = Self { env, root, dest };
        let o = ScanOptions {
            root: lab.root.clone(),
            metadata: true,
            ..Default::default()
        };
        engine(&lab.env)
            .scan(&o, &EventSink::null(), &CancelToken::new())
            .unwrap();
        lab
    }

    fn plan(&self, template: &str, transfer: Transfer) -> Plan {
        let o = OrganizeOptions {
            dest: self.dest.clone(),
            template: Template::parse(template).unwrap(),
            transfer,
            infer_location: false,
            location: Config::default().location,
        };
        engine(&self.env)
            .organize(
                &self.root,
                false,
                &o,
                &EventSink::null(),
                &CancelToken::new(),
            )
            .unwrap()
            .plan
    }

    fn run(&self, plan: &Plan, opts: ApplyOptions) -> exec::ApplyReport {
        apply(
            plan,
            &self.env.paths,
            &DirTrash(self.env.tmp.path().join("trash")),
            opts,
            &EventSink::null(),
            &CancelToken::new(),
        )
        .unwrap()
    }
}

#[test]
fn copy_leaves_the_originals_and_writes_matching_copies() {
    let lab = Lab::new();
    let before = tree_hash(&lab.root);
    let plan = lab.plan("{year}/{month:02}/{name}", Transfer::Copy);
    let r = lab.run(&plan, ApplyOptions::default());
    assert!(r.failed.is_empty() && r.skipped.is_empty(), "{r:?}");
    assert!(r.copied > 150);
    assert_eq!(tree_hash(&lab.root), before, "originals untouched");

    // Every planned copy exists and matches its source.
    let mut n = 0;
    for op in &plan.ops {
        if let Op::Copy { src, dst, .. } = op {
            assert_eq!(std::fs::read(src).unwrap(), std::fs::read(dst).unwrap());
            n += 1;
        }
    }
    assert_eq!(n, r.copied);
}

#[test]
fn move_puts_every_file_at_its_new_place() {
    let lab = Lab::new();
    let before = tree_hash(&lab.root);
    let plan = lab.plan("{kind}/{year}/{name}", Transfer::Move);
    let r = lab.run(&plan, ApplyOptions::default());
    assert!(r.failed.is_empty() && r.skipped.is_empty(), "{r:?}");
    assert!(r.moved > 150 && r.created > 0);
    let moved: BTreeMap<_, _> = tree_hash(&lab.dest);
    assert_eq!(moved.len(), r.moved);
    // Nothing planned is left at its old place.
    for op in &plan.ops {
        if let Op::Move { src, dst, .. } = op {
            assert!(!src.exists() && dst.exists());
        }
    }
    assert_eq!(tree_hash(&lab.root).len() + moved.len(), before.len());
    assert_eq!(
        history::summarize(&lab.env.paths, plan.id).unwrap().status,
        history::RunStatus::Applied
    );
}

#[test]
fn a_file_changed_after_planning_is_skipped_and_never_overwrites_anything() {
    let lab = Lab::new();
    let plan = lab.plan("{name}", Transfer::Copy);
    let (src, dst) = plan
        .ops
        .iter()
        .find_map(|o| match o {
            Op::Copy { src, dst, .. } => Some((src.clone(), dst.clone())),
            _ => None,
        })
        .unwrap();
    std::fs::write(&src, b"edited since the plan").unwrap();
    // And something appears at another destination.
    let other = plan
        .ops
        .iter()
        .filter_map(|o| match o {
            Op::Copy { dst, .. } if *dst != dst.clone() || true => Some(dst.clone()),
            _ => None,
        })
        .nth(5)
        .unwrap();
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, b"user file in the way").unwrap();

    let r = lab.run(&plan, ApplyOptions::default());
    assert!(r.skipped.iter().any(|n| n.path == src), "{r:?}");
    assert!(!dst.exists());
    assert_eq!(std::fs::read(&other).unwrap(), b"user file in the way");
    assert!(r.skipped.len() >= 2);
}

#[test]
fn live_photo_halves_are_moved_together_or_not_at_all() {
    let lab = Lab::new();
    let plan = lab.plan("{kind}/{name}", Transfer::Move);
    // Find a pair and break one half.
    let mut by_pair: BTreeMap<u32, Vec<PathBuf>> = BTreeMap::new();
    for op in &plan.ops {
        if let Op::Move {
            src, pair: Some(p), ..
        } = op
        {
            by_pair.entry(*p).or_default().push(src.clone());
        }
    }
    let (_, halves) = by_pair
        .iter()
        .find(|(_, v)| v.len() == 2)
        .expect("a pair in the fixture");
    std::fs::write(&halves[0], b"changed").unwrap();
    let r = lab.run(&plan, ApplyOptions::default());
    for h in halves {
        assert!(h.exists(), "{} must not have moved", h.display());
    }
    assert!(
        r.skipped
            .iter()
            .filter(|n| halves.contains(&n.path))
            .count()
            == 2
    );
    // Every other pair did move, both halves.
    for (_, v) in by_pair
        .iter()
        .filter(|(_, v)| v.len() == 2 && v[0] != halves[0])
    {
        assert_eq!(v.iter().filter(|p| p.exists()).count() % 2, 0);
    }
}

#[test]
fn an_interrupted_move_resumes_and_finishes_every_file() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let lab = Lab::new();
    let before = tree_hash(&lab.root);
    let plan = lab.plan("{year}/{name}", Transfer::Move);
    // Stop partway: cancel after some files by cancelling from the progress events.
    let (sink, rx) = EventSink::channel(4096);
    let cancel = CancelToken::new();
    let stop = AtomicBool::new(false);
    let c2 = cancel.clone();
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| {
            apply(
                &plan,
                &lab.env.paths,
                &DirTrash(lab.env.tmp.path().join("trash")),
                ApplyOptions::default(),
                &sink,
                &cancel,
            )
            .unwrap()
        });
        // Cancel as soon as the first progress event arrives.
        // The sink outlives the worker here, so the channel never closes: wait with a timeout,
        // or a worker that finishes right after an event would leave this loop waiting forever.
        while !h.is_finished() {
            let Ok(ev) = rx.recv_timeout(std::time::Duration::from_millis(20)) else {
                continue;
            };
            if matches!(ev, yojaka_core::CoreEvent::Progress { done, .. } if done > 40)
                && !stop.swap(true, Ordering::SeqCst)
            {
                c2.cancel();
            }
        }
        h.join().unwrap()
    });
    assert!(r.cancelled || r.moved > 0);
    let partial = tree_hash(&lab.dest).len();
    assert!(partial > 0);

    let r2 = lab.run(
        &plan,
        ApplyOptions {
            resume: true,
            ..Default::default()
        },
    );
    assert!(r2.failed.is_empty() && r2.skipped.is_empty(), "{r2:?}");
    assert_eq!(r.moved + r2.moved, tree_hash(&lab.dest).len());
    assert_eq!(
        tree_hash(&lab.root).len() + tree_hash(&lab.dest).len(),
        before.len()
    );
}

#[test]
fn bad_organize_plans_are_refused_before_anything_runs() {
    let lab = Lab::new();
    let good = lab.plan("{name}", Transfer::Copy);

    let mut p = good.clone();
    if let Some(Op::Copy { dst, .. }) = p.ops.iter_mut().find(|o| matches!(o, Op::Copy { .. })) {
        *dst = lab.env.tmp.path().join("elsewhere/x.jpg");
    }
    assert!(
        apply(
            &p,
            &lab.env.paths,
            &DirTrash("/x".into()),
            ApplyOptions::default(),
            &EventSink::null(),
            &CancelToken::new()
        )
        .is_err()
    );

    let mut p = good.clone();
    p.dest = None;
    assert!(exec::check_plan(&p, ApplyOptions::default()).is_err());

    let mut p = good.clone();
    p.dest = Some(PathBuf::from("/System/Library/yojaka"));
    assert!(exec::check_plan(&p, ApplyOptions::default()).is_err());

    assert!(
        exec::check_plan(
            &good,
            ApplyOptions {
                permanent: true,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(history::list(&lab.env.paths).unwrap().is_empty());
    assert!(!lab.dest.exists());
}

/// Needs `hdiutil` and mounts a small disk image, so it only runs on request:
/// `cargo nextest run -p yojaka-core --run-ignored only -E 'test(cross_volume)'`.
#[test]
#[ignore = "mounts a disk image"]
fn cross_volume_move_copies_verifies_and_removes() {
    use std::process::Command;
    let mut lab = Lab::new();
    let img = lab.env.tmp.path().join("yojaka-xvol.dmg");
    let out = Command::new("hdiutil")
        .args([
            "create",
            "-size",
            "256m",
            "-fs",
            "APFS",
            "-volname",
            "YojakaXVol",
            "-attach",
        ])
        .arg(&img)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let device = text.split_whitespace().next().unwrap().to_string();
    let mount = PathBuf::from("/Volumes/YojakaXVol");
    lab.dest = mount.join("sorted");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let before = tree_hash(&lab.root);
        let plan = lab.plan("{year}/{name}", Transfer::Move);
        assert!(plan.summary.cross_volume > 0, "{:?}", plan.summary);
        let r = lab.run(&plan, ApplyOptions::default());
        assert!(r.failed.is_empty(), "{r:?}");
        assert!(r.moved > 0 && tree_hash(&lab.root).len() < before.len());
    }));
    let _ = Command::new("hdiutil")
        .args(["detach", &device, "-force"])
        .output();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

// ---- Phase 3 fault injection ----

/// Organizing into a folder that cannot be written to: no file is half done, nothing is lost, the
/// originals are untouched, and the run says every file failed.
#[test]
fn a_read_only_destination_fails_cleanly_and_touches_nothing() {
    use std::os::unix::fs::PermissionsExt;
    for transfer in [Transfer::Copy, Transfer::Move] {
        let lab = Lab::new();
        let before = tree_hash(&lab.root);
        std::fs::create_dir_all(&lab.dest).unwrap();
        let plan = lab.plan("{year}/{name}", transfer);
        std::fs::set_permissions(&lab.dest, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = apply(
            &plan,
            &lab.env.paths,
            &DirTrash(lab.env.tmp.path().join("trash")),
            ApplyOptions::default(),
            &EventSink::null(),
            &CancelToken::new(),
        );

        std::fs::set_permissions(&lab.dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        match result {
            // Refused as a whole: fine, as long as nothing happened.
            Err(_) => {}
            Ok(r) => {
                assert_eq!(r.copied + r.moved, 0, "{transfer:?}: {r:?}");
                assert!(!r.failed.is_empty(), "{transfer:?}: failures are reported");
            }
        }
        assert_eq!(
            tree_hash(&lab.root),
            before,
            "{transfer:?}: originals intact"
        );
        assert!(
            tree_hash(&lab.dest).is_empty(),
            "{transfer:?}: nothing was written into the destination"
        );
    }
}
