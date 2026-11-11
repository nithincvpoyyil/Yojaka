//! The likely, name and similar duplicate tiers: what they group, what they must never do
//! (mark anything by rule, split a Live Photo, trust a stale index), and how well they do on the
//! fixture's look-alike sets.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeSet, HashMap};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use common::TestEnv;
use yojaka_core::dupes::{DupGroup, DupOptions, DupReport, Tier, parse_rules};
use yojaka_core::exec::{ApplyOptions, DirTrash, apply};
use yojaka_core::plan::{Op, Plan};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink, IssueKind};

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

fn find(e: &Engine, root: &Path, opts: DupOptions) -> DupReport {
    e.dupes(root, false, opts, &EventSink::null(), &CancelToken::new())
        .unwrap()
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

fn write(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    p
}

#[derive(Default, Clone, Copy)]
struct Meta {
    taken: Option<i64>,
    src: &'static str,
    camera: Option<&'static str>,
    w: Option<i64>,
    h: Option<i64>,
    duration_ms: Option<i64>,
    pair: Option<i64>,
}

/// A photo with a camera date, as the metadata pass would record it.
fn photo(taken: i64, camera: &'static str, w: i64, h: i64) -> Meta {
    Meta {
        taken: Some(taken),
        src: "exif",
        camera: Some(camera),
        w: Some(w),
        h: Some(h),
        ..Meta::default()
    }
}

fn clip(taken: i64, w: i64, h: i64, duration_ms: i64) -> Meta {
    Meta {
        taken: Some(taken),
        src: "container",
        w: Some(w),
        h: Some(h),
        duration_ms: Some(duration_ms),
        ..Meta::default()
    }
}

/// Records metadata for `rel` straight into the index (the files here are not real pictures).
fn set_meta(e: &Engine, root: &Path, rel: &str, m: Meta) {
    let root = root.canonicalize().unwrap();
    let idx = yojaka_core::index::Index::open(&e.index_path(&root, false)).unwrap();
    let path = root.join(rel);
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
            "INSERT OR REPLACE INTO meta(file_id, taken_at, taken_at_src, camera_model, width, \
             height, duration_ms, pair_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                m.taken,
                if m.src.is_empty() { "none" } else { m.src },
                m.camera,
                m.w,
                m.h,
                m.duration_ms,
                m.pair
            ],
        )
        .unwrap();
}

/// The file names in each group, sorted, for groups of `tier`.
fn names(r: &DupReport, tier: Tier) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = r
        .groups
        .iter()
        .filter(|g| g.tier == tier)
        .map(|g| {
            let mut v: Vec<String> = g
                .members
                .iter()
                .map(|m| {
                    m.primary()
                        .path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            v.sort();
            v
        })
        .collect();
    out.sort();
    out
}

fn strs(groups: &[&[&str]]) -> Vec<Vec<String>> {
    groups
        .iter()
        .map(|g| g.iter().map(|s| (*s).to_string()).collect())
        .collect()
}

#[test]
fn likely_photos_need_the_same_date_camera_and_pixels() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    for (i, name) in [
        "orig.jpg",
        "resaved.jpg",
        "other-camera.jpg",
        "smaller.jpg",
        "later.jpg",
        "no-exif-date.jpg",
    ]
    .iter()
    .enumerate()
    {
        write(&root, name, &jpeg_bytes(i as u8 + 1, 3000 + i * 100));
    }
    let e = engine(&env);
    scan(&e, &root, false);
    set_meta(&e, &root, "orig.jpg", photo(1000, "iPhone 15", 4000, 3000));
    set_meta(
        &e,
        &root,
        "resaved.jpg",
        photo(1000, "iPhone 15", 4000, 3000),
    );
    set_meta(
        &e,
        &root,
        "other-camera.jpg",
        photo(1000, "Pixel 8", 4000, 3000),
    );
    set_meta(
        &e,
        &root,
        "smaller.jpg",
        photo(1000, "iPhone 15", 2000, 1500),
    );
    set_meta(&e, &root, "later.jpg", photo(1001, "iPhone 15", 4000, 3000));
    // Same numbers, but the date did not come from the camera (EXIF).
    set_meta(
        &e,
        &root,
        "no-exif-date.jpg",
        Meta {
            src: "container",
            ..photo(1000, "iPhone 15", 4000, 3000)
        },
    );

    let r = find(&e, &root, DupOptions::all());
    assert_eq!(
        names(&r, Tier::Likely),
        strs(&[&["orig.jpg", "resaved.jpg"]])
    );
    assert!(names(&r, Tier::Exact).is_empty());
    assert_eq!(r.stats.likely_groups, 1);
    // The default search still reports the exact tier only.
    assert!(find(&e, &root, DupOptions::default()).groups.is_empty());
}

