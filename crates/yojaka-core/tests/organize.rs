#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use common::TestEnv;
use fixturegen::Spec;
use yojaka_core::Config;
use yojaka_core::organize::{OrganizeOptions, Template, Transfer};
use yojaka_core::plan::{Op, Plan, SkipReason};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink};

fn engine(env: &TestEnv) -> Engine {
    Engine::new(env.paths.clone(), Default::default())
}

fn scan(e: &Engine, root: &Path) {
    let o = ScanOptions {
        root: root.to_path_buf(),
        metadata: true,
        ..Default::default()
    };
    e.scan(&o, &EventSink::null(), &CancelToken::new()).unwrap();
}

fn opts(dest: &Path, template: &str, transfer: Transfer, infer: bool) -> OrganizeOptions {
    OrganizeOptions {
        dest: dest.to_path_buf(),
        template: Template::parse(template).unwrap(),
        transfer,
        infer_location: infer,
        location: Config::default().location,
        skip_identical: false,
    }
}

fn plan_for(e: &Engine, root: &Path, o: &OrganizeOptions) -> yojaka_core::organize::OrganizePlan {
    e.organize(root, false, o, &EventSink::null(), &CancelToken::new())
        .unwrap()
}

fn fixture() -> (TestEnv, PathBuf) {
    let env = TestEnv::with_spec(Spec {
        seed: 21,
        files: 300,
        noise: 20,
        videos: 0.15,
        live_photos: 16,
        ..Default::default()
    });
    let root = env.tree.clone().unwrap().canonicalize().unwrap();
    (env, root)
}

