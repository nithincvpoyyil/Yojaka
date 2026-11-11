//! `TestBackend` snapshots of every screen at 80×24 and 160×48, plus key-sequence tests
//! that drive `update()` the way the event loop does.

use std::path::{Path, PathBuf};
use std::time::Instant;

use ratatui::{Terminal, backend::TestBackend};
use yojaka_core::browse::{FileDetail, FileRow, FolderTree};
use yojaka_core::model::MediaKind;
use yojaka_core::stats::{
    CameraStat, CityStat, CountryStat, FormatStat, KindStats, LastScan, LocationStats, Stats,
    VideoStats, YearStat,
};
use yojaka_core::{CoreEvent, TaskKind};

use crate::app::{App, Cmd, Mode, Msg, Screen};
use crate::browser::{FETCH, Focus, Window};
use crate::view;
use crate::worker::{Bg, Request, Response};

const SIZES: [(u16, u16); 2] = [(80, 24), (160, 48)];

fn draw(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| view::render(app, f)).unwrap();
    t.backend().to_string()
}

fn app_at(w: u16, h: u16) -> App {
    let mut a = App::new("/photos".into(), Mode::ReadOnly);
    a.size = (w, h);
    let _ = a.init();
    a
}

fn city(city: &str, country: &str, count: u64) -> CityStat {
    CityStat {
        city: city.into(),
        state: None,
        country: country.into(),
        count,
    }
}

fn stats() -> Stats {
    let year = |year, count: u64| YearStat {
        year,
        count,
        months: [count / 12; 12],
    };
    let fmt = |f: &str, k: &str, count, bytes| FormatStat {
        format: f.into(),
        kind: k.into(),
        count,
        bytes,
    };
    Stats {
        root: "/photos".into(),
        last_scan: Some(LastScan {
            id: 1,
            status: "complete".into(),
            started_at: 0,
            finished_at: Some(2),
            files_seen: 14_210,
            errors: 2,
        }),
        photos: KindStats {
            count: 12_480,
            bytes: 48_300_000_000,
        },
        videos: KindStats {
            count: 391,
            bytes: 96_100_000_000,
        },
        cloud_only: 12,
        errors: 2,
        formats: vec![
            fmt("jpeg", "image", 9_100, 20_000_000_000),
            fmt("heic", "image", 2_900, 14_000_000_000),
            fmt("mov", "video", 300, 80_000_000_000),
            fmt("raw-dng", "image", 480, 14_000_000_000),
            fmt("mp4", "video", 91, 16_000_000_000),
        ],
        video: VideoStats {
            total_duration_ms: 93_600_000,
            ..Default::default()
        },
        live_photo_pairs: 840,
        by_year: vec![
            year(2019, 1_200),
            year(2020, 2_800),
            year(2021, 3_400),
            year(2022, 2_100),
            year(2023, 3_371),
        ],
        undated: 37,
        cameras: vec![
            CameraStat {
                camera: "Apple iPhone 14 Pro".into(),
                count: 7_900,
            },
            CameraStat {
                camera: "SONY ILCE-7M3".into(),
                count: 3_100,
            },
            CameraStat {
                camera: "Google Pixel 7".into(),
                count: 900,
            },
        ],
        largest_files: vec![],
        location: LocationStats {
            with_place: 9_800,
            without_place: 3_071,
            countries: vec![
                CountryStat {
                    country_code: "IN".into(),
                    country: "India".into(),
                    count: 6_100,
                },
                CountryStat {
                    country_code: "FR".into(),
                    country: "France".into(),
                    count: 2_400,
                },
                CountryStat {
                    country_code: "JP".into(),
                    country: "Japan".into(),
                    count: 1_300,
                },
            ],
            cities: vec![
                city("Chennai", "India", 4_200),
                city("Paris", "France", 2_400),
                city("Kochi", "India", 1_900),
                city("Kyoto", "Japan", 1_300),
            ],
            ..Default::default()
        },
        metadata_pending: 0,
        metadata_errors: 0,
    }
}

fn row(id: i64, name: &str) -> FileRow {
    FileRow {
        id,
        path: PathBuf::from(format!("/photos/2023/{name}")),
        name: name.into(),
        kind: MediaKind::Image,
        format: Some("jpeg".into()),
        width: Some(4032),
        height: Some(3024),
        duration_ms: None,
        size_bytes: 3_200_000 + id as u64 * 1000,
        taken_at: Some(1_684_000_000 + id * 86_400),
        camera: Some("Apple iPhone 14 Pro".into()),
        place: Some("Lisbon, Portugal".into()),
        place_inferred: false,
        live: false,
        cloud_only: false,
    }
}

fn rows() -> Vec<FileRow> {
    let mut v: Vec<FileRow> = (0..60)
        .map(|i| row(i, &format!("IMG_{:04}.jpg", 1200 + i)))
        .collect();
    v[1].live = true;
    v[1].format = Some("heic".into());
    v[2].kind = MediaKind::Video;
    v[2].format = Some("mov".into());
    v[2].duration_ms = Some(125_000);
    v[2].width = Some(3840);
    v[2].height = Some(2160);
    v[3].place = Some("Porto, Portugal".into());
    v[3].place_inferred = true;
    v[4].camera = None;
    v[4].taken_at = None;
    v[4].place = None;
    v[5].cloud_only = true;
    v
}

fn folders() -> FolderTree {
    FolderTree::build(
        Path::new("/photos"),
        [
            (PathBuf::from("/photos"), 12),
            (PathBuf::from("/photos/2022"), 4_100),
            (PathBuf::from("/photos/2023"), 5_300),
            (PathBuf::from("/photos/2023/Lisbon trip"), 2_400),
            (PathBuf::from("/photos/Videos"), 391),
        ],
    )
}

fn loaded_browser(w: u16, h: u16) -> App {
    let mut a = app_at(w, h);
    a.screen = Screen::Browser;
    a.update(Msg::Bg(Bg::Worker(Response::Folders(folders()))));
    a.browser.total = Some(12_871);
    a.browser.window = Some(Window {
        offset: 0,
        rows: rows(),
    });
    a.browser.expanded.insert(2);
    a
}

fn detail_for(r: FileRow) -> Box<FileDetail> {
    Box::new(FileDetail {
        row: r,
        modified_at_ns: 0,
        taken_at_src: Some("exif".into()),
        lens: Some("iPhone 14 Pro back camera 6.86mm f/1.78".into()),
        fps: None,
        video_codec: None,
        audio_codec: None,
        bitrate: None,
        container: None,
        frames: None,
        has_gps: true,
        loc_src: Some("gps".into()),
        metadata_status: "ok".into(),
        live_partner: Some("/photos/2023/IMG_1201.MOV".into()),
    })
}

// ---- Dashboard ---------------------------------------------------------------------------

