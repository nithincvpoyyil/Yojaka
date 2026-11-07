//! `TestBackend` snapshots of every screen at 80×24 and 160×48, plus key-sequence tests
//! that drive `update()` the way the event loop does.

use std::path::{Path, PathBuf};
use std::time::Instant;

use ratatui::{Terminal, backend::TestBackend};
use yojaka_core::browse::{FileDetail, FileRow, FolderTree};
use yojaka_core::model::MediaKind;
use yojaka_core::stats::{
    CameraStat, FormatStat, KindStats, LastScan, LocationStats, Stats, VideoStats, YearStat,
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
fn later_screens_say_when_they_arrive() {
    let mut a = app_at(80, 24);
    a.screen = Screen::Duplicates;
    assert!(draw(&a, 80, 24).contains("Arrives in Iteration 4"));
    a.screen = Screen::Organize;
    assert!(draw(&a, 80, 24).contains("Arrives in Iteration 6"));
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