fn transfers(plan: &Plan) -> Vec<(&PathBuf, &PathBuf)> {
    plan.ops
        .iter()
        .filter_map(|o| match o {
            Op::Move { src, dst, .. } | Op::Copy { src, dst, .. } => Some((src, dst)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_file_ends_up_in_exactly_one_new_place_and_nothing_is_overwritten() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("sorted");
    let p = plan_for(
        &e,
        &root,
        &opts(&dest, "{year}/{month:02}/{name}", Transfer::Copy, false),
    )
    .plan;

    let t = transfers(&p);
    assert!(t.len() > 200, "{} transfers", t.len());
    let mut dsts = HashSet::new();
    let mut srcs = HashSet::new();
    for (src, dst) in &t {
        assert!(dst.starts_with(&dest), "{}", dst.display());
        assert!(
            dsts.insert(dst.to_string_lossy().to_lowercase()),
            "two files go to {}",
            dst.display()
        );
        assert!(srcs.insert(*src), "{} planned twice", src.display());
        assert!(!dst.exists(), "{} already exists", dst.display());
    }
    // Every folder is created before anything goes into it.
    let mut made: HashSet<&PathBuf> = HashSet::new();
    for op in &p.ops {
        match op {
            Op::MkDir { path, .. } => {
                made.insert(path);
            }
            Op::Copy { dst, .. } => {
                let parent = dst.parent().unwrap().to_path_buf();
                assert!(
                    made.contains(&parent) || parent.exists(),
                    "{}",
                    parent.display()
                );
            }
            _ => {}
        }
    }
    assert_eq!(p.dest.as_deref(), Some(dest.as_path()));
}

#[test]
fn name_clashes_get_a_suffix_and_live_photo_halves_stay_together() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    // One folder for everything: every name clash must be resolved.
    let dest = env.tmp.path().join("flat");
    let p = plan_for(&e, &root, &opts(&dest, "{name}", Transfer::Copy, false)).plan;
    let t = transfers(&p);
    let names: HashSet<String> = t
        .iter()
        .map(|(_, d)| d.file_name().unwrap().to_string_lossy().to_lowercase())
        .collect();
    assert_eq!(names.len(), t.len(), "all names are distinct");
    assert!(
        t.iter().any(|(_, d)| d.to_string_lossy().contains(" (1)")),
        "the fixture has clashing names"
    );

    // Pairs: same folder and same stem.
    let by_src: HashMap<&PathBuf, &PathBuf> = t.iter().map(|(s, d)| (*s, *d)).collect();
    let man = env.manifest.as_ref().unwrap();
    let mut pairs: HashMap<u32, Vec<&PathBuf>> = HashMap::new();
    for f in man.files.iter().filter(|f| f.live_pair.is_some()) {
        let src = root.join(&f.path);
        if let Some(d) = by_src.get(&src) {
            pairs.entry(f.live_pair.unwrap()).or_default().push(d);
        }
    }
    assert!(!pairs.is_empty());
    for (id, halves) in pairs {
        if halves.len() == 2 {
            assert_eq!(halves[0].parent(), halves[1].parent(), "pair {id}");
            assert_eq!(halves[0].file_stem(), halves[1].file_stem(), "pair {id}");
        }
    }
}

#[test]
fn a_place_template_puts_geotagged_files_in_their_country_and_the_rest_in_no_location() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("places");
    let o = opts(
        &dest,
        "{country_code}/{city}/{year}/{name}",
        Transfer::Copy,
        false,
    );
    let p = plan_for(&e, &root, &o).plan;
    let t: HashMap<PathBuf, PathBuf> = transfers(&p)
        .into_iter()
        .map(|(s, d)| (s.clone(), d.clone()))
        .collect();
    let man = env.manifest.as_ref().unwrap();
    let (mut with, mut without) = (0, 0);
    for f in man.files.iter().filter(|f| f.should_index && !f.corrupt) {
        let Some(d) = t.get(&root.join(&f.path)) else {
            continue;
        };
        let first = d.strip_prefix(&dest).unwrap().components().next().unwrap();
        let first = first.as_os_str().to_string_lossy();
        match &f.gps {
            Some(g) => {
                with += 1;
                assert_eq!(first, g.expect_cc, "{}", f.path);
            }
            None if f.kind == "image" && f.format.as_deref() == Some("jpeg") => {
                without += 1;
                assert_eq!(first, "No Location", "{}", f.path);
            }
            None => {}
        }
    }
    assert!(with > 20 && without > 20, "{with} with, {without} without");
    assert!(p.summary.no_location > 0);
}

#[test]
fn files_already_in_place_or_already_at_the_destination_are_skipped() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    std::fs::create_dir_all(&root).unwrap();
    let data = [0xFFu8, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4, 5, 6, 7, 8];
    std::fs::write(root.join("a.jpg"), data).unwrap();
    std::fs::write(root.join("b.jpg"), [data.as_slice(), &[9]].concat()).unwrap();
    let root = root.canonicalize().unwrap();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    // `a.jpg` is already there with the same bytes; `b.jpg` clashes with different bytes.
    std::fs::write(dest.join("a.jpg"), data).unwrap();
    std::fs::write(dest.join("b.jpg"), b"other bytes").unwrap();
    let p = plan_for(&e, &root, &opts(&dest, "{name}", Transfer::Copy, false)).plan;
    let skipped: Vec<_> = p
        .ops
        .iter()
        .filter(|o| {
            matches!(
                o,
                Op::Skip {
                    why: SkipReason::AlreadyThere,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(skipped.len(), 1);
    let t = transfers(&p);
    assert_eq!(t.len(), 1);
    assert_eq!(
        t[0].1,
        &dest.join("b (1).jpg"),
        "never over the existing file"
    );
    // Organizing a folder into itself: nothing to do.
    let same = plan_for(&e, &root, &opts(&root, "{name}", Transfer::Move, false)).plan;
    assert!(transfers(&same).is_empty());
}

#[test]
fn case_and_unicode_differences_still_count_as_the_same_name() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    for d in ["x", "y", "z"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let img = |n: u8| [0xFFu8, 0xD8, 0xFF, 0xE0, n, n, n];
    std::fs::write(root.join("x/Caf\u{e9}.jpg"), img(1)).unwrap();
    std::fs::write(root.join("y/Cafe\u{301}.jpg"), img(2)).unwrap(); // decomposed
    std::fs::write(root.join("z/CAF\u{c9}.JPG"), img(3)).unwrap();
    let root = root.canonicalize().unwrap();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("out");
    let p = plan_for(&e, &root, &opts(&dest, "{name}", Transfer::Copy, false)).plan;
    let t = transfers(&p);
    assert_eq!(t.len(), 3);
    let keys: HashSet<String> = t
        .iter()
        .map(|(_, d)| {
            use unicode_normalization::UnicodeNormalization;
            d.to_string_lossy().nfc().collect::<String>().to_lowercase()
        })
        .collect();
    assert_eq!(keys.len(), 3, "three different destinations");
}

#[test]
fn a_protected_destination_is_refused_and_nothing_is_planned() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    let o = opts(
        Path::new("/System/yojaka-test"),
        "{name}",
        Transfer::Copy,
        false,
    );
    let err = e
        .organize(&root, false, &o, &EventSink::null(), &CancelToken::new())
        .unwrap_err();
    assert!(
        matches!(err, yojaka_core::YojakaError::Protected { .. }),
        "{err}"
    );
}

#[test]
fn the_plan_is_only_a_proposal() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    let before = std::fs::read_dir(&root).unwrap().count();
    let dest = env.tmp.path().join("never-created");
    plan_for(
        &e,
        &root,
        &opts(&dest, "{year}/{name}", Transfer::Move, false),
    );
    assert!(!dest.exists());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), before);
}