#[test]
fn likely_videos_allow_one_second_and_never_mix_with_photos() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    for (i, name) in ["a.mov", "export.mov", "long.mov", "small.mov"]
        .iter()
        .enumerate()
    {
        write(&root, name, &mov_bytes(i as u8 + 1, 5000 + i * 300));
    }
    write(&root, "still.jpg", &jpeg_bytes(9, 4000));
    let e = engine(&env);
    scan(&e, &root, false);
    set_meta(&e, &root, "a.mov", clip(5000, 1920, 1080, 10_000));
    set_meta(&e, &root, "export.mov", clip(5000, 1920, 1080, 10_900));
    set_meta(&e, &root, "long.mov", clip(5000, 1920, 1080, 11_500));
    set_meta(&e, &root, "small.mov", clip(5000, 1280, 720, 10_000));
    // A photo with the very same date and pixels is not a copy of a video.
    set_meta(&e, &root, "still.jpg", photo(5000, "", 1920, 1080));

    let r = find(&e, &root, DupOptions::only(Tier::Likely));
    assert_eq!(names(&r, Tier::Likely), strs(&[&["a.mov", "export.mov"]]));
}

#[test]
fn identical_files_are_left_to_the_exact_tier() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    let same = jpeg_bytes(3, 5000);
    write(&root, "a/IMG_1.jpg", &same);
    write(&root, "b/IMG_1 (1).jpg", &same);
    write(&root, "c/IMG_1 copy.jpg", &jpeg_bytes(4, 5100)); // same name, other bytes
    let e = engine(&env);
    scan(&e, &root, false);
    for rel in ["a/IMG_1.jpg", "b/IMG_1 (1).jpg"] {
        set_meta(&e, &root, rel, photo(77, "EOS R6", 6000, 4000));
    }

    let r = find(&e, &root, DupOptions::all());
    assert_eq!(
        names(&r, Tier::Exact),
        strs(&[&["IMG_1 (1).jpg", "IMG_1.jpg"]])
    );
    // The two identical files have the same date, camera and pixels, but that is not news.
    assert!(names(&r, Tier::Likely).is_empty());
    // In the name tier they count once, next to the third, different file.
    assert_eq!(
        names(&r, Tier::Name),
        strs(&[&["IMG_1 copy.jpg", "IMG_1.jpg"]])
    );
    // Asking for one tier only gives that tier, with the same exclusions.
    let only = find(&e, &root, DupOptions::only(Tier::Name));
    assert!(only.groups.iter().all(|g| g.tier == Tier::Name));
    assert_eq!(only.groups.len(), 1);
}

#[test]
fn name_tier_needs_nearly_the_same_size_and_the_same_kind() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    write(&root, "a/DSC_0042.jpg", &jpeg_bytes(1, 10_000));
    write(&root, "b/DSC_0042 (1).jpg", &jpeg_bytes(2, 10_400)); // 4% bigger
    write(&root, "c/DSC_0042 copy.jpg", &jpeg_bytes(3, 12_000)); // 20% bigger
    write(&root, "d/dsc_0042-1.JPG", &jpeg_bytes(4, 9_950));
    write(&root, "e/DSC_0042.mov", &mov_bytes(5, 10_000)); // a video, not a photo
    write(&root, "f/DSC_0043.jpg", &jpeg_bytes(6, 10_000)); // another counter
    // A second name for a/DSC_0042.jpg: the same file, not a copy.
    std::fs::create_dir_all(root.join("g")).unwrap();
    std::fs::hard_link(root.join("a/DSC_0042.jpg"), root.join("g/DSC_0042.jpg")).unwrap();
    let e = engine(&env);
    scan(&e, &root, false);

    let r = find(&e, &root, DupOptions::only(Tier::Name));
    assert_eq!(
        names(&r, Tier::Name),
        strs(&[&["DSC_0042 (1).jpg", "DSC_0042.jpg", "dsc_0042-1.JPG"]])
    );
}