#[test]
fn dashboard_idle_with_numbers() {
    for (w, h) in SIZES {
        let mut a = app_at(w, h);
        a.dashboard_stats(stats());
        insta::assert_snapshot!(format!("dashboard_idle_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn dashboard_scanning_without_a_known_total() {
    for (w, h) in SIZES {
        let mut a = app_at(w, h);
        a.update(Msg::Rescan);
        a.update(Msg::Core(CoreEvent::Progress {
            task: TaskKind::Scan,
            done: 48_210,
            total: None,
            bytes: 0,
            current: Some("/photos/2021/Summer in Kerala/IMG_8841.HEIC".into()),
        }));
        a.dashboard_scan_rate(1_850.0);
        insta::assert_snapshot!(format!("dashboard_scanning_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn dashboard_reading_metadata_with_a_gauge() {
    for (w, h) in SIZES {
        let mut a = app_at(w, h);
        a.dashboard_stats(stats());
        a.update(Msg::Rescan);
        a.update(Msg::Core(CoreEvent::Progress {
            task: TaskKind::Metadata,
            done: 6_000,
            total: Some(12_871),
            bytes: 0,
            current: Some("/photos/2023/IMG_1.jpg".into()),
        }));
        a.dashboard_scan_rate(940.0);
        insta::assert_snapshot!(format!("dashboard_metadata_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn dashboard_before_the_first_scan() {
    for (w, h) in SIZES {
        let mut a = app_at(w, h);
        a.update(Msg::Bg(Bg::Worker(Response::NoIndex)));
        // the automatic scan started; show the idle "no index" state instead
        a.dashboard_stop_scan();
        insta::assert_snapshot!(format!("dashboard_empty_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn dashboard_error_for_a_protected_folder() {
    let mut a = app_at(80, 24);
    a.update(Msg::Bg(Bg::Worker(Response::Error(
        "refusing to scan /System: system folder".into(),
    ))));
    insta::assert_snapshot!("dashboard_error_80x24", draw(&a, 80, 24));
}

// ---- Browser -----------------------------------------------------------------------------

#[test]
fn e_exports_the_files_the_browser_shows() {
    let mut a = app_at(100, 30);
    a.dashboard_stats(stats());
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::CycleKind);
    let cmds = a.update(Msg::ExportView);
    match &cmds[..] {
        [Cmd::ExportView(q)] => {
            assert_eq!(q.kind, yojaka_core::browse::KindFilter::Photos);
        }
        other => panic!("expected one export command, got {other:?}"),
    }
    // On another screen the key does nothing.
    a.update(Msg::Goto(Screen::History));
    assert!(a.update(Msg::ExportView).is_empty());
}

mod saved_choices {
    use super::*;
    use crate::app::Remember;

    fn config(keep: &[&str], template: &str, mode: &str) -> yojaka_core::Config {
        let mut c = yojaka_core::Config::default();
        c.dupes.keep_rules = keep.iter().map(|s| (*s).to_string()).collect();
        c.organize.template = template.into();
        c.organize.mode = mode.into();
        c
    }

    #[test]
    fn the_defaults_change_nothing() {
        let mut a = app_at(100, 30);
        a.use_config(&yojaka_core::Config::default());
        assert_eq!(a.dupes.chain_text(), "oldest-modified,shortest-path");
        assert_eq!(a.dupes.preset, 0);
        assert!(a.dupes.saved.is_none() && a.notice.is_none());
        assert_eq!(a.organize.transfer, yojaka_core::organize::Transfer::Copy);
        assert_eq!(a.organize.preset_label(), "Date");
    }

    #[test]
    fn a_saved_chain_is_the_starting_rule_and_one_more_step_in_the_cycle() {
        let mut a = app_at(100, 30);
        a.use_config(&config(
            &["largest-file", "shortest-path"],
            "{year}/{month:02}/{name}",
            "copy",
        ));
        assert_eq!(a.dupes.chain_text(), "largest-file,shortest-path");
        assert!(a.dupes.preset_label().starts_with("saved:"));
        // The presets still come round; after the last one the saved chain is back.
        a.screen = Screen::Duplicates;
        a.update(Msg::NextRule);
        assert_eq!(a.dupes.chain_text(), crate::dupes::PRESETS[0].1);
        for _ in 0..crate::dupes::PRESETS.len() {
            a.update(Msg::NextRule);
        }
        assert_eq!(a.dupes.chain_text(), "largest-file,shortest-path");
    }

    #[test]
    fn a_saved_chain_equal_to_a_preset_selects_that_preset() {
        let mut a = app_at(100, 30);
        a.use_config(&config(
            &["shortest-path", "oldest-modified"],
            "{year}/{month:02}/{name}",
            "copy",
        ));
        assert_eq!(a.dupes.preset, 1);
        assert!(a.dupes.saved.is_none());
    }

    #[test]
    fn saved_organize_choices_are_the_starting_settings() {
        let mut a = app_at(100, 30);
        a.use_config(&config(
            &["oldest-modified"],
            "{country}/{city}/{name}",
            "move",
        ));
        assert_eq!(a.organize.transfer, yojaka_core::organize::Transfer::Move);
        assert_eq!(a.organize.template, "{country}/{city}/{name}");
        assert_eq!(a.organize.preset_label(), "Place");
        let custom = {
            let mut b = app_at(100, 30);
            b.use_config(&config(&[], "{year}/{name}", "copy"));
            b.organize.preset_label()
        };
        assert_eq!(custom, "Custom");
    }

    #[test]
    fn bad_saved_values_are_ignored_and_the_footer_says_so() {
        let mut a = app_at(100, 30);
        a.use_config(&config(&["bogus"], "{nope}/{name}", "teleport"));
        assert_eq!(a.dupes.chain_text(), "oldest-modified,shortest-path");
        assert_eq!(a.organize.template, yojaka_core::organize::PRESETS[0].1);
        assert_eq!(a.organize.transfer, yojaka_core::organize::Transfer::Copy);
        assert!(a.notice.as_ref().unwrap().0.contains("ignored"));
        // The app still works.
        a.update(Msg::Goto(Screen::Organize));
        assert!(a.update(Msg::Enter).len() <= 1);
    }

    #[test]
    fn m_remembers_only_when_pressed_and_only_on_the_two_screens() {
        let mut a = app_at(100, 30);
        a.dashboard_stats(stats());
        a.screen = Screen::Duplicates;
        a.update(Msg::NextRule);
        let cmds = a.update(Msg::Remember);
        assert!(matches!(
            &cmds[..],
            [Cmd::Remember(Remember::KeepRules(c))] if c == "shortest-path,oldest-modified"
        ));
        a.screen = Screen::Organize;
        a.organize.cursor = 3; // Action: copy -> move
        a.update(Msg::Enter);
        let cmds = a.update(Msg::Remember);
        assert!(matches!(
            &cmds[..],
            [Cmd::Remember(Remember::Organize { template, mode })]
                if template == yojaka_core::organize::PRESETS[0].1 && mode == "move"
        ));
        a.screen = Screen::Browser;
        assert!(a.update(Msg::Remember).is_empty());
    }
}

#[test]
fn browser_with_rows_and_details() {
    for (w, h) in SIZES {
        let mut a = loaded_browser(w, h);
        a.update(Msg::Down); // asks for the details of row 1
        let d = detail_for(a.browser.row_at(1).unwrap().clone());
        a.update(Msg::Bg(Bg::Worker(Response::Detail {
            id: 1,
            detail: Some(d),
        })));
        insta::assert_snapshot!(format!("browser_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn browser_detail_of_a_video_before_the_details_arrive() {
    let mut a = loaded_browser(160, 48);
    a.browser.cursor = 2;
    insta::assert_snapshot!("browser_video_loading_160x48", draw(&a, 160, 48));
}

#[test]
fn browser_scrolled_with_the_tree_focused_and_sorted() {
    let mut a = loaded_browser(80, 24);
    a.browser.focus = Focus::Tree;
    a.browser.selected = 2;
    a.browser.top = 20;
    a.browser.cursor = 23;
    a.browser.query.sort = yojaka_core::browse::SortKey::Size;
    a.browser.query.descending = true;
    insta::assert_snapshot!("browser_tree_focus_80x24", draw(&a, 80, 24));
}

#[test]
fn browser_rows_outside_the_window_show_placeholders() {
    let mut a = loaded_browser(80, 24);
    a.browser.top = 1_000;
    a.browser.cursor = 1_002;
    let out = draw(&a, 80, 24);
    assert!(out.contains('…'));
    insta::assert_snapshot!("browser_placeholders_80x24", out);
}

#[test]
fn browser_filter_prompt_and_empty_result() {
    let mut a = loaded_browser(80, 24);
    a.update(Msg::StartFilter);
    a.browser.query.text = "kerala".into();
    a.browser.total = Some(0);
    a.browser.window = None;
    insta::assert_snapshot!("browser_no_match_80x24", draw(&a, 80, 24));
}

#[test]
fn browser_before_anything_is_indexed() {
    let mut a = app_at(80, 24);
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::Bg(Bg::Worker(Response::NoIndex)));
    a.dashboard_stop_scan();
    insta::assert_snapshot!("browser_empty_80x24", draw(&a, 80, 24));
}

// ---- Help, placeholders, debug overlay ----------------------------------------------------

#[test]
fn help_overlay() {
    for (w, h) in SIZES {
        let mut a = app_at(w, h);
        a.show_help = true;
        insta::assert_snapshot!(format!("help_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn the_read_only_badge_is_always_in_the_header() {
    for (w, h) in SIZES {
        for screen in [Screen::Dashboard, Screen::Browser, Screen::History] {
            let mut a = app_at(w, h);
            a.screen = screen;
            assert!(
                draw(&a, w, h)
                    .lines()
                    .next()
                    .unwrap()
                    .contains("[READ-ONLY]")
            );
        }
    }
    let mut a = App::new("/p".into(), Mode::Write);
    a.size = (80, 24);
    assert!(draw(&a, 80, 24).lines().next().unwrap().contains("[WRITE]"));
}

#[test]
fn the_debug_overlay_shows_frame_times() {
    let mut a = loaded_browser(80, 24);
    a.update(Msg::ToggleDebug);
    a.debug_info.frame_ms = 1.25;
    a.debug_info.fetch_ms = 12.5;
    let out = draw(&a, 80, 24);
    assert!(out.contains("frame 1.25 ms"), "{out}");
    assert!(out.contains("rows 12,871"), "{out}");
}

// ---- no screen may panic at any size -----------------------------------------------------

#[test]
fn every_screen_renders_at_every_small_size() {
    for screen in [
        Screen::Dashboard,
        Screen::Browser,
        Screen::Duplicates,
        Screen::History,
    ] {
        for (w, h) in [
            (1, 1),
            (10, 3),
            (20, 5),
            (40, 10),
            (60, 12),
            (79, 23),
            (80, 24),
            (200, 60),
        ] {
            let mut a = loaded_browser(w, h);
            a.dashboard_stats(stats());
            a.screen = screen;
            a.show_help = (w + h) % 2 == 0;
            a.debug = true;
            let _ = draw(&a, w, h);
        }
    }
}

// ---- frame time -------------------------------------------------------------------------

#[test]
fn rendering_a_full_browser_frame_is_far_below_16_ms() {
    let mut a = loaded_browser(160, 48);
    a.browser.window = Some(Window {
        offset: 0,
        rows: (0..FETCH as i64)
            .map(|i| row(i, &format!("IMG_{i:05}.jpg")))
            .collect(),
    });
    let mut t = Terminal::new(TestBackend::new(160, 48)).unwrap();
    let n = 200;
    let start = Instant::now();
    for i in 0..n {
        a.browser.cursor = i;
        a.browser.top = i;
        t.draw(|f| view::render(&a, f)).unwrap();
    }
    let per_frame = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
    assert!(per_frame < 16.0, "{per_frame:.2} ms per frame");
}

// ---- key sequences -----------------------------------------------------------------------

fn keys(a: &mut App, msgs: Vec<Msg>) -> Vec<Cmd> {
    msgs.into_iter().flat_map(|m| a.update(m)).collect()
}

#[test]
fn first_visit_scans_then_loads_everything() {
    let mut a = app_at(80, 24);
    // nothing saved: the browser and the stats both report it
    let cmds = keys(
        &mut a,
        vec![
            Msg::Bg(Bg::Worker(Response::NoIndex)),
            Msg::Bg(Bg::Worker(Response::NoIndex)),
        ],
    );
    assert_eq!(
        cmds.iter()
            .filter(|c| matches!(c, Cmd::StartScan { .. }))
            .count(),
        1
    );
    assert!(a.status_line().contains("scanning"));
}

fn stats_scanned_at(finished: i64) -> Stats {
    let mut s = stats();
    s.last_scan = Some(yojaka_core::stats::LastScan {
        id: 1,
        status: "complete".into(),
        started_at: finished - 5,
        finished_at: Some(finished),
        files_seen: 10,
        errors: 0,
    });
    s
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn an_existing_index_is_not_rescanned_on_open_by_default() {
    let mut a = app_at(80, 24);
    let old = stats_scanned_at(now() - 5 * 3600);
    let cmds = a.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(old)))));
    assert!(cmds.is_empty() && !a.dashboard.scan.running);
}

#[test]
fn refresh_scans_on_open_only_when_the_last_scan_is_over_an_hour_old() {
    // old: scans, without clearing anything
    let mut a = app_at(80, 24);
    a.refresh_when_stale = true;
    let old = stats_scanned_at(now() - 2 * 3600);
    let cmds = a.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(old)))));
    assert!(matches!(cmds.as_slice(), [Cmd::StartScan { force: false }]));
    assert!(a.dashboard.stats.is_some(), "old data stays on screen");
    // recent: does nothing
    let mut b = app_at(80, 24);
    b.refresh_when_stale = true;
    let fresh = stats_scanned_at(now() - 600);
    assert!(
        b.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(fresh)))))
            .is_empty()
    );
    // later statistics refreshes (live during a scan) never start another scan
    let old = stats_scanned_at(now() - 9 * 3600);
    assert!(
        b.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(old)))))
            .is_empty()
    );
}

#[test]
fn browsing_session() {
    let mut a = app_at(80, 24);
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::Bg(Bg::Worker(Response::Folders(folders()))));
    let g = a.browser.generation;
    a.update(Msg::Bg(Bg::Worker(Response::Count {
        generation: g,
        total: 500,
    })));
    a.update(Msg::Bg(Bg::Worker(Response::Page {
        generation: g,
        offset: 0,
        limit: FETCH,
        rows: (0..FETCH as i64)
            .map(|i| row(i, &format!("f{i}.jpg")))
            .collect(),
        ms: 1.0,
    })));
    assert!(a.browser.current_row().is_some());

    // move down: the details of the new row are requested
    let cmds = a.update(Msg::Down);
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::Worker(Request::Detail { id: 1 })))
    );

    // kind filter restarts the table
    let cmds = a.update(Msg::CycleKind);
    assert_eq!(cmds.len(), 2);
    assert!(a.browser.window.is_none());

    // answers for the old query are ignored
    let old = g;
    a.update(Msg::Bg(Bg::Worker(Response::Count {
        generation: old,
        total: 1,
    })));
    assert_eq!(a.browser.total, None);

    // switch to the tree, move onto a folder: the table filters by it
    a.update(Msg::Tab);
    assert_eq!(a.browser.focus, Focus::Tree);
    let cmds = a.update(Msg::Down);
    assert_eq!(cmds.len(), 2);
    assert_eq!(
        a.browser.query.folder.as_deref(),
        Some(Path::new("/photos/2022"))
    );
}

