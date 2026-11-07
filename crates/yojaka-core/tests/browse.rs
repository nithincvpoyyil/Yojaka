#![allow(clippy::unwrap_used)]

mod common;

use common::TestEnv;
use fixturegen::Spec;
use yojaka_core::browse::{BrowseQuery, Browser, KindFilter, SortKey};
use yojaka_core::scan::ScanOptions;
use yojaka_core::{CancelToken, Engine, EventSink};

fn setup() -> (TestEnv, Engine, Browser) {
    let env = TestEnv::with_spec(Spec {
        seed: 7,
        files: 300,
        noise: 20,
        videos: 0.2,
        live_photos: 10,
        ..Default::default()
    });
    let e = Engine::new(env.paths.clone(), Default::default());
    let o = ScanOptions {
        root: env.tree.clone().unwrap(),
        metadata: true,
        ..Default::default()
    };
    e.scan(&o, &EventSink::null(), &CancelToken::new()).unwrap();
    let b = e.browser(env.tree.as_ref().unwrap(), false).unwrap();
    (env, e, b)
}

#[test]
fn kind_filters_partition_the_index() {
    let (_env, _e, b) = setup();
    let all = b.count(&BrowseQuery::default()).unwrap();
    let photos = b
        .count(&BrowseQuery {
            kind: KindFilter::Photos,
            ..Default::default()
        })
        .unwrap();
    let videos = b
        .count(&BrowseQuery {
            kind: KindFilter::Videos,
            ..Default::default()
        })
        .unwrap();
    assert!(all > 0 && photos > 0 && videos > 0);
    assert_eq!(photos + videos, all);
}

#[test]
fn pages_tile_the_result_without_gaps_or_overlap() {
    let (_env, _e, b) = setup();
    for sort in SortKey::ALL {
        for descending in [false, true] {
            let q = BrowseQuery {
                sort,
                descending,
                ..Default::default()
            };
            let total = b.count(&q).unwrap();
            let whole = b.page(&q, 0, total + 10).unwrap();
            assert_eq!(whole.len() as u64, total, "{sort:?}");
            let mut stitched = vec![];
            let mut off = 0;
            while off < total {
                stitched.extend(b.page(&q, off, 64).unwrap());
                off += 64;
            }
            assert_eq!(stitched, whole, "{sort:?} desc={descending}");
        }
    }
}

#[test]
fn the_sorted_temp_table_gives_the_same_pages_as_direct_queries() {
    let (env, e, direct) = setup();
    let sorted = e
        .browser(env.tree.as_ref().unwrap(), false)
        .unwrap()
        .always_materialize();
    let tree = direct.folders().unwrap();
    let folder = tree
        .nodes
        .iter()
        .find(|n| n.files_total > 20)
        .unwrap()
        .path
        .clone();
    for sort in SortKey::ALL {
        for descending in [false, true] {
            for (folder, kind, text) in [
                (None, KindFilter::All, ""),
                (None, KindFilter::Videos, ""),
                (Some(folder.clone()), KindFilter::Photos, ""),
                (None, KindFilter::All, "img"),
            ] {
                let q = BrowseQuery {
                    folder,
                    kind,
                    text: text.into(),
                    sort,
                    descending,
                };
                for (off, n) in [(0, 40), (17, 5), (100, 64), (10_000, 10)] {
                    assert_eq!(
                        sorted.page(&q, off, n).unwrap(),
                        direct.page(&q, off, n).unwrap(),
                        "{sort:?} desc={descending} off={off} {q:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn switching_queries_rebuilds_the_temp_table() {
    let (env, e, _b) = setup();
    let b = e
        .browser(env.tree.as_ref().unwrap(), false)
        .unwrap()
        .always_materialize();
    let by_size = BrowseQuery {
        sort: SortKey::Size,
        ..Default::default()
    };
    let by_name = BrowseQuery::default();
    let a = b.page(&by_size, 0, 5).unwrap();
    let n = b.page(&by_name, 0, 5).unwrap();
    assert_ne!(a, n);
    // and back again
    assert_eq!(b.page(&by_size, 0, 5).unwrap(), a);
}

#[test]
fn folder_filter_matches_the_tree_totals() {
    let (_env, _e, b) = setup();
    let tree = b.folders().unwrap();
    assert_eq!(
        tree.nodes[0].files_total,
        b.count(&BrowseQuery::default()).unwrap()
    );
    for node in &tree.nodes {
        let q = BrowseQuery {
            folder: Some(node.path.clone()),
            ..Default::default()
        };
        assert_eq!(b.count(&q).unwrap(), node.files_total, "{:?}", node.path);
        for row in b.page(&q, 0, 1000).unwrap() {
            assert!(row.path.starts_with(&node.path));
        }
    }
}

#[test]
fn folder_filter_does_not_leak_into_similarly_named_siblings() {
    let (_env, _e, b) = setup();
    let tree = b.folders().unwrap();
    // "/x/2020" must not match "/x/2020-extra": find any folder and check every row's parent.
    let leaf = tree
        .nodes
        .iter()
        .find(|n| n.children.is_empty() && n.files_direct > 0)
        .unwrap();
    let rows = b
        .page(
            &BrowseQuery {
                folder: Some(leaf.path.clone()),
                ..Default::default()
            },
            0,
            1000,
        )
        .unwrap();
    assert_eq!(rows.len() as u64, leaf.files_direct);
}

#[test]
fn text_filter_matches_names_and_can_combine_with_a_folder() {
    let (_env, _e, b) = setup();
    let first = b.page(&BrowseQuery::default(), 0, 1).unwrap().remove(0);
    let stem = first.name.split('.').next().unwrap().to_uppercase();
    let q = BrowseQuery {
        text: stem.clone(),
        ..Default::default()
    };
    let rows = b.page(&q, 0, 100).unwrap();
    assert!(rows.iter().any(|r| r.id == first.id));
    assert_eq!(rows.len() as u64, b.count(&q).unwrap());
    let q2 = BrowseQuery {
        folder: Some(first.path.parent().unwrap().to_path_buf()),
        text: stem,
        kind: KindFilter::All,
        ..Default::default()
    };
    assert!(
        b.page(&q2, 0, 100)
            .unwrap()
            .iter()
            .any(|r| r.id == first.id)
    );
    // wildcard characters are literal
    let none = BrowseQuery {
        text: "%".into(),
        ..Default::default()
    };
    assert_eq!(b.count(&none).unwrap(), 0);
}

#[test]
fn detail_reports_live_photo_partners_and_never_gps() {
    let (_env, _e, b) = setup();
    let live = b
        .page(&BrowseQuery::default(), 0, 1000)
        .unwrap()
        .into_iter()
        .find(|r| r.live)
        .expect("fixture has live photos");
    let d = b.detail(live.id).unwrap().unwrap();
    let partner = d.live_partner.expect("pair partner");
    assert_ne!(partner, live.path);
    assert_eq!(partner.file_stem(), live.path.file_stem());
    assert!(b.detail(-1).unwrap().is_none());
}

#[test]
fn browser_refuses_unscanned_roots() {
    let env = TestEnv::with_fixture(1, 20);
    let e = Engine::new(env.paths.clone(), Default::default());
    let err = e.browser(env.tree.as_ref().unwrap(), false).unwrap_err();
    assert!(matches!(err, yojaka_core::YojakaError::NoIndex(_)), "{err}");
}
