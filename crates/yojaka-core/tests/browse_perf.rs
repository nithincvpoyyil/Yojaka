//! Browsing speed on a 1M-row index. Slow to build, so ignored by default:
//! `cargo nextest run -p yojaka-core --run-ignored only -E 'binary(browse_perf)' --no-capture`
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::time::Instant;

use rusqlite::params;
use yojaka_core::browse::{BrowseQuery, Browser, KindFilter, SortKey};
use yojaka_core::index::Index;

const ROWS: i64 = 1_000_000;

/// Paths in the index start with this. Set `YOJAKA_PERF_ROOT=<dir>` to write the index to
/// `<dir>/.yojaka/index.db` and keep it, so `yojaka --local-index <dir>` can open it.
fn root() -> String {
    std::env::var("YOJAKA_PERF_ROOT").unwrap_or_else(|_| "/r".into())
}

fn build(path: &std::path::Path) {
    drop(Index::open(path).unwrap()); // creates the schema
    let mut conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "INSERT INTO scan(id,root_path,started_at,status) VALUES(1,CAST(?1 AS BLOB),0,'complete')",
        [root()],
    )
    .unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut f = tx
            .prepare(
                "INSERT INTO file(id,full_path,parent_folder,file_name,extension,size_bytes,modified_at_ns,\
                 device_id,inode,media_kind,format,last_seen_scan_id,metadata_status) \
                 VALUES(?1,?2,?3,?4,'jpg',?5,0,1,?1,?6,?7,1,'ok')",
            )
            .unwrap();
        let mut m = tx
            .prepare(
                "INSERT INTO meta(file_id,width,height,taken_at,taken_at_src,camera_make,camera_model,has_gps,\
                 place_city,place_country,duration_ms,pair_id) VALUES(?1,4032,3024,?2,'exif','Apple',?3,0,?4,'Portugal',?5,?6)",
            )
            .unwrap();
        for i in 0..ROWS {
            let folder = format!("{}/{:02}/{:03}", root(), i % 40, (i / 40) % 500);
            let name = format!("IMG_{i:07}.jpg");
            let video = i % 10 == 0;
            f.execute(params![
                i + 1,
                format!("{folder}/{name}").into_bytes(),
                folder.into_bytes(),
                name.into_bytes(),
                (i * 7919) % 9_000_000,
                if video { "video" } else { "image" },
                if video { "mov" } else { "jpeg" },
            ])
            .unwrap();
            m.execute(params![
                i + 1,
                1_300_000_000 + (i * 104_729) % 400_000_000,
                format!("iPhone {}", i % 7),
                format!("City{}", i % 300),
                video.then_some(i % 60_000),
                (i % 50 == 0).then_some(i),
            ])
            .unwrap();
        }
    }
    tx.commit().unwrap();
}

fn time<T>(label: &str, f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let v = f();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("{label:<48} {ms:>9.1} ms");
    (v, ms)
}

#[test]
#[ignore = "builds a 1M-row index, takes a while"]
fn browsing_a_million_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = match std::env::var("YOJAKA_PERF_ROOT") {
        Ok(r) => std::path::Path::new(&r).join(".yojaka").join("index.db"),
        Err(_) => dir.path().join("index.db"),
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let (_, ms) = time("build 1M-row index", || build(&path));
    println!("(build {ms:.0} ms)");
    let b = Browser::open(&path, root().into()).unwrap();

    let all = BrowseQuery::default();
    let (n, _) = time("count (all)", || b.count(&all).unwrap());
    assert_eq!(n, ROWS as u64);
    time("folder tree", || b.folders().unwrap());
    time("count (videos)", || {
        b.count(&BrowseQuery {
            kind: KindFilter::Videos,
            ..Default::default()
        })
        .unwrap()
    });
    time("count (one folder)", || {
        b.count(&BrowseQuery {
            folder: Some("/r/07".into()),
            ..Default::default()
        })
        .unwrap()
    });

    // The first deep page of a query pays for sorting it once; every page after that is instant.
    for sort in SortKey::ALL {
        let q = BrowseQuery {
            sort,
            descending: false,
            ..Default::default()
        };
        let (rows, first) = time(
            &format!("sort={:<9} first page, at the very end", sort.label()),
            || b.page(&q, 999_744, 256).unwrap(),
        );
        assert_eq!(rows.len(), 256);
        let mut worst = 0f64;
        for off in [0u64, 1, 250_000, 500_000, 750_000, 999_000] {
            let (rows, ms) = time(
                &format!("sort={:<9} next page @ {off}", sort.label()),
                || b.page(&q, off, 256).unwrap(),
            );
            assert!(!rows.is_empty());
            worst = worst.max(ms);
        }
        println!(
            "  -> sort={}: build+first {first:.0} ms, slowest later page {worst:.1} ms",
            sort.label()
        );
        assert!(worst < 50.0, "later pages must be quick, got {worst:.1} ms");
    }
    time("page 256, text filter 'iphone 3'", || {
        b.page(
            &BrowseQuery {
                text: "iphone 3".into(),
                ..Default::default()
            },
            0,
            256,
        )
        .unwrap()
    });
    time("page 256, text filter 'img_00123' (name)", || {
        b.page(
            &BrowseQuery {
                text: "img_00123".into(),
                ..Default::default()
            },
            0,
            256,
        )
        .unwrap()
    });
    time("detail of one row", || b.detail(500_000).unwrap());
}