#[test]
fn leaving_the_browser_while_typing_a_filter_stops_the_typing() {
    let mut a = app_at(80, 24);
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::StartFilter);
    assert!(a.browser.editing_filter);
    a.update(Msg::Goto(Screen::Dashboard));
    assert!(!a.browser.editing_filter);
}

#[test]
fn resizing_recomputes_how_many_rows_fit() {
    let mut a = app_at(80, 24);
    let small = a.browser.table_rows;
    a.update(Msg::Resize(160, 48));
    assert!(a.browser.table_rows > small);
    assert_eq!(a.browser.tree_rows, 41);
}

#[test]
fn the_ui_state_survives_a_scan_finishing() {
    let mut a = loaded_browser(80, 24);
    a.browser.query.text = "img".into();
    a.update(Msg::Rescan);
    let cmds = a.update(Msg::Bg(Bg::ScanFinished(Err("boom".into()))));
    assert!(cmds.is_empty());
    assert_eq!(a.browser.query.text, "img");
    assert_eq!(a.dashboard.error.as_deref(), Some("boom"));
}

#[test]
fn tab_bar_names_every_screen_and_marks_the_current_one() {
    let mut a = app_at(80, 24);
    a.update(Msg::Goto(Screen::Organize));
    let out = draw(&a, 80, 24);
    let rows: Vec<&str> = out.lines().skip(1).take(3).collect();
    for name in ["Dashboard", "File browser", "Duplicates", "History"] {
        assert!(rows[1].contains(name), "{}", rows[1]);
    }
    assert!(rows[1].contains("│ 4 Organize │"), "{}", rows[1]);
    // the current tab is open at the bottom, the others stand on the line
    assert!(
        rows[2].contains('┘') && rows[2].contains('└'),
        "{}",
        rows[2]
    );
    // narrow: numbers only, plus the name of the current screen
    let narrow = view::tab_bar(Screen::Duplicates, 50, false);
    assert!(narrow[1].to_string().contains("│ 3 Duplicates │"));
    assert!(!narrow[1].to_string().contains("Dashboard"));
    for line in &narrow {
        assert!(line.width() <= 50, "{line}");
    }
}

#[test]
fn full_rescan_asks_first_and_only_flushes_on_yes() {
    let mut a = app_at(80, 24);
    a.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(stats())))));
    // asking does nothing yet
    assert!(a.update(Msg::FullRescan).is_empty());
    assert!(a.confirm_flush);
    insta::assert_snapshot!("full_rescan_prompt_80x24", draw(&a, 80, 24));
    // no / Esc: back to normal, nothing started
    assert!(a.update(Msg::ConfirmNo).is_empty());
    assert!(!a.confirm_flush && !a.dashboard.scan.running);
    a.update(Msg::FullRescan);
    a.update(Msg::Back);
    assert!(!a.confirm_flush && !a.dashboard.scan.running);
    // yes: scan with force, and nothing from the old index stays on screen
    a.update(Msg::FullRescan);
    let cmds = a.update(Msg::ConfirmYes);
    assert!(matches!(cmds.as_slice(), [Cmd::StartScan { force: true }]));
    assert!(a.dashboard.scan.running && !a.confirm_flush);
    assert!(a.dashboard.stats.is_none());
    assert_eq!(a.browser.total, Some(0));
    // a plain rescan never forces
    let mut b = app_at(80, 24);
    assert!(matches!(
        b.update(Msg::Rescan).as_slice(),
        [Cmd::StartScan { force: false }]
    ));
    // R is ignored while a scan runs
    assert!(b.update(Msg::FullRescan).is_empty());
    assert!(!b.confirm_flush);
}

#[test]
fn the_prompt_only_listens_to_yes_and_no() {
    use crate::keymap::translate;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
    assert!(matches!(
        translate(k(KeyCode::Char('R')), false, false),
        Some(Msg::FullRescan)
    ));
    assert!(matches!(
        translate(k(KeyCode::Char('y')), false, true),
        Some(Msg::ConfirmYes)
    ));
    assert!(matches!(
        translate(k(KeyCode::Esc), false, true),
        Some(Msg::ConfirmNo)
    ));
    // 'q' must not quit and '2' must not switch screens while the prompt is open
    assert!(translate(k(KeyCode::Char('q')), false, true).is_none());
    assert!(translate(k(KeyCode::Char('2')), false, true).is_none());
}

#[test]
fn angle_keys_and_arrows_walk_through_the_tabs() {
    let mut a = app_at(80, 24);
    a.update(Msg::NextScreen);
    assert_eq!(a.screen, Screen::Browser);
    a.update(Msg::PrevScreen);
    a.update(Msg::PrevScreen);
    assert_eq!(a.screen, Screen::History, "wraps around");
    // plain arrows switch tabs outside the Browser
    a.update(Msg::Right);
    assert_eq!(a.screen, Screen::Dashboard);
    a.update(Msg::Left);
    assert_eq!(a.screen, Screen::History);
    // the Browser's file table switches tabs too
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::Right);
    assert_eq!(a.screen, Screen::Duplicates);
    a.update(Msg::Left);
    assert_eq!(a.screen, Screen::Browser);
    // but with the folder tree focused ←/→ collapse and expand
    a.update(Msg::Tab);
    assert_eq!(a.browser.focus, Focus::Tree);
    a.update(Msg::Left);
    a.update(Msg::Right);
    assert_eq!(a.screen, Screen::Browser);
    // < and > always switch
    a.update(Msg::NextScreen);
    assert_eq!(a.screen, Screen::Duplicates);
    // typing never leaves the screen: `>` inside the filter is text (see keymap tests)
}

#[test]
fn every_screen_shows_a_loader_in_the_header_while_scanning() {
    for screen in Screen::ALL {
        let mut a = app_at(80, 24);
        a.update(Msg::Goto(screen));
        a.update(Msg::Rescan);
        a.update(Msg::Core(CoreEvent::Progress {
            task: TaskKind::Scan,
            done: 1200,
            total: None,
            bytes: 0,
            current: None,
        }));
        let out = draw(&a, 80, 24);
        let head = out.lines().next().unwrap();
        assert!(head.contains("scanning 1,200 files"), "{screen:?}: {head}");
        assert!(
            head.contains('░') || head.contains('█'),
            "{screen:?}: {head}"
        );
        assert!(
            screens_spinner().iter().any(|c| head.contains(*c)),
            "{screen:?}: {head}"
        );
    }
}

fn screens_spinner() -> [char; 10] {
    crate::screens::dashboard::SPINNER
}

