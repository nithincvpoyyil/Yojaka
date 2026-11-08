#![allow(clippy::unwrap_used)]

mod common;

use common::TestEnv;
use yojaka_core::model::MediaKind;
use yojaka_core::scan::{ScanOptions, ScanStatus};
use yojaka_core::{CancelToken, Engine, EventSink, YojakaError};

fn engine(env: &TestEnv) -> Engine {
    Engine::new(env.paths.clone(), Default::default())
}

fn opts(env: &TestEnv) -> ScanOptions {
    ScanOptions {
        root: env.tree.clone().unwrap(),
        ..Default::default()
    }
}

fn scan(e: &Engine, o: &ScanOptions) -> yojaka_core::scan::ScanSummary {
    e.scan(o, &EventSink::null(), &CancelToken::new()).unwrap()
}

fn index_count(e: &Engine, o: &ScanOptions, where_clause: &str) -> i64 {
    let root = o.root.canonicalize().unwrap();
    let idx = yojaka_core::index::Index::open(&e.index_path(&root, o.local_index)).unwrap();
    idx.connection()
        .query_row(
            &format!("SELECT COUNT(*) FROM file WHERE {where_clause}"),
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn first_scan_matches_the_manifest() {
    let env = TestEnv::with_fixture(5, 300);
    let e = engine(&env);
    let o = opts(&env);
    let m = env.manifest.as_ref().unwrap();

    let s = scan(&e, &o);
    assert_eq!(s.status, ScanStatus::Complete);
    assert_eq!(s.files_indexed as usize, m.expected.indexed_count);
    assert_eq!(s.new_or_changed as usize, m.expected.indexed_count);
    assert_eq!(s.unchanged, 0);
    assert_eq!(s.issues, 0, "protected/skip bait must not produce issues");

    // Hard links are counted once in the statistics.
    let hardlinks = m
        .files
        .iter()
        .filter(|f| f.should_index && f.hardlink_of.is_some())
        .count();
    assert!(hardlinks > 0);
    let stats = e.stats(&o.root, false).unwrap();
    assert_eq!(
        stats.photos.count as usize,
        m.expected.indexed_count - hardlinks
    );
    assert_eq!(stats.videos.count, 0);

    // Per-format counts match the manifest (minus hard-link duplicates).
    let mut expected = std::collections::BTreeMap::<String, u64>::new();
    for f in m
        .files
        .iter()
        .filter(|f| f.should_index && f.hardlink_of.is_none())
    {
        *expected.entry(f.format.clone().unwrap()).or_default() += 1;
    }
    let got: std::collections::BTreeMap<_, _> = stats
        .formats
        .iter()
        .map(|f| (f.format.clone(), f.count))
        .collect();
    assert_eq!(got, expected);
    // Sorted by most used.
    assert!(stats.formats.windows(2).all(|w| w[0].count >= w[1].count));

    // Skip rules: bait, symlinks and noise never reach the index.
    assert_eq!(
        index_count(
            &e,
            &o,
            "full_path LIKE '%photoslibrary%' OR full_path LIKE '%/.git/%'"
        ),
        0
    );
    assert_eq!(index_count(&e, &o, "file_name LIKE 'link%'"), 0);
    assert_eq!(
        index_count(&e, &o, "extension IN ('txt','pdf','json','md')"),
        0
    );
}

#[test]
fn rescan_is_incremental_and_prunes() {
    let env = TestEnv::with_fixture(6, 200);
    let e = engine(&env);
    let o = opts(&env);
    let m = env.manifest.as_ref().unwrap();
    let first = scan(&e, &o);

    let again = scan(&e, &o);
    assert_eq!(again.unchanged, first.files_indexed);
    assert_eq!(again.new_or_changed, 0);
    assert_eq!(again.pruned, 0);

    // Change one file, delete one, add one.
    let tree = env.tree.as_ref().unwrap();
    let plain: Vec<_> = m
        .files
        .iter()
        .filter(|f| {
            f.should_index && f.hardlink_of.is_none() && !f.corrupt && f.dup_group.is_none()
        })
        .take(3)
        .collect();
    let changed = tree.join(&plain[0].path);
    let mut bytes = std::fs::read(&changed).unwrap();
    bytes.extend_from_slice(b"extra");
    std::fs::write(&changed, bytes).unwrap();
    std::fs::remove_file(tree.join(&plain[1].path)).unwrap();
    std::fs::copy(tree.join(&plain[2].path), tree.join("brand_new.jpg")).unwrap();

    let third = scan(&e, &o);
    assert_eq!(third.new_or_changed, 2);
    assert_eq!(third.pruned, 1);
    assert_eq!(third.files_indexed, first.files_indexed + 1 - 1);
}

#[test]
fn cancelled_scan_is_partial_and_never_prunes() {
    let env = TestEnv::with_fixture(7, 100);
    let e = engine(&env);
    let o = opts(&env);
    let m = env.manifest.as_ref().unwrap();
    let first = scan(&e, &o);

    let victim = m
        .files
        .iter()
        .find(|f| f.should_index && f.hardlink_of.is_none())
        .unwrap();
    std::fs::remove_file(env.tree.as_ref().unwrap().join(&victim.path)).unwrap();

    let cancel = CancelToken::new();
    cancel.cancel();
    let s = e.scan(&o, &EventSink::null(), &cancel).unwrap();
    assert_eq!(s.status, ScanStatus::Partial);
    assert_eq!(s.pruned, 0);
    assert_eq!(index_count(&e, &o, "1=1") as u64, first.files_indexed);
    let stats = e.stats(&o.root, false).unwrap();
    assert_eq!(stats.last_scan.unwrap().status, "partial");
}

#[test]
fn kind_filter_and_sniff_all() {
    let env = TestEnv::with_fixture(8, 60);
    let e = engine(&env);
    let tree = env.tree.clone().unwrap();

    let videos_only = ScanOptions {
        kinds: vec![MediaKind::Video],
        ..opts(&env)
    };
    assert_eq!(scan(&e, &videos_only).files_indexed, 0);

    // A renamed JPEG: found only with --sniff-all.
    let jpeg = std::fs::read(
        tree.join(
            &env.manifest
                .as_ref()
                .unwrap()
                .files
                .iter()
                .find(|f| f.format.as_deref() == Some("jpeg") && !f.corrupt && f.should_index)
                .unwrap()
                .path,
        ),
    )
    .unwrap();
    std::fs::write(tree.join("no_extension"), &jpeg).unwrap();
    std::fs::write(tree.join("renamed.dat"), &jpeg).unwrap();
    let plain = scan(&e, &opts(&env));
    assert_eq!(
        index_count(&e, &opts(&env), "CAST(file_name AS TEXT)='no_extension'"),
        0
    );
    assert!(plain.files_indexed > 0);

    let sniffed = ScanOptions {
        sniff_all: true,
        ..opts(&env)
    };
    scan(&e, &sniffed);
    assert_eq!(
        index_count(
            &e,
            &sniffed,
            "CAST(file_name AS TEXT)='no_extension' AND format='jpeg'"
        ),
        1
    );
    assert_eq!(
        index_count(
            &e,
            &sniffed,
            "CAST(file_name AS TEXT)='renamed.dat' AND format='jpeg'"
        ),
        1
    );
}

#[test]
fn protected_roots_are_refused() {
    let env = TestEnv::new();
    let e = engine(&env);
    for root in ["/", "/System", "/usr"] {
        let o = ScanOptions {
            root: root.into(),
            ..Default::default()
        };
        let err = e
            .scan(&o, &EventSink::null(), &CancelToken::new())
            .unwrap_err();
        assert!(
            matches!(err, YojakaError::Protected { .. }),
            "{root}: {err}"
        );
        assert!(matches!(
            e.stats(root.as_ref(), false),
            Err(YojakaError::Protected { .. })
        ));
    }
}

#[test]
fn force_rebuilds_an_unusable_index() {
    let env = TestEnv::with_fixture(12, 40);
    let e = engine(&env);
    let o = opts(&env);
    let first = scan(&e, &o);

    // An index from a newer Yojaka (or a damaged one) blocks a normal scan.
    let idx_path = e.index_path(&o.root.canonicalize().unwrap(), false);
    rusqlite::Connection::open(&idx_path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert!(e.scan(&o, &EventSink::null(), &CancelToken::new()).is_err());

    let forced = ScanOptions {
        force: true,
        ..opts(&env)
    };
    let s = scan(&e, &forced);
    assert_eq!(s.files_indexed, first.files_indexed);
    assert_eq!(
        s.new_or_changed, first.files_indexed,
        "rebuilt from scratch, not incremental"
    );
    assert_eq!(s.unchanged, 0);
    // The user's files are untouched.
    assert!(
        o.root
            .join(&env.manifest.as_ref().unwrap().files[0].path)
            .exists()
    );
}

#[test]
fn stats_before_scan_is_a_clear_error() {
    let env = TestEnv::with_fixture(9, 20);
    let err = engine(&env)
        .stats(env.tree.as_ref().unwrap(), false)
        .unwrap_err();
    assert!(matches!(err, YojakaError::NoIndex(_)), "{err}");
}

#[test]
fn local_index_lives_in_the_scanned_folder_and_is_not_scanned() {
    let env = TestEnv::with_fixture(10, 40);
    let e = engine(&env);
    let o = ScanOptions {
        local_index: true,
        ..opts(&env)
    };
    let first = scan(&e, &o);
    assert!(o.root.join(".yojaka/index.db").is_file());
    assert_eq!(scan(&e, &o).files_indexed, first.files_indexed);
    assert!(e.stats(&o.root, true).unwrap().photos.count > 0);
}

#[test]
fn own_app_folders_are_never_scanned() {
    let env = TestEnv::with_fixture(11, 30);
    let tree = env.tree.clone().unwrap();
    // App data placed inside the scanned tree (e.g. YOJAKA_HOME pointing into the library).
    let paths = yojaka_core::AppPaths::under(&tree.join("yojaka_home"));
    paths.ensure().unwrap();
    std::fs::copy(
        tree.join(
            &env.manifest
                .as_ref()
                .unwrap()
                .files
                .iter()
                .find(|f| f.should_index && !f.corrupt)
                .unwrap()
                .path,
        ),
        paths.data.join("sample.jpg"),
    )
    .unwrap();
    let e = Engine::new(paths, Default::default());
    scan(&e, &opts(&env));
    assert_eq!(
        index_count(&e, &opts(&env), "full_path LIKE '%yojaka_home%'"),
        0
    );
}

/// Regression test: the old walker (jwalk plus a 50,000-entry back-pressure cap) deadlocked when
/// one folder held more entries than the cap (docs/adr/0005-directory-walker.md). This test is a
/// guard, not a reproduction: the old hang needed release-build timing. The manual check is a
/// release scan of the repo root, whose `target/debug/deps` has about 60k entries.
#[test]
fn a_folder_bigger_than_any_buffer_scans_to_completion() {
    use std::fs;
    use std::time::Duration;
    let env = TestEnv::new();
    let root = env.tmp.path().join("photos");
    for top in ["a", "b", "c", "d", "e", "f", "g", "h"] {
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let dir = root.join(format!("{top}/{i}/{j}/{k}"));
                    fs::create_dir_all(&dir).unwrap();
                    for n in 0..10 {
                        fs::write(dir.join(format!("{n}.jpg")), [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
                    }
                }
            }
        }
    }
    let big = root.join("big");
    fs::create_dir_all(&big).unwrap();
    for i in 0..60_000 {
        fs::File::create(big.join(format!("{i}.txt"))).unwrap();
    }

    let e = engine(&env);
    let o = ScanOptions {
        root,
        ..Default::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let s = e.scan(&o, &EventSink::null(), &CancelToken::new()).unwrap();
        let _ = tx.send(s);
    });
    let s = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("scan did not finish in 60 s: the directory walker is stuck");
    assert_eq!(s.status, ScanStatus::Complete);
    assert_eq!(s.files_seen, 60_000 + 8 * 64 * 10);
    assert_eq!(s.files_indexed, 8 * 64 * 10);
}

/// The skip list ("Library", "node_modules", hidden names...) applies inside the scanned folder,
/// never to the folder the user picked.
#[test]
fn a_root_named_like_a_skipped_folder_is_still_scanned() {
    let env = TestEnv::new();
    let root = env.tmp.path().join("Library");
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(root.join("a.jpg"), [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    std::fs::write(root.join("node_modules/b.jpg"), [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    let e = engine(&env);
    let o = ScanOptions {
        root,
        ..Default::default()
    };
    let s = scan(&e, &o);
    assert_eq!(
        s.files_indexed, 1,
        "a.jpg only; node_modules inside is still skipped"
    );
}