#[test]
fn a_weaker_tier_does_not_repeat_a_group() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    write(&root, "a/IMG_7.jpg", &jpeg_bytes(1, 8000));
    write(&root, "b/IMG_7 (1).jpg", &jpeg_bytes(2, 8100));
    let e = engine(&env);
    scan(&e, &root, false);
    for rel in ["a/IMG_7.jpg", "b/IMG_7 (1).jpg"] {
        set_meta(&e, &root, rel, photo(42, "iPhone 15", 4032, 3024));
    }
    let r = find(&e, &root, DupOptions::all());
    assert_eq!(r.groups.len(), 1, "one pair of files is one group");
    assert_eq!(r.groups[0].tier, Tier::Likely);
    // On its own the name tier does find them.
    assert_eq!(
        find(&e, &root, DupOptions::only(Tier::Name)).groups.len(),
        1
    );
}

#[test]
fn a_changed_file_is_reported_and_left_out() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    write(&root, "a.jpg", &jpeg_bytes(1, 4000));
    let changed = write(&root, "b.jpg", &jpeg_bytes(2, 4100));
    let e = engine(&env);
    scan(&e, &root, false);
    for rel in ["a.jpg", "b.jpg"] {
        set_meta(&e, &root, rel, photo(9, "iPhone 15", 100, 100));
    }
    std::fs::write(&changed, jpeg_bytes(3, 9999)).unwrap();

    let r = find(&e, &root, DupOptions::all());
    assert!(r.groups.is_empty());
    assert_eq!(r.issues.len(), 1, "{:?}", r.issues);
    assert_eq!(r.issues[0].kind, IssueKind::Changed);
}

#[test]
fn a_live_photo_stays_whole_in_a_likely_group() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    write(&root, "live/IMG_1.jpg", &jpeg_bytes(1, 6000));
    write(&root, "live/IMG_1.mov", &mov_bytes(1, 9000));
    write(&root, "export/IMG_1_edited.jpg", &jpeg_bytes(2, 5000));
    let e = engine(&env);
    scan(&e, &root, false);
    let shot = photo(500, "iPhone 15", 4032, 3024);
    set_meta(
        &e,
        &root,
        "live/IMG_1.jpg",
        Meta {
            pair: Some(7),
            ..shot
        },
    );
    set_meta(
        &e,
        &root,
        "live/IMG_1.mov",
        Meta {
            pair: Some(7),
            ..clip(500, 1920, 1440, 2000)
        },
    );
    set_meta(&e, &root, "export/IMG_1_edited.jpg", shot);

    let mut r = find(&e, &root, DupOptions::all());
    assert_eq!(r.groups.len(), 1);
    let g = &mut r.groups[0];
    assert_eq!(g.tier, Tier::Likely);
    let pair = g.members.iter().position(|m| m.is_pair()).unwrap();
    assert_eq!(g.members[pair].files.len(), 2);
    assert_eq!(g.members[1 - pair].files.len(), 1);

    // Marking the Live Photo plans both halves, each naming the other.
    g.set_keeper(1 - pair);
    let plan = Plan::from_dupes(&root, &r.groups);
    assert_eq!(plan.ops.len(), 2);
    assert_eq!(plan.summary.live_pairs, 1);
    for op in &plan.ops {
        let Op::Trash { reason, .. } = op else {
            panic!("{op:?}");
        };
        assert!(reason.partner.is_some());
        assert_eq!(reason.tier, Tier::Likely);
    }
}

#[test]
fn rules_never_mark_a_group_that_is_not_identical() {
    let env = TestEnv::with_spec(fixturegen::Spec {
        files: 120,
        variants: 24,
        noise: 5,
        ..Default::default()
    });
    let tree = env.tree.clone().unwrap();
    let e = engine(&env);
    scan(&e, &tree, true);
    let mut r = find(&e, &tree, DupOptions::all().with_similar());
    assert!(r.stats.exact_groups > 0);
    assert!(r.stats.likely_groups > 0 && r.stats.similar_groups > 0);

    for rules in [
        "oldest-modified,shortest-path",
        "highest-resolution",
        "newest",
    ] {
        r.apply_rules(&parse_rules(&[rules]).unwrap());
        for g in &r.groups {
            if g.tier.is_exact() {
                assert_eq!(g.delete_count(), g.members.len() - 1);
            } else {
                assert_eq!(g.delete_count(), 0, "group {} ({:?})", g.id, g.tier);
            }
        }
    }
    // So a plan made from rules alone only ever removes identical copies.
    let plan = Plan::from_dupes(&tree, &r.groups);
    assert!(!plan.ops.is_empty());
    assert_eq!(plan.lookalike_ops(), 0);
    assert_eq!(plan.summary.groups as u64, r.stats.exact_groups);
}