#[test]
fn escape_in_the_file_browser_hands_the_arrows_to_the_tabs() {
    let mut a = app_at(80, 24);
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::Tab); // folder tree: arrows would collapse / expand
    assert_eq!(a.browser.focus, Focus::Tree);
    a.update(Msg::Back);
    assert!(a.tab_mode);
    insta::assert_snapshot!("tab_mode_80x24", draw(&a, 80, 24));
    a.update(Msg::Right);
    assert_eq!(a.screen, Screen::Duplicates);
    assert!(a.tab_mode, "stays in tab mode so you can keep going");
    a.update(Msg::Left);
    a.update(Msg::Left);
    assert_eq!(a.screen, Screen::Dashboard);
    // Enter / Esc go back into the screen
    a.update(Msg::Enter);
    assert!(!a.tab_mode);
    // `T` does the same, also while a scan runs (Esc would stop the scan)
    a.update(Msg::Goto(Screen::Browser));
    a.update(Msg::Rescan);
    a.update(Msg::TabMode);
    assert!(a.tab_mode);
    a.update(Msg::Back);
    assert!(!a.tab_mode);
    assert!(!a.dashboard.scan.cancelling, "Esc only left tab mode");
    // keys inside the screen are ignored meanwhile
    a.update(Msg::TabMode);
    let before = a.browser.query.clone();
    a.update(Msg::CycleKind);
    assert_eq!(a.browser.query, before);
}

#[test]
fn force_asks_at_start_up_and_waits_for_the_answer() {
    // `--force`: the prompt is open from the first frame and nothing scans underneath it
    let mut a = app_at(80, 24);
    a.confirm_flush = true;
    insta::assert_snapshot!("force_prompt_at_start_80x24", draw(&a, 80, 24));
    assert!(a.update(Msg::Bg(Bg::Worker(Response::NoIndex))).is_empty());
    assert!(!a.dashboard.scan.running);
    // N: carries on as without the flag, so a never-scanned folder is scanned now
    let cmds = a.update(Msg::ConfirmNo);
    assert!(matches!(cmds.as_slice(), [Cmd::StartScan { force: false }]));
}

#[test]
fn force_yes_wipes_and_scans_once() {
    let mut a = app_at(80, 24);
    a.confirm_flush = true;
    // an existing, recent scan: N does nothing, Y wipes
    a.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(
        stats_scanned_at(now() - 60),
    )))));
    assert!(!a.dashboard.scan.running);
    let cmds = a.update(Msg::ConfirmYes);
    assert!(matches!(cmds.as_slice(), [Cmd::StartScan { force: true }]));
    // later answers never start a second scan
    assert!(a.update(Msg::Bg(Bg::Worker(Response::NoIndex))).is_empty());
    let mut b = app_at(80, 24);
    b.confirm_flush = true;
    b.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(
        stats_scanned_at(now() - 60),
    )))));
    assert!(b.update(Msg::ConfirmNo).is_empty());
    assert!(!b.dashboard.scan.running);
}

#[test]
fn enter_on_a_file_asks_the_os_to_open_it() {
    let mut a = loaded_browser(80, 24);
    a.browser.focus = Focus::Table;
    let first = a.browser.current_row().unwrap().clone();
    let cmds = a.update(Msg::Enter);
    match cmds.as_slice() {
        [Cmd::OpenFile(p)] => assert_eq!(*p, first.path),
        other => panic!("{other:?}"),
    }
    assert!(a.draw_footer_has("Opening"));
}

#[test]
fn enter_in_the_folder_tree_only_moves_to_the_files() {
    let mut a = loaded_browser(80, 24);
    a.browser.focus = Focus::Tree;
    assert!(a.update(Msg::Enter).is_empty());
    assert_eq!(a.browser.focus, Focus::Table);
}

#[test]
fn icloud_only_files_are_never_opened() {
    let mut a = loaded_browser(80, 24);
    a.browser.focus = Focus::Table;
    if let Some(w) = a.browser.window.as_mut() {
        w.rows[0].cloud_only = true;
    }
    assert!(a.update(Msg::Enter).is_empty());
    assert!(a.draw_footer_has("only in iCloud"));
}

#[test]
fn the_footer_message_goes_away_by_itself() {
    let mut a = loaded_browser(80, 24);
    a.browser.focus = Focus::Table;
    a.update(Msg::Enter);
    assert!(a.notice.is_some());
    for _ in 0..12 {
        a.update(Msg::Tick);
    }
    assert!(a.notice.is_none());
    assert!(a.dirty, "the footer is redrawn without the message");
    // and the OS failing to open a file shows up too
    a.update(Msg::Bg(Bg::Notice(
        "The system could not open that file".into(),
    )));
    assert!(a.draw_footer_has("could not open"));
}

impl App {
    fn draw_footer_has(&self, text: &str) -> bool {
        let (w, h) = self.size;
        draw(self, w, h).lines().last().unwrap().contains(text)
    }
}

#[test]
fn the_dashboard_lists_top_places_with_a_summary() {
    let mut a = app_at(80, 24);
    a.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(stats())))));
    let out = draw(&a, 80, 24);
    assert!(out.contains("Top places"), "{out}");
    assert!(out.contains("Chennai, India"), "{out}");
    assert!(out.contains("3 countries"), "{out}");
    // the summary line is cut to the panel width; a wide terminal shows all of it
    let wide = {
        let mut w = app_at(160, 48);
        w.update(Msg::Bg(Bg::Worker(Response::Stats(Box::new(stats())))));
        draw(&w, 160, 48)
    };
    assert!(wide.contains("3 countries · 3,071 without place"), "{wide}");
    // before any statistics arrive it says so instead of showing nothing
    let b = app_at(80, 24);
    assert!(draw(&b, 80, 24).contains("Top places"));
}

// ---- Duplicates -------------------------------------------------------------------------

use yojaka_core::dupes::{DupFile, DupGroup, DupItem, DupReport, DupStats, Tier};
use yojaka_core::hash::ContentHash;

use crate::dupes::{Pane, Phase};
use crate::worker::DupOutcome;

fn dfile(id: i64, path: &str, size: u64, mtime: i64, kind: MediaKind) -> DupFile {
    DupFile {
        id: yojaka_core::model::FileId(id),
        path: PathBuf::from(path),
        size,
        mtime_ns: mtime,
        kind,
        taken_at: None,
        width: None,
        height: None,
        duration_ms: None,
        bitrate: None,
        camera: None,
        hash: Some(ContentHash([id as u8; 32])),
    }
}

fn single(id: i64, path: &str, size: u64, mtime: i64) -> DupItem {
    DupItem {
        files: vec![dfile(id, path, size, mtime, MediaKind::Image)],
    }
}

fn report() -> DupReport {
    let g1 = DupGroup::new(
        1,
        Tier::Exact,
        vec![
            single(1, "/photos/2023/Goa/IMG_0001.jpg", 4_200_000, 10),
            single(2, "/photos/2023/Goa/IMG_0001 (1).jpg", 4_200_000, 20),
            single(3, "/photos/backup/old/IMG_0001 copy.jpg", 4_200_000, 30),
        ],
    );
    let pair = |base: i64, dir: &str, mtime: i64| DupItem {
        files: vec![
            dfile(
                base,
                &format!("{dir}/IMG_7.heic"),
                2_000_000,
                mtime,
                MediaKind::Image,
            ),
            dfile(
                base + 1,
                &format!("{dir}/IMG_7.mov"),
                6_000_000,
                mtime,
                MediaKind::Video,
            ),
        ],
    };
    let g2 = DupGroup::new(
        2,
        Tier::Exact,
        vec![pair(10, "/photos/phone", 5), pair(20, "/photos/export", 9)],
    );
    let g3 = DupGroup::new(
        3,
        Tier::Exact,
        vec![
            single(30, "/photos/a/clip.mp4", 90_000_000, 1),
            single(31, "/photos/b/clip.mp4", 90_000_000, 2),
        ],
    );
    DupReport {
        groups: vec![g3, g2, g1],
        stats: DupStats {
            candidate_files: 412,
            ..Default::default()
        },
        issues: vec![],
    }
}

/// An app with a scanned folder and a finished duplicate search.
fn with_dupes(w: u16, h: u16) -> App {
    let mut a = app_at(w, h);
    a.dashboard_stats(stats());
    a.screen = Screen::Duplicates;
    a.dupes.begin();
    a.dupes.finish(report());
    a
}

#[test]
fn duplicates_screen_in_each_state() {
    for (w, h) in SIZES {
        // not searched yet
        let mut a = app_at(w, h);
        a.dashboard_stats(stats());
        a.screen = Screen::Duplicates;
        insta::assert_snapshot!(format!("duplicates_idle_{w}x{h}"), draw(&a, w, h));

        // searching
        a.dupes.begin();
        a.dupes.progress(120, Some(412), 1_500_000_000);
        insta::assert_snapshot!(format!("duplicates_searching_{w}x{h}"), draw(&a, w, h));

        // results, rule applied
        let a = with_dupes(w, h);
        insta::assert_snapshot!(format!("duplicates_results_{w}x{h}"), draw(&a, w, h));

        // nothing found
        let mut a = app_at(w, h);
        a.dashboard_stats(stats());
        a.screen = Screen::Duplicates;
        a.dupes.begin();
        a.dupes.finish(DupReport {
            stats: DupStats {
                candidate_files: 30,
                ..Default::default()
            },
            ..Default::default()
        });
        insta::assert_snapshot!(format!("duplicates_none_{w}x{h}"), draw(&a, w, h));
    }
}

#[test]
fn duplicates_help_lists_its_own_keys_and_not_the_browsers() {
    let mut a = with_dupes(80, 40);
    a.show_help = true;
    let out = draw(&a, 80, 40);
    assert!(out.contains("mark / unmark"), "{out}");
    assert!(!out.contains("filter by name"), "{out}");
    let mut b = loaded_browser(80, 40);
    b.show_help = true;
    let out = draw(&b, 80, 40);
    assert!(out.contains("filter by name"));
    assert!(!out.contains("mark / unmark"));
}

