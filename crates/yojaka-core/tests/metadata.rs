#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::TestEnv;
use fixturegen::Spec;
use yojaka_core::export::{ExportFormat, ExportOptions};
use yojaka_core::meta::format_local;
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink};

fn spec(seed: u64) -> Spec {
    Spec {
        seed,
        files: 300,
        noise: 20,
        videos: 0.15,
        live_photos: 16,
        ..Default::default()
    }
}

fn engine(env: &TestEnv) -> Engine {
    Engine::new(env.paths.clone(), Default::default())
}

fn scan(e: &Engine, env: &TestEnv, metadata: bool) -> yojaka_core::scan::ScanSummary {
    let o = ScanOptions {
        root: env.tree.clone().unwrap(),
        metadata,
        ..Default::default()
    };
    e.scan(&o, &EventSink::null(), &CancelToken::new()).unwrap()
}

fn open(e: &Engine, env: &TestEnv) -> yojaka_core::index::Index {
    let root = env.tree.clone().unwrap().canonicalize().unwrap();
    yojaka_core::index::Index::open(&e.index_path(&root, false)).unwrap()
}

/// path (relative to the tree) → one row of interest
#[derive(Debug)]
struct Row {
    taken_at: Option<String>,
    camera: Option<String>,
    duration_ms: Option<i64>,
    gps: bool,
    country_code: Option<String>,
    city: Option<String>,
    pair_id: Option<i64>,
    status: String,
}