/// Exact groups as sets of paths relative to the tree.
fn exact_sets(r: &DupReport, tree: &Path) -> BTreeSet<BTreeSet<String>> {
    let tree = tree.canonicalize().unwrap();
    r.groups
        .iter()
        .filter(|g| g.tier == Tier::Exact)
        .map(|g| {
            g.members
                .iter()
                .flat_map(|m| &m.files)
                .map(|f| {
                    f.path
                        .strip_prefix(&tree)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        })
        .collect()
}

#[test]
fn the_exact_tier_has_no_false_positives_with_every_tier_on() {
    let env = TestEnv::with_spec(fixturegen::Spec {
        files: 200,
        variants: 30,
        videos: 0.1,
        noise: 5,
        ..Default::default()
    });
    let tree = env.tree.clone().unwrap();
    let e = engine(&env);
    scan(&e, &tree, true);
    // What the exact tier finds on its own (checked against the manifest in `dupes.rs`).
    let want = exact_sets(&find(&e, &tree, DupOptions::default()), &tree);
    assert!(!want.is_empty());
    let all = find(&e, &tree, DupOptions::all().with_similar());
    assert_eq!(exact_sets(&all, &tree), want);
    assert!(
        all.groups.len() > want.len(),
        "the other tiers found something"
    );
    // No group of another tier is made only of identical files.
    for g in all.groups.iter().filter(|g| !g.tier.is_exact()) {
        let hashes: BTreeSet<_> = g.members.iter().map(|m| m.primary().hash).collect();
        assert!(
            hashes.len() > 1 || hashes.contains(&None),
            "group {} repeats an exact group",
            g.id
        );
    }
}

/// Unordered pairs of relative paths that a tier put in one group.
fn pairs(r: &DupReport, tier: Tier, tree: &Path) -> BTreeSet<(String, String)> {
    let tree = tree.canonicalize().unwrap();
    let mut out = BTreeSet::new();
    for g in r.groups.iter().filter(|g| g.tier == tier) {
        let paths: Vec<String> = g
            .members
            .iter()
            .map(|m| {
                m.primary()
                    .path
                    .strip_prefix(&tree)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        for (i, a) in paths.iter().enumerate() {
            for b in &paths[i + 1..] {
                out.insert((a.clone().min(b.clone()), a.clone().max(b.clone())));
            }
        }
    }
    out
}

/// Precision and recall of each tier on the fixture's look-alike sets (iteration 7 acceptance).
/// Run with `--no-capture` to print the table recorded in `docs/dupes-accuracy.md`.
#[test]
fn tiers_precision_and_recall_on_the_fixture() {
    let spec = fixturegen::Spec {
        files: 400,
        variants: 120,
        videos: 0.1,
        live_photos: 8,
        noise: 10,
        ..Default::default()
    };
    let env = TestEnv::with_spec(spec);
    let tree = env.tree.clone().unwrap();
    let manifest = env.manifest.as_ref().unwrap();
    let e = engine(&env);
    scan(&e, &tree, true);

    // What each look-alike is, and which set it belongs to.
    let label: HashMap<&str, &str> = manifest
        .files
        .iter()
        .filter_map(|f| Some((f.path.as_str(), f.variant.as_deref()?)))
        .collect();
    let mut set_of: HashMap<&str, usize> = HashMap::new();
    for (i, set) in manifest.expected.same_content.iter().enumerate() {
        for p in set {
            set_of.insert(p.as_str(), i);
        }
    }
    // Identical bytes are the same picture too (a Live Photo next to a lone copy of its photo
    // is shown by these tiers, because the exact tier never trades a pair for a single file).
    let bytes_of: HashMap<&str, u32> = manifest
        .files
        .iter()
        .filter_map(|f| Some((f.path.as_str(), f.dup_group?)))
        .collect();
    let same = |a: &str, b: &str| {
        let look_alike = matches!((set_of.get(a), set_of.get(b)), (Some(x), Some(y)) if x == y);
        let identical = matches!((bytes_of.get(a), bytes_of.get(b)), (Some(x), Some(y)) if x == y);
        look_alike || identical
    };
    // The copies each tier is meant to catch.
    let in_scope = |tier: Tier, copy: &str| match tier {
        Tier::Likely => matches!(copy, "reencoded" | "reexport"),
        Tier::Name => copy == "renamed",
        Tier::Similar => matches!(copy, "reencoded" | "resized" | "stripped" | "renamed"),
        Tier::Exact => false,
    };

    println!("tier     groups  pairs  correct  precision  in-scope found  recall");
    let mut results = HashMap::new();
    for tier in [Tier::Likely, Tier::Similar, Tier::Name] {
        let r = find(&e, &tree, DupOptions::only(tier));
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        let found = pairs(&r, tier, &tree);
        let correct = found.iter().filter(|(a, b)| same(a, b)).count();
        let mut wanted = 0;
        let mut hit = 0;
        for set in &manifest.expected.same_content {
            let base = set.iter().find(|p| label[p.as_str()] == "base").unwrap();
            for copy in set.iter().filter(|p| in_scope(tier, label[p.as_str()])) {
                wanted += 1;
                let key = (
                    base.clone().min(copy.clone()),
                    base.clone().max(copy.clone()),
                );
                hit += usize::from(found.contains(&key));
            }
        }
        let precision = correct as f64 / found.len().max(1) as f64;
        let recall = hit as f64 / wanted.max(1) as f64;
        println!(
            "{:<8} {:>6} {:>6} {:>8} {:>10.3} {:>8}/{:<6} {:>6.3}",
            tier.label(),
            r.groups.len(),
            found.len(),
            correct,
            precision,
            hit,
            wanted,
            recall
        );
        for (a, b) in found.iter().filter(|(a, b)| !same(a, b)) {
            println!("    not the same: {a}  |  {b}");
        }
        results.insert(tier, (precision, recall, wanted));
    }

    // Every copy a tier is meant to catch is caught.
    for tier in [Tier::Likely, Tier::Similar, Tier::Name] {
        let (_, recall, wanted) = results[&tier];
        assert!(wanted >= 20, "{tier:?}: the fixture has too few cases");
        assert!(recall >= 0.99, "{tier:?} recall {recall}");
    }
    // The likely tier is fooled by every burst decoy and the name tier by counter collisions;
    // that is why a person reviews them. Looking at the pictures is right far more often: its
    // only wrong pairs are among the tiny plain gradients of the base fixture, never among the
    // detailed pictures.
    assert!(
        results[&Tier::Similar].0 >= 0.85,
        "{:?}",
        results[&Tier::Similar]
    );
    assert!(
        results[&Tier::Likely].0 >= 0.6,
        "{:?}",
        results[&Tier::Likely]
    );
    assert!(results[&Tier::Name].0 >= 0.45, "{:?}", results[&Tier::Name]);
    let similar = pairs(
        &find(&e, &tree, DupOptions::only(Tier::Similar)),
        Tier::Similar,
        &tree,
    );
    for (a, b) in similar.iter().filter(|(a, b)| !same(a, b)) {
        assert!(
            !a.contains("PIC_") && !b.contains("PIC_"),
            "two different detailed pictures were matched: {a} | {b}"
        );
    }
}

fn save_picture(path: &Path, seed: u32, w: u32, h: u32) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (f64::from(x) / f64::from(w), f64::from(y) / f64::from(h));
        let s = f64::from(seed);
        let v =
            ((fx * (4.0 + s) + s).sin() + (fy * (3.0 + s * 0.5) + s * 1.3).cos()) * 60.0 + 128.0;
        let v = v.clamp(0.0, 255.0) as u8;
        image::Rgb([v, v.wrapping_add((seed * 40) as u8) / 2 + v / 2, 255 - v])
    });
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    img.save(path).unwrap();
}

#[test]
fn similar_finds_a_resized_copy_and_remembers_what_it_saw() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    save_picture(&root.join("a/big.png"), 1, 400, 300);
    save_picture(&root.join("b/small.jpg"), 1, 200, 150);
    save_picture(&root.join("c/other.png"), 2, 400, 300);
    // Same picture, but portrait: not a resized copy.
    save_picture(&root.join("d/tall.png"), 1, 300, 400);
    // Too plain to compare, and not a picture at all.
    image::RgbImage::from_pixel(64, 64, image::Rgb([200, 200, 200]))
        .save(root.join("flat.png"))
        .unwrap();
    write(&root, "broken.jpg", &jpeg_bytes(5, 3000));
    let e = engine(&env);
    scan(&e, &root, true);

    // Off unless asked for.
    assert!(find(&e, &root, DupOptions::all()).groups.is_empty());

    let r = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(names(&r, Tier::Similar), strs(&[&["big.png", "small.jpg"]]));
    assert_eq!(r.stats.pictures_hashed, 5);
    assert_eq!(
        r.stats.pictures_unusable, 2,
        "the flat one and the broken one"
    );
    assert!(r.issues.is_empty(), "{:?}", r.issues);

    // The second search decodes only the file that could not be decoded before.
    let again = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(again.stats.pictures_hashed, 0);
    assert_eq!(again.stats.pictures_cached, 5);
    assert_eq!(names(&again, Tier::Similar), names(&r, Tier::Similar));

    // A rescan after a change forgets the saved look of that file only.
    save_picture(&root.join("b/small.jpg"), 3, 200, 150);
    scan(&e, &root, true);
    let third = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(third.stats.pictures_hashed, 1);
    assert!(third.groups.is_empty());

    // Looks saved by an older way of measuring are all made again, once.
    let idx = yojaka_core::index::Index::open(&e.index_path(&root.canonicalize().unwrap(), false))
        .unwrap();
    idx.connection()
        .execute(
            "UPDATE meta_kv SET value = '1' WHERE key = 'picture_look_version'",
            [],
        )
        .unwrap();
    let fourth = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(fourth.stats.pictures_cached, 0);
    assert_eq!(fourth.stats.pictures_hashed, 5);
    let fifth = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(fifth.stats.pictures_hashed, 0);
}

