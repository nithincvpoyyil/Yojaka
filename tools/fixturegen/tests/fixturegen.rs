#![allow(clippy::unwrap_used)]
use std::collections::BTreeMap;
use std::path::Path;

use fixturegen::{Spec, generate};

fn small() -> Spec {
    Spec {
        files: 120,
        noise: 20,
        ..Spec::default()
    }
}

/// Hash of the whole tree: relative paths, file bytes and symlink targets (not mtimes).
fn tree_hash(root: &Path) -> String {
    fn walk(dir: &Path, root: &Path, h: &mut blake3::Hasher) {
        let mut items: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        items.sort();
        for p in items {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
            h.update(rel.as_bytes());
            let md = std::fs::symlink_metadata(&p).unwrap();
            if md.file_type().is_symlink() {
                h.update(b"L");
                h.update(std::fs::read_link(&p).unwrap().to_string_lossy().as_bytes());
            } else if md.is_dir() {
                h.update(b"D");
                walk(&p, root, h);
            } else {
                h.update(b"F");
                h.update(&std::fs::read(&p).unwrap());
            }
        }
    }
    let mut h = blake3::Hasher::new();
    walk(root, root, &mut h);
    h.finalize().to_hex().to_string()
}

#[test]
fn same_seed_gives_identical_output() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    generate(&small(), a.path()).unwrap();
    generate(&small(), b.path()).unwrap();
    assert_eq!(tree_hash(a.path()), tree_hash(b.path()));
}

#[test]
fn different_seed_differs() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    generate(&small(), a.path()).unwrap();
    generate(&Spec { seed: 7, ..small() }, b.path()).unwrap();
    assert_ne!(tree_hash(a.path()), tree_hash(b.path()));
}

#[test]
fn manifest_matches_tree_on_disk() {
    let out = tempfile::tempdir().unwrap();
    let m = generate(&small(), out.path()).unwrap();
    let tree = out.path().join("tree");

    fn count(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                let md = std::fs::symlink_metadata(&p).unwrap();
                if md.is_dir() { count(&p) } else { 1 }
            })
            .sum()
    }
    assert_eq!(count(&tree), m.files.len());
    for e in &m.files {
        assert!(
            std::fs::symlink_metadata(tree.join(&e.path)).is_ok(),
            "missing {}",
            e.path
        );
    }
    assert_eq!(
        m.expected.indexed_count,
        m.files.iter().filter(|e| e.should_index).count()
    );

    let on_disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.path().join("manifest.json")).unwrap()).unwrap();
    assert_eq!(on_disk["generator_version"], 1);
    assert_eq!(on_disk["files"].as_array().unwrap().len(), m.files.len());
}

#[test]
fn bait_links_and_exact_groups_are_consistent() {
    let out = tempfile::tempdir().unwrap();
    let m = generate(&small(), out.path()).unwrap();
    assert!(
        m.files
            .iter()
            .any(|e| e.path.starts_with("Test.photoslibrary/") && !e.should_index)
    );
    assert!(
        m.files
            .iter()
            .any(|e| e.path.starts_with(".git/") && !e.should_index)
    );
    assert!(
        m.files
            .iter()
            .any(|e| e.symlink_to.is_some() && !e.should_index)
    );
    assert!(
        m.files
            .iter()
            .any(|e| e.hardlink_of.is_some() && e.should_index)
    );
    assert!(!m.expected.exact_groups.is_empty());
    let by_format: BTreeMap<_, _> = m.expected.by_format.clone();
    assert_eq!(by_format.values().sum::<usize>(), m.expected.indexed_count);
    for g in &m.expected.exact_groups {
        let sizes: Vec<_> = g
            .iter()
            .map(|p| std::fs::read(out.path().join("tree").join(p)).unwrap())
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] == w[1]));
    }
}

#[test]
fn jpegs_carry_the_exif_that_was_written() {
    let out = tempfile::tempdir().unwrap();
    let m = generate(
        &Spec {
            gps: 1.0,
            corrupt: 0.0,
            ..small()
        },
        out.path(),
    )
    .unwrap();
    let mut checked = 0;
    for e in m
        .files
        .iter()
        .filter(|e| e.format.as_deref() == Some("jpeg") && e.should_index && e.taken_at.is_some())
    {
        let f = std::fs::File::open(out.path().join("tree").join(&e.path)).unwrap();
        let exif = exif::Reader::new()
            .read_from_container(&mut std::io::BufReader::new(f))
            .unwrap();
        let dt = exif
            .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
            .unwrap();
        let want = e.taken_at.as_ref().unwrap().replace('T', " ");
        assert_eq!(dt.display_value().to_string(), want.replace('"', ""));
        let model = exif.get_field(exif::Tag::Model, exif::In::PRIMARY).unwrap();
        assert!(
            e.camera
                .as_ref()
                .unwrap()
                .ends_with(&model.display_value().to_string().replace('"', ""))
        );
        let lat = exif
            .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
            .unwrap();
        if let exif::Value::Rational(v) = &lat.value {
            let got = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
            assert!(
                (got - e.gps.as_ref().unwrap().lat.abs()).abs() < 1e-3,
                "{}",
                e.path
            );
        } else {
            panic!("latitude is not a rational");
        }
        checked += 1;
    }
    assert!(checked > 20);
}

#[test]
fn refuses_to_touch_a_foreign_non_empty_dir() {
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("precious.txt"), b"keep me").unwrap();
    assert!(generate(&small(), out.path()).is_err());
    assert!(out.path().join("precious.txt").exists());
}

#[test]
fn regenerating_into_a_previous_output_works() {
    let out = tempfile::tempdir().unwrap();
    generate(&small(), out.path()).unwrap();
    let first = tree_hash(out.path());
    generate(&small(), out.path()).unwrap();
    assert_eq!(first, tree_hash(out.path()));
}