#[test]
fn the_search_needs_a_scanned_folder_and_no_running_scan() {
    let mut a = app_at(80, 24);
    a.screen = Screen::Duplicates;
    // nothing known about the folder yet
    assert!(keys(&mut a, vec![Msg::FindDupes]).is_empty());
    assert!(!a.dupes.searching());

    a.dashboard_stats(stats());
    a.dashboard.scan.running = true;
    assert!(keys(&mut a, vec![Msg::FindDupes]).is_empty());
    assert!(a.notice.as_ref().unwrap().0.contains("scan to finish"));

    a.dashboard_stop_scan();
    let cmds = keys(&mut a, vec![Msg::FindDupes]);
    assert!(matches!(cmds.as_slice(), [Cmd::FindDupes { .. }]));
    assert!(a.dupes.searching());
    // asking again while it runs does nothing
    assert!(keys(&mut a, vec![Msg::FindDupes]).is_empty());
    // and a rescan waits for it
    assert!(keys(&mut a, vec![Msg::Rescan]).is_empty());
    assert!(a.notice.as_ref().unwrap().0.contains("duplicate search"));
}

#[test]
fn f_only_works_on_the_duplicates_screen() {
    let mut a = app_at(80, 24);
    a.dashboard_stats(stats());
    a.screen = Screen::Browser;
    assert!(keys(&mut a, vec![Msg::FindDupes]).is_empty());
    assert!(!a.dupes.searching());
}

#[test]
fn search_progress_does_not_touch_the_scan_gauge() {
    let mut a = app_at(80, 24);
    a.dashboard_stats(stats());
    a.screen = Screen::Duplicates;
    keys(&mut a, vec![Msg::FindDupes]);
    keys(
        &mut a,
        vec![
            Msg::Core(CoreEvent::TaskStarted {
                task: TaskKind::Hash,
                label: "x".into(),
            }),
            Msg::Core(CoreEvent::Progress {
                task: TaskKind::Hash,
                done: 5,
                total: Some(10),
                bytes: 99,
                current: None,
            }),
        ],
    );
    assert!(!a.dashboard.scan.running);
    assert_eq!(a.dashboard.scan.phase, None);
    assert_eq!(a.dupes.done, 5);
    assert_eq!(a.status_line(), "comparing files 50%");
    // the loader shows on every screen
    a.screen = Screen::Dashboard;
    assert!(
        draw(&a, 80, 24)
            .lines()
            .next()
            .unwrap()
            .contains("comparing files 50%")
    );
}

#[test]
fn escape_and_c_stop_the_search_and_the_result_is_kept_out() {
    for stop in [Msg::Back, Msg::CancelScan, Msg::Interrupt] {
        let mut a = app_at(80, 24);
        a.dashboard_stats(stats());
        a.screen = Screen::Duplicates;
        keys(&mut a, vec![Msg::FindDupes]);
        let cmds = keys(&mut a, vec![stop]);
        assert!(matches!(cmds.as_slice(), [Cmd::CancelDupes]));
        keys(
            &mut a,
            vec![Msg::Bg(Bg::DupesFinished(DupOutcome::Cancelled))],
        );
        assert_eq!(a.dupes.phase, Phase::Cancelled);
        assert!(!a.should_quit);
    }
}

#[test]
fn a_failed_search_says_why() {
    let mut a = app_at(80, 24);
    a.dashboard_stats(stats());
    a.screen = Screen::Duplicates;
    keys(&mut a, vec![Msg::FindDupes]);
    keys(
        &mut a,
        vec![Msg::Bg(Bg::DupesFinished(DupOutcome::Failed(
            "disk on fire".into(),
        )))],
    );
    assert!(draw(&a, 80, 24).contains("disk on fire"));
}

#[test]
fn marking_session() {
    let mut a = with_dupes(80, 24);
    // Biggest group first; the default rule keeps the oldest copy.
    assert_eq!(a.dupes.groups[0].id, 3);
    assert_eq!(a.dupes.groups[0].delete, [false, true]);
    let total = a.dupes.total_reclaim();
    assert_eq!(total, 90_000_000 + 8_000_000 + 8_400_000);
    assert_eq!(a.dupes.marked_files(), 1 + 2 + 2);

    // Move to group 2 (Live Photos), then into its copies.
    keys(&mut a, vec![Msg::Down]);
    assert_eq!(a.dupes.group, 1);
    keys(&mut a, vec![Msg::Tab]);
    assert_eq!(a.dupes.pane, Pane::Copies);
    // Space on the kept copy would leave none: refused.
    assert_eq!(a.dupes.groups[1].delete, [false, true]);
    keys(&mut a, vec![Msg::Toggle]);
    assert_eq!(a.dupes.groups[1].delete, [false, true]);
    assert!(a.notice.as_ref().unwrap().0.contains("always stays"));

    // Unmark the other copy: keep both.
    keys(&mut a, vec![Msg::Down, Msg::Toggle]);
    assert_eq!(a.dupes.groups[1].delete, [false, false]);
    // Enter on the second copy keeps only it.
    keys(&mut a, vec![Msg::Enter]);
    assert_eq!(a.dupes.groups[1].delete, [true, false]);
    // x keeps everything in the group.
    keys(&mut a, vec![Msg::KeepAllInGroup]);
    assert_eq!(a.dupes.groups[1].delete, [false, false]);

    // The cursor never leaves the lists.
    keys(&mut a, vec![Msg::Bottom, Msg::Down, Msg::Down]);
    assert_eq!(a.dupes.copy, 1);
    keys(&mut a, vec![Msg::Tab, Msg::Bottom, Msg::Down]);
    assert_eq!(a.dupes.group, 2);
    keys(&mut a, vec![Msg::Top, Msg::Up]);
    assert_eq!(a.dupes.group, 0);

    // p cycles the rule and applies it to every group again.
    keys(&mut a, vec![Msg::NextRule]);
    assert_eq!(a.dupes.preset, 1);
    assert!(a.notice.as_ref().unwrap().0.starts_with("Keep rule"));
    for g in &a.dupes.groups {
        assert_eq!(g.delete_count(), g.members.len() - 1);
    }
}

/// A photo with the facts the comparison shows.
fn shot(id: i64, path: &str, size: u64, pixels: (u32, u32), taken: i64) -> DupItem {
    let mut f = dfile(id, path, size, 50, MediaKind::Image);
    f.hash = None;
    f.width = Some(pixels.0);
    f.height = Some(pixels.1);
    f.taken_at = Some(taken);
    f.camera = Some("iPhone 15".into());
    DupItem { files: vec![f] }
}

/// The exact groups of `report()` plus two likely groups and a name group.
fn tiered_report() -> DupReport {
    let mut r = report();
    let taken = 1_700_000_000;
    r.groups.push(DupGroup::new(
        4,
        Tier::Likely,
        vec![
            shot(
                40,
                "/photos/2023/Goa/IMG_0042.jpg",
                3_800_000,
                (4032, 3024),
                taken,
            ),
            shot(
                41,
                "/photos/shared/IMG_0042_edited.jpg",
                1_100_000,
                (4032, 3024),
                taken,
            ),
        ],
    ));
    r.groups.push(DupGroup::new(
        5,
        Tier::Likely,
        vec![
            shot(
                42,
                "/photos/a/burst_1.jpg",
                2_000_000,
                (4032, 3024),
                taken + 9,
            ),
            shot(
                43,
                "/photos/a/burst_2.jpg",
                2_100_000,
                (4032, 3024),
                taken + 9,
            ),
        ],
    ));
    r.groups.push(DupGroup::new(
        6,
        Tier::Name,
        vec![
            shot(
                44,
                "/photos/x/DSC_0001.jpg",
                5_000_000,
                (6000, 4000),
                taken + 100,
            ),
            shot(
                45,
                "/photos/y/DSC_0001 (1).jpg",
                5_100_000,
                (6000, 4000),
                taken + 9000,
            ),
        ],
    ));
    r
}

fn with_tiers(w: u16, h: u16) -> App {
    let mut a = app_at(w, h);
    a.dashboard_stats(stats());
    a.screen = Screen::Duplicates;
    a.dupes.begin();
    a.update(Msg::Bg(Bg::DupesFinished(DupOutcome::Done(Box::new(
        tiered_report(),
    )))));
    a
}

#[test]
fn look_alike_groups_are_never_marked_by_a_rule() {
    let mut a = with_tiers(80, 24);
    assert!(
        a.notice
            .as_ref()
            .unwrap()
            .0
            .contains("3 groups are not identical")
    );
    assert_eq!(a.dupes.undecided(), 3);
    let marks =
        |a: &App| -> Vec<usize> { a.dupes.groups.iter().map(|g| g.delete_count()).collect() };
    assert_eq!(marks(&a), [1, 1, 2, 0, 0, 0]);
    // Every rule, twice round: still nothing marked outside the identical groups.
    for _ in 0..crate::dupes::PRESETS.len() * 2 {
        keys(&mut a, vec![Msg::NextRule]);
        assert_eq!(marks(&a), [1, 1, 2, 0, 0, 0]);
    }

    // v shows one tier at a time, and the cursor stays inside what is shown.
    keys(&mut a, vec![Msg::CycleTier]);
    assert_eq!(a.dupes.filter, Some(Tier::Exact));
    keys(&mut a, vec![Msg::CycleTier]);
    assert_eq!(a.dupes.filter, Some(Tier::Likely));
    assert_eq!(a.dupes.visible(), [3, 4]);
    assert_eq!(a.dupes.current().unwrap().id, 4);
    keys(&mut a, vec![Msg::Up, Msg::Up]);
    assert_eq!(a.dupes.current().unwrap().id, 4);
    keys(&mut a, vec![Msg::Bottom, Msg::Down]);
    assert_eq!(a.dupes.current().unwrap().id, 5);

    // Marking by hand, in the group under the cursor, works: keep the first burst shot.
    keys(&mut a, vec![Msg::Enter, Msg::Enter]);
    assert_eq!(a.dupes.groups[4].delete, [false, true]);
    assert_eq!(a.dupes.undecided(), 2);
    // A rule change leaves the hand-made mark alone.
    keys(&mut a, vec![Msg::NextRule]);
    assert_eq!(a.dupes.groups[4].delete, [false, true]);

    keys(&mut a, vec![Msg::CycleTier]);
    assert_eq!(a.dupes.filter, Some(Tier::Name));
    assert_eq!(a.dupes.current().unwrap().id, 6);
    keys(&mut a, vec![Msg::CycleTier]);
    assert_eq!(a.dupes.filter, None);
    assert_eq!(a.dupes.visible().len(), 6);
}