#[test]
fn similar_compares_heic_and_avif_and_turns_sideways_photos_upright() {
    use fixturegen::pictures;
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    let heic = |img: &image::RgbImage, orientation: u16| {
        pictures::heic_from_jpeg(&pictures::jpeg(img, 90, orientation).unwrap())
            .expect("sips makes a HEIC")
    };
    // A portrait photo as an iPhone keeps it (stored sideways with a rotation flag), and the
    // upright JPEG a messaging app sends on.
    let portrait = pictures::detailed(41, 240, 320);
    write(
        &root,
        "iphone/IMG_0001.HEIC",
        &heic(&image::imageops::rotate270(&portrait), 6),
    );
    write(
        &root,
        "shared/IMG_0001.jpg",
        &pictures::jpeg(&portrait, 70, 1).unwrap(),
    );
    // A landscape photo, an AVIF copy and a smaller JPEG copy of it.
    let landscape = pictures::detailed(42, 320, 240);
    write(&root, "iphone/IMG_0002.HEIC", &heic(&landscape, 1));
    let avif = pictures::avif_from_jpeg(&pictures::jpeg(&landscape, 90, 1).unwrap())
        .expect("sips makes an AVIF");
    write(&root, "web/IMG_0002.avif", &avif);
    let small = image::imageops::resize(&landscape, 160, 120, image::imageops::Triangle);
    write(
        &root,
        "shared/small.jpg",
        &pictures::jpeg(&small, 80, 1).unwrap(),
    );
    // Another picture: matches nothing.
    write(
        &root,
        "iphone/IMG_0003.HEIC",
        &heic(&pictures::detailed(43, 320, 240), 1),
    );
    let e = engine(&env);
    scan(&e, &root, true);

    let r = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(
        names(&r, Tier::Similar),
        strs(&[
            &["IMG_0001.HEIC", "IMG_0001.jpg"],
            &["IMG_0002.HEIC", "IMG_0002.avif", "small.jpg"]
        ])
    );
    assert_eq!(r.stats.pictures_hashed, 6);
    assert_eq!(
        r.stats.pictures_unsupported, 0,
        "HEIC and AVIF are compared"
    );
    assert_eq!(r.stats.pictures_unusable, 0);
    assert!(r.issues.is_empty(), "{:?}", r.issues);
}