// ---- Actions (plan 10): identical files once, copies never inside the folder, many names ----

fn with_identical(mut o: OrganizeOptions) -> OrganizeOptions {
    o.skip_identical = true;
    o
}

fn skipped_identical(plan: &Plan) -> Vec<&PathBuf> {
    plan.ops
        .iter()
        .filter_map(|o| match o {
            Op::Skip {
                path,
                why: SkipReason::Identical,
                ..
            } => Some(path),
            _ => None,
        })
        .collect()
}

fn set_mtime(p: &Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn identical_files_go_once_and_the_others_stay_where_they_are() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    for d in ["a", "b", "c", "d"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let same = [0xFFu8, 0xD8, 0xFF, 0xE0, 7, 7, 7, 7, 7, 7, 7, 7];
    std::fs::write(root.join("a/x.jpg"), same).unwrap();
    std::fs::write(root.join("b/x.jpg"), same).unwrap();
    std::fs::write(root.join("c/y.jpg"), same).unwrap();
    std::fs::write(root.join("d/z.jpg"), [0xFFu8, 0xD8, 0xFF, 0xE0, 1, 2, 3]).unwrap();
    // The keep rules (oldest modified first) pick c/y.jpg as the one that goes.
    set_mtime(&root.join("a/x.jpg"), 1_600_000_000);
    set_mtime(&root.join("b/x.jpg"), 1_600_000_000);
    set_mtime(&root.join("c/y.jpg"), 1_500_000_000);
    let root = root.canonicalize().unwrap();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("out");

    let p = plan_for(
        &e,
        &root,
        &with_identical(opts(&dest, "{kind}/{name}", Transfer::Copy, false)),
    )
    .plan;
    let mut t: Vec<(PathBuf, PathBuf)> = transfers(&p)
        .into_iter()
        .map(|(s, d)| (s.clone(), d.clone()))
        .collect();
    t.sort();
    assert_eq!(
        t,
        vec![
            (root.join("c/y.jpg"), dest.join("Photos/y.jpg")),
            (root.join("d/z.jpg"), dest.join("Photos/z.jpg")),
        ]
    );
    let mut left: Vec<&PathBuf> = skipped_identical(&p);
    left.sort();
    assert_eq!(left, vec![&root.join("a/x.jpg"), &root.join("b/x.jpg")]);
    assert_eq!(p.summary.identical, 2);
    assert_eq!(p.summary.identical_bytes, 2 * same.len() as u64);

    // Without the option every file goes, as before.
    let all = plan_for(
        &e,
        &root,
        &opts(&dest, "{kind}/{name}", Transfer::Copy, false),
    )
    .plan;
    assert_eq!(transfers(&all).len(), 4);
    assert!(skipped_identical(&all).is_empty());
}

#[test]
fn a_live_photo_is_left_out_only_as_a_whole_pair_and_only_for_an_identical_pair() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("out");
    // Two Live Photo pairs from a plain plan.
    let p = plan_for(&e, &root, &opts(&dest, "{name}", Transfer::Copy, false)).plan;
    let mut pairs: HashMap<u32, Vec<PathBuf>> = HashMap::new();
    for o in &p.ops {
        if let Op::Copy {
            src,
            pair: Some(id),
            ..
        } = o
        {
            pairs.entry(*id).or_default().push(src.clone());
        }
    }
    let mut ids: Vec<u32> = pairs.keys().copied().collect();
    ids.sort_unstable();
    let pairs: HashMap<u32, Vec<PathBuf>> = pairs
        .into_iter()
        .map(|(k, mut v)| {
            v.sort();
            (k, v)
        })
        .collect();
    assert!(ids.len() >= 2, "the fixture has Live Photos");
    let is_video = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mov"));
    // An identical copy of the first pair (both halves), and of the second pair's photo only.
    let copy_dir = root.join("copied pair");
    let lone_dir = root.join("lone photo");
    std::fs::create_dir_all(&copy_dir).unwrap();
    std::fs::create_dir_all(&lone_dir).unwrap();
    let first = &pairs[&ids[0]];
    for f in first {
        std::fs::copy(f, copy_dir.join(f.file_name().unwrap())).unwrap();
    }
    let photo = pairs[&ids[1]].iter().find(|f| !is_video(f)).unwrap();
    let lone = lone_dir.join(photo.file_name().unwrap());
    std::fs::copy(photo, &lone).unwrap();
    scan(&e, &root);

    let p = plan_for(
        &e,
        &root,
        &with_identical(opts(&dest, "{name}", Transfer::Copy, false)),
    )
    .plan;
    let left: HashSet<&PathBuf> = skipped_identical(&p).into_iter().collect();
    let copied: Vec<PathBuf> = first
        .iter()
        .map(|f| copy_dir.join(f.file_name().unwrap()))
        .collect();
    let left_of = |files: &[PathBuf]| files.iter().filter(|f| left.contains(f)).count();
    // Exactly one of the two identical pairs stays, as a whole: never one half.
    assert_eq!(first.len(), 2);
    let (a, b) = (left_of(first), left_of(&copied));
    assert!(
        (a, b) == (2, 0) || (a, b) == (0, 2),
        "{a} and {b} halves stay"
    );
    // A lone photo never matches the photo of a pair: it goes.
    let sources: HashSet<&PathBuf> = transfers(&p).into_iter().map(|(s, _)| s).collect();
    assert!(sources.contains(&lone));
    assert!(!left.contains(&lone));
    // Every pair in the plan goes or stays as a whole.
    for o in &p.ops {
        if let Op::Copy {
            src, pair: Some(_), ..
        } = o
        {
            assert!(!left.contains(src), "half of a pair both goes and stays");
        }
    }
    let second = &pairs[&ids[1]];
    assert!(matches!(left_of(second), 0 | 2));
}