#[test]
fn look_alike_groups_show_a_comparison() {
    for (w, h) in SIZES {
        let mut a = with_tiers(w, h);
        keys(&mut a, vec![Msg::CycleTier, Msg::CycleTier]);
        let screen = draw(&a, w, h);
        assert!(screen.contains("3 exact · 2 likely · 1 name"), "{screen}");
        assert!(screen.contains("showing likely"), "{screen}");
        assert!(screen.contains("? review"), "{screen}");
        // Both files, their sizes and what they share.
        for fact in [
            "3.8 MB",
            "1.1 MB",
            "4032×3024",
            "2023-11-14 22:13",
            "pixels",
        ] {
            assert!(screen.contains(fact), "{fact} missing:\n{screen}");
        }
        if w >= 100 {
            assert!(screen.contains("iPhone 15"), "{screen}");
        }
        insta::assert_snapshot!(format!("duplicates_likely_{w}x{h}"), screen);

        // After a decision the rows say keep / delete.
        keys(&mut a, vec![Msg::Enter, Msg::Enter]);
        let screen = draw(&a, w, h);
        assert!(screen.contains("✓ keep") && screen.contains("✕ delete"));
        assert!(!screen.contains("? review"));
    }
}

#[test]
fn reviewing_a_plan_with_look_alikes_says_so() {
    let mut a = with_tiers(100, 30);
    a.mode = Mode::Write;
    // Only identical copies are marked: no warning.
    keys(&mut a, vec![Msg::ReviewPlan]);
    let screen = draw(&a, 100, 30);
    assert!(screen.contains("Review"), "{screen}");
    assert!(!screen.contains("not identical to the copy"), "{screen}");
    keys(&mut a, vec![Msg::Back]);
    assert!(a.modal.is_none());

    // Mark one look-alike by hand: the review names it.
    keys(
        &mut a,
        vec![Msg::CycleTier, Msg::CycleTier, Msg::Enter, Msg::Enter],
    );
    keys(&mut a, vec![Msg::ReviewPlan]);
    let screen = draw(&a, 100, 30);
    assert!(
        screen.contains("1 of these are not identical to the copy that stays"),
        "{screen}"
    );
}

#[test]
fn f_and_shift_f_ask_for_different_searches() {
    let mut a = app_at(80, 24);
    a.dashboard_stats(stats());
    a.screen = Screen::Duplicates;
    let cmds = keys(&mut a, vec![Msg::FindDupes]);
    assert!(matches!(
        cmds.as_slice(),
        [Cmd::FindDupes { similar: false }]
    ));
    a.update(Msg::Bg(Bg::DupesFinished(DupOutcome::Cancelled)));
    let cmds = keys(&mut a, vec![Msg::FindSimilar]);
    assert!(matches!(
        cmds.as_slice(),
        [Cmd::FindDupes { similar: true }]
    ));

    // Pictures that cannot be compared by look are mentioned when the search ends.
    let mut report = tiered_report();
    report.stats.pictures_hashed = 900;
    report.stats.pictures_unsupported = 120;
    a.update(Msg::Bg(Bg::DupesFinished(DupOutcome::Done(Box::new(
        report,
    )))));
    assert!(
        a.notice
            .as_ref()
            .unwrap()
            .0
            .contains("120 pictures are in a format that cannot be compared")
    );
}

#[test]
fn every_group_always_keeps_a_copy_whatever_is_pressed() {
    let mut a = with_dupes(80, 24);
    let presses = [
        Msg::Toggle,
        Msg::Down,
        Msg::Toggle,
        Msg::Tab,
        Msg::Toggle,
        Msg::Down,
        Msg::Toggle,
        Msg::Enter,
        Msg::Tab,
        Msg::Down,
        Msg::Toggle,
        Msg::NextRule,
        Msg::Toggle,
        Msg::KeepAllInGroup,
    ];
    for m in presses {
        keys(&mut a, vec![m]);
        for g in &a.dupes.groups {
            assert!(g.keeper().is_some());
        }
    }
}

#[test]
fn a_finished_scan_forgets_stale_results() {
    let mut a = with_dupes(80, 24);
    assert!(!a.dupes.groups.is_empty());
    let sum = yojaka_core::scan::ScanSummary {
        scan_id: 1,
        status: yojaka_core::scan::ScanStatus::Complete,
        files_seen: 0,
        files_indexed: 0,
        new_or_changed: 0,
        unchanged: 0,
        pruned: 0,
        cloud_only: 0,
        bytes: 0,
        issues: 0,
        permission_denied: 0,
        elapsed_ms: 0,
        metadata: None,
    };
    keys(&mut a, vec![Msg::Bg(Bg::ScanFinished(Ok(sum)))]);
    assert!(a.dupes.groups.is_empty());
    assert_eq!(a.dupes.phase, Phase::Idle);
}

#[test]
fn nothing_the_duplicates_screen_does_asks_the_runtime_to_change_a_file() {
    // The only commands it can produce are reading ones.
    let mut a = with_dupes(80, 24);
    let cmds = keys(
        &mut a,
        vec![
            Msg::Toggle,
            Msg::Enter,
            Msg::NextRule,
            Msg::KeepAllInGroup,
            Msg::FindDupes,
        ],
    );
    assert!(
        cmds.iter().all(|c| matches!(c, Cmd::FindDupes { .. })),
        "{cmds:?}"
    );
}

// ---- write mode, review, history -----------------------------------------------------------

mod write_path {
    use yojaka_core::exec::{ApplyReport, RunStatus, RunSummary};
    use yojaka_core::plan::{OpMode, PlanKind};

    use super::*;
    use crate::modal::Modal;
    use crate::worker::{Bg, ExecOutcome};

    fn write_app(w: u16, h: u16) -> App {
        let mut a = with_dupes(w, h);
        a.update(Msg::ToggleWrite);
        a.update(Msg::Char('y'));
        a
    }

    fn run_summary(done: usize, status: RunStatus) -> RunSummary {
        RunSummary {
            plan_id: uuid::Uuid::now_v7(),
            started: 1_800_000_000,
            root: "/photos".into(),
            kind: PlanKind::Dedupe,
            mode: OpMode::Trash,
            ops: done,
            done,
            skipped: 0,
            failed: 0,
            bytes: 12_000_000,
            status,
        }
    }

    #[test]
    fn write_mode_is_asked_for_and_can_be_turned_off() {
        let mut a = with_dupes(80, 24);
        a.update(Msg::ToggleWrite);
        assert!(matches!(
            a.modal,
            Some(Modal::EnableWrite { then_review: false })
        ));
        assert_eq!(a.mode, Mode::ReadOnly, "still off until confirmed");
        assert!(draw(&a, 80, 24).contains("Turn on write mode?"));
        a.update(Msg::Char('n'));
        assert!(a.modal.is_none() && a.mode == Mode::ReadOnly);

        a.update(Msg::ToggleWrite);
        a.update(Msg::Char('y'));
        assert_eq!(a.mode, Mode::Write);
        assert!(draw(&a, 80, 24).lines().next().unwrap().contains("[WRITE]"));
        a.update(Msg::ToggleWrite);
        assert_eq!(a.mode, Mode::ReadOnly);
    }

    #[test]
    fn review_in_read_only_mode_asks_for_write_mode_first() {
        let mut a = with_dupes(80, 24);
        let cmds = a.update(Msg::ReviewPlan);
        assert!(cmds.is_empty());
        assert!(matches!(
            a.modal,
            Some(Modal::EnableWrite { then_review: true })
        ));
        assert_eq!(a.mode, Mode::ReadOnly, "still off until confirmed");
        assert!(draw(&a, 80, 24).contains("turn on and review"));
        a.update(Msg::Char('n'));
        assert!(a.modal.is_none() && a.mode == Mode::ReadOnly);

        a.update(Msg::ReviewPlan);
        a.update(Msg::Enter);
        assert_eq!(a.mode, Mode::Write);
        assert!(
            matches!(a.modal, Some(Modal::Review(_))),
            "the review follows"
        );
    }

    #[test]
    fn a_plan_can_be_saved_in_read_only_mode_without_a_review() {
        let mut a = with_dupes(100, 40);
        assert_eq!(a.mode, Mode::ReadOnly);
        // Nothing is marked yet by the copy-rule: mark one, then save.
        let marked = a.dupes.marked_files();
        assert!(marked > 0, "the fixture marks some copies");
        let cmds = a.update(Msg::SavePlan);
        assert!(matches!(&cmds[..], [Cmd::SavePlan(p)] if p.ops.len() == marked));
        assert!(a.modal.is_none(), "saving never opens the review box");
    }

    #[test]
    fn a_small_plan_is_confirmed_with_y_and_runs_once() {
        let mut a = write_app(100, 40);
        let marked = a.dupes.marked_files();
        assert!(marked > 0);
        a.update(Msg::ReviewPlan);
        assert!(matches!(a.modal, Some(Modal::Review(_))));
        let out = draw(&a, 100, 40);
        assert!(out.contains("to the Trash"), "{out}");
        assert!(out.contains("y / Enter"), "{out}");

        let cmds = a.update(Msg::Char('y'));
        assert!(matches!(&cmds[..], [Cmd::ApplyPlan(p)] if p.ops.len() == marked));
        assert!(matches!(a.modal, Some(Modal::Running(_))));
        // Keys do nothing while it runs, except Ctrl-C which asks it to stop.
        assert!(a.update(Msg::Char('y')).is_empty());
        let stop = a.update(Msg::Interrupt);
        assert!(matches!(&stop[..], [Cmd::CancelExec]));
    }