/// Real raw photos cannot be made here (ImageIO does not write them) and are too big for the
/// repository. Point `YOJAKA_RAW_SAMPLES` at a folder of raw files, each next to the JPEG the
/// camera saved with it (`DSC_0001.NEF` + `DSC_0001.JPG`). They are copied to a temporary
/// folder first. Run with:
/// `YOJAKA_RAW_SAMPLES=<dir> cargo nextest run -p yojaka-core --run-ignored only -E 'test(raw_photos)'`
#[test]
#[ignore = "needs real raw photos in YOJAKA_RAW_SAMPLES"]
fn raw_photos_match_the_jpeg_the_camera_saved_with_them() {
    let samples = PathBuf::from(std::env::var("YOJAKA_RAW_SAMPLES").expect("YOJAKA_RAW_SAMPLES"));
    let env = TestEnv::new();
    let root = env.tmp.path().join("raws");
    std::fs::create_dir_all(&root).unwrap();
    let mut want = Vec::new();
    for entry in std::fs::read_dir(&samples).unwrap() {
        let path = entry.unwrap().path();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        let is_raw = ext
            .as_deref()
            .and_then(yojaka_core::model::Format::from_extension)
            .is_some_and(|f| f.as_str().starts_with("raw-"));
        let jpeg = ["JPG", "jpg", "JPEG", "jpeg"]
            .iter()
            .map(|e| path.with_extension(e))
            .find(|p| p.exists());
        if let (true, Some(jpeg)) = (is_raw, jpeg) {
            for p in [&path, &jpeg] {
                std::fs::copy(p, root.join(p.file_name().unwrap())).unwrap();
            }
            let mut pair = vec![
                path.file_name().unwrap().to_string_lossy().into_owned(),
                jpeg.file_name().unwrap().to_string_lossy().into_owned(),
            ];
            pair.sort();
            want.push(pair);
        }
    }
    assert!(
        !want.is_empty(),
        "no raw + JPEG pairs in {}",
        samples.display()
    );
    want.sort();
    let e = engine(&env);
    scan(&e, &root, true);
    let r = find(&e, &root, DupOptions::only(Tier::Similar));
    assert_eq!(names(&r, Tier::Similar), want);
    assert_eq!(r.stats.pictures_unusable, 0);

    // The metadata pass read every raw file: size, date and camera, and the same proportions
    // as the camera's JPEG (not the size of a small thumbnail).
    let idx = yojaka_core::index::Index::open(&e.index_path(&root.canonicalize().unwrap(), false))
        .unwrap();
    let mut st = idx
        .connection()
        .prepare(
            "SELECT CAST(f.file_name AS TEXT), f.format, m.width, m.height, m.taken_at, \
             m.camera_model FROM file f LEFT JOIN meta m ON m.file_id = f.id",
        )
        .unwrap();
    type Row = (
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    );
    let rows: Vec<Row> = st
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let shape = |name: &str| {
        let row = rows.iter().find(|r| r.0 == name).unwrap();
        let (w, h) = (row.2.unwrap() as f64, row.3.unwrap() as f64);
        w.max(h) / w.min(h)
    };
    for pair in &want {
        let (raw, jpeg) = if rows
            .iter()
            .any(|r| r.0 == pair[0] && r.1.starts_with("raw-"))
        {
            (&pair[0], &pair[1])
        } else {
            (&pair[1], &pair[0])
        };
        let row = rows.iter().find(|r| &r.0 == raw).unwrap();
        assert!(
            row.2.is_some() && row.3.is_some() && row.4.is_some() && row.5.is_some(),
            "{row:?}"
        );
        assert!(
            (shape(raw) - shape(jpeg)).abs() <= 0.03 * shape(jpeg),
            "{raw}: {} against {}",
            shape(raw),
            shape(jpeg)
        );
    }
}