fn rows(e: &Engine, env: &TestEnv) -> BTreeMap<String, Row> {
    let idx = open(e, env);
    let root = env.tree.clone().unwrap().canonicalize().unwrap();
    let prefix = format!("{}/", root.display());
    let mut st = idx
        .connection()
        .prepare(
            "SELECT CAST(f.full_path AS TEXT), m.taken_at, m.camera_make, m.camera_model, m.duration_ms, \
             m.has_gps, m.place_country_code, m.place_city, m.pair_id, f.metadata_status \
             FROM file f LEFT JOIN meta m ON m.file_id=f.id",
        )
        .unwrap();
    st.query_map([], |r| {
        let path: String = r.get(0)?;
        let make: Option<String> = r.get(2)?;
        let model: Option<String> = r.get(3)?;
        Ok((
            path.strip_prefix(&prefix).unwrap().to_string(),
            Row {
                taken_at: r.get::<_, Option<i64>>(1)?.map(format_local),
                camera: make.zip(model).map(|(a, b)| format!("{a} {b}")),
                duration_ms: r.get(4)?,
                gps: r.get::<_, Option<bool>>(5)?.unwrap_or(false),
                country_code: r.get(6)?,
                city: r.get(7)?,
                pair_id: r.get(8)?,
                status: r.get(9)?,
            },
        ))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn metadata_matches_the_manifest() {
    let env = TestEnv::with_spec(spec(11));
    let e = engine(&env);
    let s = scan(&e, &env, true);
    let m = s.metadata.unwrap();
    let man = env.manifest.as_ref().unwrap();
    let rows = rows(&e, &env);

    let mut checked = 0;
    for f in man
        .files
        .iter()
        .filter(|f| f.should_index && f.symlink_to.is_none())
    {
        let r = rows
            .get(&f.path)
            .unwrap_or_else(|| panic!("{} missing from the index", f.path));
        if f.corrupt {
            assert_eq!(r.status, "error", "{}", f.path);
            continue;
        }
        // Wall-clock dates: the manifest writes `T`, the index `format_local` writes a space.
        if f.kind == "video" || f.format.as_deref() == Some("jpeg") {
            assert_eq!(r.status, "ok", "{}", f.path);
        }
        if let Some(t) = &f.taken_at {
            assert_eq!(
                r.taken_at.as_deref(),
                Some(t.replace('T', " ").as_str()),
                "{}",
                f.path
            );
        }
        if let Some(c) = &f.camera {
            assert_eq!(r.camera.as_deref(), Some(c.as_str()), "{}", f.path);
        }
        if let Some(d) = f.duration_ms {
            assert_eq!(r.duration_ms, Some(d as i64), "{}", f.path);
        }
        match &f.gps {
            Some(g) => {
                assert!(r.gps, "{}", f.path);
                assert_eq!(
                    r.country_code.as_deref(),
                    Some(g.expect_cc.as_str()),
                    "{}",
                    f.path
                );
                // GeoNames names neighbourhoods ("Paris 04 Hôtel-de-Ville", "Panjim"), so only the
                // country is compared here; exact cities are covered by the geocoder's own tests.
                assert!(r.city.is_some(), "{}", f.path);
            }
            None if f.kind == "video" || f.format.as_deref() == Some("jpeg") => {
                assert!(!r.gps, "{} has unexpected GPS", f.path)
            }
            None => {}
        }
        checked += 1;
    }
    assert!(checked > 250, "only {checked} files were checked");
    assert_eq!(
        m.errors as usize,
        man.files
            .iter()
            .filter(|f| f.should_index && f.corrupt)
            .count()
    );
    assert!(m.geocoded > 50);
}

#[test]
fn live_photos_all_paired_and_no_false_pairs() {
    let env = TestEnv::with_spec(spec(12));
    let e = engine(&env);
    scan(&e, &env, true);
    let man = env.manifest.as_ref().unwrap();
    let rows = rows(&e, &env);

    // Ground truth: files grouped by their live_pair number.
    let mut truth: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for f in &man.files {
        if let Some(n) = f.live_pair {
            truth.entry(n).or_default().insert(f.path.clone());
        }
    }
    assert!(truth.len() >= 16, "fixture should hold the planned pairs");
    assert_eq!(man.expected.live_pairs, truth.len());
    for halves in truth.values() {
        assert_eq!(halves.len(), 2);
    }

    // What the index paired: pair_id → files.
    let mut got: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for (path, r) in &rows {
        if let Some(p) = r.pair_id {
            got.entry(p).or_default().insert(path.clone());
        }
    }
    let truth_sets: BTreeSet<BTreeSet<String>> = truth.into_values().collect();
    let got_sets: BTreeSet<BTreeSet<String>> = got.into_values().collect();
    assert_eq!(got_sets, truth_sets, "paired sets differ from the manifest");

    // Both halves of a pair share a place (FR-47), and the pair count is reported once.
    for set in &got_sets {
        let cities: BTreeSet<_> = set.iter().map(|p| rows[p].city.clone()).collect();
        assert_eq!(cities.len(), 1, "{set:?} disagree on the place: {cities:?}");
    }
    let stats = e.stats(&env.tree.clone().unwrap(), false).unwrap();
    assert_eq!(stats.live_photo_pairs as usize, truth_sets.len());
}

#[test]
fn rescan_reads_only_new_or_changed_files() {
    let env = TestEnv::with_spec(spec(13));
    let e = engine(&env);
    let first = scan(&e, &env, true);
    assert!(first.metadata.as_ref().unwrap().extracted > 0);
    let second = scan(&e, &env, true);
    let m = second.metadata.unwrap();
    assert_eq!(m.extracted, 0, "unchanged files must not be re-read");
    assert_eq!(
        m.errors, 0,
        "known-bad files are not retried (and not re-reported)"
    );
}

#[test]
fn scan_without_metadata_then_extract_later() {
    let env = TestEnv::with_spec(spec(14));
    let e = engine(&env);
    let s = scan(&e, &env, false);
    assert!(s.metadata.is_none());
    let before = e.stats(&env.tree.clone().unwrap(), false).unwrap();
    assert!(before.metadata_pending > 0);
    assert!(before.by_year.is_empty());

    let m = e
        .extract_metadata(
            &env.tree.clone().unwrap(),
            false,
            &EventSink::null(),
            &CancelToken::new(),
        )
        .unwrap();
    assert!(m.extracted > 0);
    let after = e.stats(&env.tree.clone().unwrap(), false).unwrap();
    assert_eq!(after.metadata_pending, 0);
    assert!(!after.by_year.is_empty());
    assert!(after.video.total_duration_ms > 0);
}

#[test]
fn export_leaves_out_gps_unless_asked() {
    let env = TestEnv::with_spec(spec(15));
    let e = engine(&env);
    scan(&e, &env, true);
    let root = env.tree.clone().unwrap();
    let export = |fmt, gps| {
        let mut out = Vec::new();
        e.export(
            &root,
            false,
            fmt,
            ExportOptions { include_gps: gps },
            &mut out,
        )
        .unwrap();
        String::from_utf8(out).unwrap()
    };

    let csv = export(ExportFormat::Csv, false);
    let header = csv.lines().next().unwrap();
    assert!(!header.contains("gps"), "{header}");
    assert!(header.contains("city"));
    // No coordinate may leak into any row, e.g. Panaji's 15.4989.
    assert!(!csv.contains("15.4989") && !csv.contains("73.8278"));

    let csv_gps = export(ExportFormat::Csv, true);
    assert!(csv_gps.lines().next().unwrap().ends_with("gps_lat,gps_lon"));
    assert!(csv_gps.contains("15.4989"));

    let json = export(ExportFormat::Json, false);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.as_array().unwrap().len() > 250);
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .all(|o| o.get("gps_lat").is_none())
    );
    let v: serde_json::Value = serde_json::from_str(&export(ExportFormat::Json, true)).unwrap();
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|o| o["gps_lat"].is_number())
    );
}