#[test]
fn copies_into_the_scanned_folder_are_refused_but_moves_there_are_not() {
    let (env, root) = fixture();
    let e = engine(&env);
    scan(&e, &root);
    for dest in [root.clone(), root.join("sorted")] {
        let err = e
            .organize(
                &root,
                false,
                &opts(&dest, "{year}/{name}", Transfer::Copy, false),
                &EventSink::null(),
                &CancelToken::new(),
            )
            .unwrap_err();
        assert!(
            matches!(err, yojaka_core::YojakaError::Refused(_)),
            "{}: {err}",
            dest.display()
        );
    }
    // Tidying a folder in place by moving is still allowed.
    let p = plan_for(
        &e,
        &root,
        &opts(&root.join("sorted"), "{year}/{name}", Transfer::Move, false),
    )
    .plan;
    assert!(!transfers(&p).is_empty());
}

#[test]
fn many_files_with_one_name_are_numbered_in_order() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    const N: u16 = 1500;
    for i in 0..N {
        let d = root.join(format!("{i:04}"));
        std::fs::create_dir_all(&d).unwrap();
        let [lo, hi] = i.to_le_bytes();
        std::fs::write(d.join("a.jpg"), [0xFFu8, 0xD8, 0xFF, 0xE0, lo, hi]).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let e = engine(&env);
    scan(&e, &root);
    let dest = env.tmp.path().join("out");
    let started = std::time::Instant::now();
    let p = plan_for(&e, &root, &opts(&dest, "{name}", Transfer::Copy, false)).plan;
    eprintln!("planned {N} same-name files in {:?}", started.elapsed());
    let names: HashSet<String> = transfers(&p)
        .iter()
        .map(|(_, d)| d.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let want: HashSet<String> = std::iter::once("a.jpg".to_string())
        .chain((1..N).map(|n| format!("a ({n}).jpg")))
        .collect();
    assert_eq!(names, want);
}

#[test]
fn a_folder_counts_as_opened_before_once_it_has_saved_data() {
    let (env, root) = fixture();
    let e = engine(&env);
    assert!(
        !e.has_index(&root, false),
        "new folder: the app opens on Actions"
    );
    scan(&e, &root);
    assert!(
        e.has_index(&root, false),
        "scanned: the app opens on the Dashboard"
    );
    assert!(!e.has_index(Path::new("/System"), false));
}