fn lab_with_a_likely_pair() -> (TestEnv, Engine, PathBuf, DupReport) {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    write(&root, "a/IMG_9.jpg", &jpeg_bytes(1, 7000));
    write(&root, "b/IMG_9_small.jpg", &jpeg_bytes(2, 3000));
    write(&root, "c/unrelated.jpg", &jpeg_bytes(3, 4000));
    let root = root.canonicalize().unwrap();
    let e = engine(&env);
    scan(&e, &root, false);
    for rel in ["a/IMG_9.jpg", "b/IMG_9_small.jpg"] {
        set_meta(&e, &root, rel, photo(123, "iPhone 15", 4032, 3024));
    }
    let r = find(&e, &root, DupOptions::all());
    assert_eq!(r.groups.len(), 1);
    (env, e, root, r)
}

fn keep(g: &mut DupGroup, name: &str) {
    let i = g
        .members
        .iter()
        .position(|m| m.primary().path.ends_with(name))
        .unwrap();
    g.set_keeper(i);
}

#[test]
fn a_reviewed_likely_group_is_trashed_and_can_be_put_back() {
    let (env, _e, root, mut r) = lab_with_a_likely_pair();
    keep(&mut r.groups[0], "a/IMG_9.jpg");
    let plan = Plan::from_dupes(&root, &r.groups);
    assert_eq!(plan.ops.len(), 1);
    assert_eq!(plan.lookalike_ops(), 1);
    let Op::Trash { reason, expect, .. } = &plan.ops[0] else {
        panic!();
    };
    // The kept copy is a different file, so its own size and date are recorded.
    assert_eq!(reason.kept_expect.as_ref().unwrap().size, 7004);
    assert_eq!(expect.size, 3004);

    // The plan survives a round trip through its file.
    let file = env.tmp.path().join("plan.json");
    plan.save(&file).unwrap();
    assert_eq!(Plan::load(&file).unwrap(), plan);

    let trash = DirTrash(env.tmp.path().join("trash"));
    let (sink, cancel) = (EventSink::null(), CancelToken::new());
    let report = apply(
        &plan,
        &env.paths,
        &trash,
        ApplyOptions::default(),
        &sink,
        &cancel,
    )
    .unwrap();
    assert_eq!(report.trashed, 1, "{:?}", report.skipped);
    assert!(!root.join("b/IMG_9_small.jpg").exists());
    assert!(root.join("a/IMG_9.jpg").exists());
}