#[test]
fn stats_cover_the_new_sections() {
    let env = TestEnv::with_spec(spec(16));
    let e = engine(&env);
    scan(&e, &env, true);
    let s = e.stats(&env.tree.clone().unwrap(), false).unwrap();
    assert!(s.video.total_duration_ms > 0);
    assert!(s.video.longest.len() <= 10 && !s.video.longest.is_empty());
    assert!(
        s.video
            .longest
            .windows(2)
            .all(|w| w[0].duration_ms >= w[1].duration_ms)
    );
    assert!(s.video.codecs.iter().any(|c| c.codec == "hevc"));
    assert!(s.video.codecs.iter().any(|c| c.codec == "h264"));
    assert!(s.live_photo_pairs >= 16);
    assert!(
        s.by_year
            .iter()
            .all(|y| y.months.iter().sum::<u64>() == y.count)
    );
    assert!(s.cameras.iter().any(|c| c.camera == "Apple iPhone 15"));
    assert!(s.undated > 0);
    assert!(s.location.with_gps > 0);
    assert_eq!(
        s.location.with_place + s.location.without_place,
        s.photos.count + s.videos.count
    );
    assert!(s.location.countries.iter().any(|c| c.country_code == "IN"));
    assert!(
        s.largest_files
            .windows(2)
            .all(|w| w[0].size_bytes >= w[1].size_bytes)
    );
}

#[test]
fn stats_snapshot() {
    let env = TestEnv::with_spec(Spec {
        seed: 99,
        files: 120,
        noise: 5,
        videos: 0.2,
        live_photos: 8,
        hardlinks: 2,
        symlinks: 2,
        ..Default::default()
    });
    let e = engine(&env);
    scan(&e, &env, true);
    let s = e.stats(&env.tree.clone().unwrap(), false).unwrap();
    // Byte sizes depend on the image encoder, and paths on the temp dir, so snapshot the
    // counts and shapes only.
    let view = serde_json::json!({
        "photos": s.photos.count,
        "videos": s.videos.count,
        "formats": s.formats.iter().map(|f| format!("{} {}", f.format, f.count)).collect::<Vec<_>>(),
        "video_total_duration_ms": s.video.total_duration_ms,
        "video_codecs": s.video.codecs.iter().map(|c| format!("{} {}", c.codec, c.count)).collect::<Vec<_>>(),
        "live_photo_pairs": s.live_photo_pairs,
        "by_year": s.by_year.iter().map(|y| format!("{} {} {:?}", y.year, y.count, y.months)).collect::<Vec<_>>(),
        "undated": s.undated,
        "cameras": s.cameras.iter().map(|c| format!("{} {}", c.camera, c.count)).collect::<Vec<_>>(),
        "with_gps": s.location.with_gps,
        "with_place": s.location.with_place,
        "countries": s.location.countries.iter().map(|c| format!("{} {}", c.country_code, c.count)).collect::<Vec<_>>(),
        "cities": s.location.cities.iter().map(|c| format!("{}, {} {}", c.city, c.country, c.count)).collect::<Vec<_>>(),
        "metadata_errors": s.metadata_errors,
    });
    insta::assert_yaml_snapshot!(view);
}