    #[test]
    fn escape_leaves_the_review_without_running_anything() {
        let mut a = write_app(100, 40);
        a.update(Msg::ReviewPlan);
        assert!(a.update(Msg::Back).is_empty());
        assert!(a.modal.is_none());
    }

    #[test]
    fn a_large_plan_needs_its_phrase_typed() {
        let mut a = write_app(100, 40);
        a.update(Msg::ReviewPlan);
        let Some(Modal::Review(r)) = &mut a.modal else {
            panic!("no review");
        };
        // Pretend the plan is big.
        let first = r.plan.ops[0].clone();
        while r.plan.ops.len() < yojaka_core::plan::Plan::LARGE_PLAN_FILES {
            r.plan.ops.push(first.clone());
        }
        let phrase = r.plan.confirm_phrase();
        // 'y' is text now, not a shortcut, and Enter does nothing until the phrase matches.
        assert!(a.update(Msg::Char('y')).is_empty());
        assert!(a.update(Msg::Enter).is_empty());
        assert!(matches!(a.modal, Some(Modal::Review(_))));
        assert!(draw(&a, 100, 40).contains("Type \"trash"));
        a.update(Msg::Backspace);
        for c in phrase.chars() {
            a.update(Msg::Char(c));
        }
        let cmds = a.update(Msg::Enter);
        assert!(matches!(&cmds[..], [Cmd::ApplyPlan(_)]));
    }

    #[test]
    fn when_a_run_ends_the_report_shows_and_everything_stale_is_dropped() {
        let mut a = write_app(100, 40);
        a.update(Msg::ReviewPlan);
        a.update(Msg::Char('y'));
        let report = ApplyReport {
            trashed: 3,
            bytes: 9_000_000,
            ..Default::default()
        };
        let cmds = a.update(Msg::Bg(Bg::ExecFinished(ExecOutcome::Applied(Box::new(
            report,
        )))));
        assert!(matches!(a.modal, Some(Modal::Report(_))));
        assert!(a.dupes.groups.is_empty(), "the old results are gone");
        assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadHistory)));
        assert!(cmds.iter().any(|c| matches!(c, Cmd::StartScan { .. })));
        let out = draw(&a, 100, 40);
        assert!(out.contains("3 files moved to the Trash"), "{out}");
        a.update(Msg::Char('x'));
        assert!(a.modal.is_none());
    }

    #[test]
    fn a_failed_run_says_so_and_leaves_the_files_alone() {
        let mut a = write_app(100, 40);
        a.modal = Some(Modal::Running(crate::modal::RunView {
            label: "x",
            done: 0,
            total: None,
            cancelling: false,
        }));
        a.update(Msg::Bg(Bg::ExecFinished(ExecOutcome::Failed(
            "plan was already applied".into(),
        ))));
        assert!(draw(&a, 100, 40).contains("already applied"));
    }

    #[test]
    fn history_loads_on_entry_lists_runs_and_says_there_is_no_undo() {
        let mut a = app_at(100, 40);
        let cmds = a.update(Msg::Goto(Screen::History));
        assert!(matches!(&cmds[..], [Cmd::LoadHistory]));
        assert!(draw(&a, 100, 40).contains("Loading"));
        a.update(Msg::Bg(Bg::History(vec![
            run_summary(10, RunStatus::Applied),
            run_summary(4, RunStatus::Incomplete),
        ])));
        let out = draw(&a, 100, 40);
        assert!(
            out.contains("applied") && out.contains("incomplete"),
            "{out}"
        );
        assert!(out.contains("no undo"), "{out}");

        a.update(Msg::Down);
        assert_eq!(a.history.selected, 1);
        a.update(Msg::Up);
    }

    #[test]
    fn the_new_screens_and_boxes_render_at_every_small_size() {
        for (w, h) in [(20, 6), (40, 12), (60, 16), (80, 24), (120, 40)] {
            let mut a = with_dupes(w, h);
            a.mode = Mode::Write;
            a.update(Msg::ReviewPlan);
            let _ = draw(&a, w, h);
            a.modal = Some(Modal::EnableWrite { then_review: true });
            let _ = draw(&a, w, h);
            a.modal = Some(Modal::Report(vec![crate::modal::ReportLine {
                text: "a very long line ".repeat(10),
                bad: true,
            }]));
            let _ = draw(&a, w, h);
            a.modal = None;
            a.screen = Screen::History;
            let _ = draw(&a, w, h);
            a.update(Msg::Bg(Bg::History(vec![run_summary(
                3,
                RunStatus::Incomplete,
            )])));
            let _ = draw(&a, w, h);
        }
    }

    #[test]
    fn help_describes_the_write_keys() {
        let lines = crate::screens::help::lines_for(Screen::Duplicates);
        assert!(lines.iter().any(|(k, _)| *k == "W"));
        assert!(lines.iter().any(|(k, _)| *k == "a"));
    }
}

mod organize_screen {
    use yojaka_core::model::Fingerprint;
    use yojaka_core::organize::{OrganizePlan, OrganizeStats, Transfer};
    use yojaka_core::plan::{Op, OpMode, Plan, PlanKind};

    use super::*;
    use crate::modal::Modal;
    use crate::organize::{Editing, Phase, Row};
    use crate::worker::Bg;

    fn organize_app(w: u16, h: u16) -> App {
        let mut a = app_at(w, h);
        a.dashboard_stats(stats());
        a.screen = Screen::Organize;
        a
    }

    fn type_text(a: &mut App, s: &str) {
        for c in s.chars() {
            a.update(Msg::Char(c));
        }
    }

    fn organize_plan(n: usize) -> Box<OrganizePlan> {
        let mut p = Plan::from_dupes(std::path::Path::new("/photos"), &[]);
        p.kind = PlanKind::Organize;
        p.mode = OpMode::Copy;
        p.dest = Some("/out".into());
        for i in 0..n {
            p.ops.push(Op::Copy {
                id: i as u64,
                src: format!("/photos/IMG_{i}.jpg").into(),
                dst: format!("/out/2023/08/IMG_{i}.jpg").into(),
                expect: Fingerprint {
                    size: 1000,
                    mtime_ns: 0,
                    hash: None,
                },
                inferred: i == 1,
                pair: None,
            });
        }
        p.summary.bytes = 1000 * n as u64;
        Box::new(OrganizePlan {
            plan: p,
            stats: OrganizeStats {
                files: n,
                with_place: n / 2,
                inferred: 1,
            },
        })
    }

    /// Moves the form cursor to `row`.
    fn select(a: &mut App, row: Row) {
        a.update(Msg::Top);
        while a.organize.row() != row {
            a.update(Msg::Down);
        }
    }

    /// Lets the first plan finish, so the next change starts a fresh one.
    fn planned(a: &mut App, n: usize) {
        a.update(Msg::Tick);
        a.update(Msg::Bg(Bg::Planned(Ok(organize_plan(n)))));
        assert_eq!(a.organize.phase, Phase::Ready);
    }

    #[test]
    fn the_screen_shows_the_settings_at_every_size() {
        for (w, h) in [(40, 12), (80, 24), (120, 40)] {
            let a = organize_app(w, h);
            let out = draw(&a, w, h);
            if w >= 80 {
                for text in [
                    "{year}/{month_name}/{name}",
                    "Yojaka Organized",
                    "‹ Date ›",
                    "‹ Copy ›",
                    "‹ No Location folder ›",
                    "Destination",
                    "▶ Layout",
                ] {
                    assert!(out.contains(text), "{text}: {out}");
                }
                let footer = out.lines().last().unwrap();
                assert!(footer.contains("choose") && footer.contains("review & run"));
            }
        }
    }

    #[test]
    fn arrows_choose_a_row_and_enter_or_left_right_change_it() {
        let mut a = organize_app(100, 30);
        a.update(Msg::Enter); // Layout: the next ready-made pattern
        assert!(a.organize.template.contains("{country}"));
        assert_eq!(a.organize.preset_label(), "Place");
        a.update(Msg::Left);
        assert_eq!(a.organize.preset_label(), "Date");
        assert_eq!(a.screen, Screen::Organize, "← → stay on the screen");

        select(&mut a, Row::Action);
        a.update(Msg::Right);
        assert_eq!(a.organize.transfer, Transfer::Move);
        assert!(draw(&a, 100, 30).contains("‹ Move ›"));
        select(&mut a, Row::NoGps);
        a.update(Msg::Enter);
        assert!(a.organize.infer);
        assert!(
            draw(&a, 100, 30).contains("not used"),
            "the date pattern has no place folders"
        );

        // On a text row ← → do nothing, not even change the screen.
        select(&mut a, Row::Dest);
        a.update(Msg::Right);
        assert_eq!(a.screen, Screen::Organize);
        assert!(a.organize.editing.is_none());
    }

    #[test]
    fn the_pattern_can_be_typed_cleared_or_the_edit_dropped() {
        let mut a = organize_app(100, 30);
        select(&mut a, Row::Pattern);
        a.update(Msg::Enter);
        assert_eq!(a.organize.editing, Some(Editing::Template));
        let out = draw(&a, 100, 30);
        assert!(out.lines().last().unwrap().contains("Ctrl-U"), "{out}");
        type_text(&mut a, "/x");
        a.update(Msg::FilterCancel);
        assert_eq!(
            a.organize.template,
            yojaka_core::organize::PRESETS[0].1,
            "Esc drops the edit"
        );

        a.update(Msg::Enter);
        a.update(Msg::ClearText);
        type_text(&mut a, "{year}/{name}");
        a.update(Msg::FilterAccept);
        assert_eq!(a.organize.template, "{year}/{name}");
        assert_eq!(a.organize.preset_label(), "Custom");
    }