#[test]
fn a_likely_removal_is_skipped_when_the_kept_copy_changed_or_vanished() {
    for vanish in [false, true] {
        let (env, _e, root, mut r) = lab_with_a_likely_pair();
        keep(&mut r.groups[0], "a/IMG_9.jpg");
        let plan = Plan::from_dupes(&root, &r.groups);
        if vanish {
            std::fs::remove_file(root.join("a/IMG_9.jpg")).unwrap();
        } else {
            std::fs::write(root.join("a/IMG_9.jpg"), jpeg_bytes(8, 7000)).unwrap();
        }
        let trash = DirTrash(env.tmp.path().join("trash"));
        let report = apply(
            &plan,
            &env.paths,
            &trash,
            ApplyOptions::default(),
            &EventSink::null(),
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(report.trashed, 0);
        assert_eq!(report.skipped.len(), 1);
        assert!(
            report.skipped[0].reason.contains("the copy to keep"),
            "{}",
            report.skipped[0].reason
        );
        assert!(
            root.join("b/IMG_9_small.jpg").exists(),
            "nothing may be lost"
        );
    }
}

#[test]
fn groups_that_share_a_file_never_remove_what_another_keeps() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("pics");
    // a and b: likely copies. b and c: the same name with a copy suffix.
    write(&root, "x/a.jpg", &jpeg_bytes(1, 5000));
    write(&root, "y/shot.jpg", &jpeg_bytes(2, 6000));
    write(&root, "z/shot copy.jpg", &jpeg_bytes(3, 6100));
    let root = root.canonicalize().unwrap();
    let e = engine(&env);
    scan(&e, &root, false);
    for rel in ["x/a.jpg", "y/shot.jpg"] {
        set_meta(&e, &root, rel, photo(1, "iPhone 15", 10, 10));
    }
    let mut r = find(&e, &root, DupOptions::all());
    assert_eq!(r.groups.len(), 2);
    let (likely, name) = (0, 1);
    assert_eq!(r.groups[likely].tier, Tier::Likely);
    assert_eq!(r.groups[name].tier, Tier::Name);

    // Keep a (remove shot.jpg), and in the other group keep shot.jpg (remove the copy): the
    // second group would be left with nothing, so it is dropped with a warning.
    keep(&mut r.groups[likely], "x/a.jpg");
    keep(&mut r.groups[name], "y/shot.jpg");
    let plan = Plan::from_dupes(&root, &r.groups);
    let removed: Vec<_> = plan
        .ops
        .iter()
        .map(|op| match op {
            Op::Trash { path, .. } => path.strip_prefix(&root).unwrap().to_path_buf(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(removed, vec![PathBuf::from("y/shot.jpg")]);
    assert_eq!(plan.warnings.len(), 1, "{:?}", plan.warnings);

    // Keep shot.jpg in both groups: it stays, the other two go, each once.
    keep(&mut r.groups[likely], "y/shot.jpg");
    let plan = Plan::from_dupes(&root, &r.groups);
    assert_eq!(plan.ops.len(), 2);
    assert!(plan.warnings.is_empty());

    // Remove shot.jpg in the first group; the second marks it again and keeps the copy.
    keep(&mut r.groups[likely], "x/a.jpg");
    keep(&mut r.groups[name], "z/shot copy.jpg");
    let plan = Plan::from_dupes(&root, &r.groups);
    assert_eq!(plan.ops.len(), 1, "a file is removed once");
    yojaka_core::exec::check_plan(&plan, ApplyOptions::default()).unwrap();
}