    #[test]
    fn a_bad_template_is_explained_and_nothing_is_planned() {
        let mut a = organize_app(100, 30);
        a.organize.template = "{nope}/{name}".into();
        assert!(a.update(Msg::Tick).is_empty());
        let out = draw(&a, 100, 30);
        assert!(out.contains("unknown placeholder"), "{out}");
        assert!(out.contains("Fix the setting"), "{out}");
        assert!(
            a.update(Msg::Tick).is_empty(),
            "not retried until it changes"
        );

        a.organize.template = "{year}/{name}".into();
        a.organize.dest = String::new();
        a.organize.invalidate();
        a.update(Msg::Tick);
        assert!(draw(&a, 100, 30).contains("destination folder"));
    }

    #[test]
    fn the_preview_follows_the_settings() {
        let mut a = organize_app(100, 40);
        let cmds = a.update(Msg::Tick);
        assert!(matches!(&cmds[..], [Cmd::PlanOrganize(r)] if r.transfer == Transfer::Copy));
        assert_eq!(a.organize.phase, Phase::Planning);
        assert!(draw(&a, 100, 40).contains("Working out"));
        assert!(
            a.update(Msg::Tick).is_empty(),
            "no second plan while one runs"
        );

        a.update(Msg::Bg(Bg::Planned(Ok(organize_plan(6)))));
        assert_eq!(a.organize.phase, Phase::Ready);
        let out = draw(&a, 100, 40);
        assert!(out.contains("6 files (6.0 KB) will be copied"), "{out}");
        assert!(out.contains("IMG_0.jpg"), "{out}");
        assert!(out.contains("2023/08"), "{out}");
        assert!(out.contains("≈"), "the inferred example is marked: {out}");

        // A change plans again by itself.
        select(&mut a, Row::Action);
        let cmds = a.update(Msg::Enter);
        assert!(matches!(&cmds[..], [Cmd::PlanOrganize(r)] if r.transfer == Transfer::Move));
        assert!(a.organize.plan.is_none());
    }

    #[test]
    fn a_change_while_planning_drops_the_old_plan_and_plans_again() {
        let mut a = organize_app(100, 40);
        a.update(Msg::Tick);
        select(&mut a, Row::Action);
        let cmds = a.update(Msg::Enter);
        assert!(matches!(&cmds[..], [Cmd::CancelPlan]), "{cmds:?}");
        // The plan for the old settings arrives anyway: it is not shown, and a new one starts.
        let cmds = a.update(Msg::Bg(Bg::Planned(Ok(organize_plan(3)))));
        assert!(a.organize.plan.is_none());
        assert!(matches!(&cmds[..], [Cmd::PlanOrganize(r)] if r.transfer == Transfer::Move));
    }

    #[test]
    fn nothing_is_planned_before_a_scan_or_while_one_runs() {
        let mut a = organize_app(100, 30);
        a.dashboard.scan.running = true;
        assert!(a.update(Msg::Tick).is_empty());
        assert!(draw(&a, 100, 30).contains("Waiting for the scan"));
        a.dashboard.scan.running = false;
        assert!(matches!(&a.update(Msg::Tick)[..], [Cmd::PlanOrganize(_)]));

        let mut b = app_at(100, 30);
        b.dashboard.no_index = true;
        b.update(Msg::Goto(Screen::Organize));
        assert!(b.update(Msg::Tick).is_empty());
        assert!(draw(&b, 100, 30).contains("Scan this folder first"));
    }

    #[test]
    fn a_failed_plan_says_why_and_is_not_retried_by_itself() {
        let mut a = organize_app(100, 40);
        a.update(Msg::Tick);
        a.update(Msg::Bg(Bg::Planned(Err(
            "/System/x is a protected location".into(),
        ))));
        assert!(draw(&a, 100, 40).contains("protected location"));
        assert!(a.update(Msg::Tick).is_empty());
    }

    #[test]
    fn esc_stops_the_plan_and_a_change_starts_it_again() {
        let mut a = organize_app(100, 40);
        a.update(Msg::Tick);
        assert!(matches!(&a.update(Msg::Back)[..], [Cmd::CancelPlan]));
        a.update(Msg::Bg(Bg::Planned(Err(crate::organize::STOPPED.into()))));
        assert!(draw(&a, 100, 40).contains("Change a setting"));
        assert!(matches!(&a.update(Msg::Enter)[..], [Cmd::PlanOrganize(_)]));
    }

    #[test]
    fn esc_hands_the_arrows_to_the_tabs() {
        let mut a = organize_app(100, 30);
        planned(&mut a, 1);
        a.update(Msg::Back);
        assert!(a.tab_mode);
        a.update(Msg::Right);
        assert_eq!(a.screen, Screen::History);
    }

    #[test]
    fn review_asks_for_write_mode_then_a_copy_needs_only_a_yes() {
        let mut a = organize_app(100, 40);
        a.update(Msg::ReviewPlan);
        assert!(
            a.notice.as_ref().unwrap().0.contains("Wait"),
            "nothing planned yet"
        );
        a.update(Msg::Bg(Bg::Planned(Ok(organize_plan(4)))));

        a.update(Msg::ReviewPlan);
        assert!(matches!(
            a.modal,
            Some(Modal::EnableWrite { then_review: true })
        ));
        assert_eq!(a.mode, Mode::ReadOnly);
        a.update(Msg::Char('y'));
        assert_eq!(a.mode, Mode::Write);
        assert!(matches!(a.modal, Some(Modal::Review(_))));
        let out = draw(&a, 100, 40);
        assert!(out.contains("Copy 4 files"), "{out}");
        assert!(out.contains("The originals stay"), "{out}");
        assert!(out.contains("y / Enter"), "a copy asks yes or no: {out}");
        let cmds = a.update(Msg::Char('y'));
        assert!(matches!(&cmds[..], [Cmd::ApplyPlan(p)] if p.file_ops() == 4));
        assert!(draw(&a, 100, 40).contains("Copying files"));
    }

    #[test]
    fn a_move_needs_the_phrase_typed_even_for_two_files() {
        let mut a = organize_app(100, 40);
        a.mode = Mode::Write;
        a.update(Msg::Tick);
        let mut p = organize_plan(2);
        p.plan.mode = OpMode::Move;
        a.update(Msg::Bg(Bg::Planned(Ok(p))));
        a.update(Msg::ReviewPlan);
        assert!(draw(&a, 100, 40).contains("Type \"move 2 files\""));
        assert!(a.update(Msg::Char('y')).is_empty(), "y is text here");
        assert!(a.update(Msg::Enter).is_empty());
        a.update(Msg::Backspace);
        type_text(&mut a, "move 2 files");
        assert!(matches!(&a.update(Msg::Enter)[..], [Cmd::ApplyPlan(_)]));
    }

    #[test]
    fn a_plan_with_nothing_to_do_is_not_reviewed() {
        let mut a = organize_app(100, 40);
        a.mode = Mode::Write;
        planned(&mut a, 0);
        a.update(Msg::ReviewPlan);
        assert!(a.modal.is_none());
        assert!(draw(&a, 100, 40).contains("already in place"));
    }

    #[test]
    fn a_finished_run_clears_the_plan_and_rescans() {
        use crate::worker::ExecOutcome;
        let mut a = organize_app(100, 40);
        a.mode = Mode::Write;
        planned(&mut a, 2);
        let cmds = a.update(Msg::Bg(Bg::ExecFinished(ExecOutcome::Applied(Box::new(
            yojaka_core::exec::ApplyReport {
                copied: 2,
                bytes: 2000,
                ..Default::default()
            },
        )))));
        assert!(a.organize.plan.is_none());
        assert!(cmds.iter().any(|c| matches!(c, Cmd::StartScan { .. })));
        assert!(
            !cmds.iter().any(|c| matches!(c, Cmd::PlanOrganize(_))),
            "no plan while the scan runs"
        );
        assert!(draw(&a, 100, 40).contains("2 copied"));
    }

    #[test]
    fn typing_goes_to_the_box_and_a_tilde_is_the_home_folder() {
        let mut a = organize_app(100, 30);
        planned(&mut a, 1);
        select(&mut a, Row::Dest);
        a.update(Msg::Enter);
        a.update(Msg::ClearText);
        type_text(&mut a, "~/q");
        assert!(!a.should_quit);
        assert_eq!(a.organize.dest, "~/q");
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let cmds = a.update(Msg::FilterAccept);
        assert!(
            matches!(&cmds[..], [Cmd::PlanOrganize(r)] if r.dest == home.join("q")),
            "{cmds:?}"
        );
    }

    #[test]
    fn a_plan_that_finishes_while_you_type_is_kept() {
        let mut a = organize_app(100, 30);
        a.update(Msg::Tick);
        select(&mut a, Row::Dest);
        a.update(Msg::Enter);
        a.update(Msg::Bg(Bg::Planned(Ok(organize_plan(2)))));
        assert_eq!(a.organize.phase, Phase::Ready);
        assert!(a.organize.editing.is_some());
    }

    #[test]
    fn it_renders_in_every_state_at_small_sizes() {
        for (w, h) in [(20, 6), (40, 12), (60, 16), (80, 24)] {
            let mut a = organize_app(w, h);
            let _ = draw(&a, w, h);
            select(&mut a, Row::Pattern);
            let _ = draw(&a, w, h);
            a.update(Msg::Enter);
            let _ = draw(&a, w, h);
            a.update(Msg::FilterCancel);
            a.update(Msg::Bg(Bg::Planned(Ok(organize_plan(30)))));
            let _ = draw(&a, w, h);
            select(&mut a, Row::NoGps);
            let _ = draw(&a, w, h);
        }
    }
}
