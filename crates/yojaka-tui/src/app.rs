//! Elm-style core: `App` holds all state, `update(Msg)` changes it and returns `Cmd`s for the
//! runtime to execute, `view` (see `view.rs`) only reads it.

use std::path::PathBuf;
use std::time::Instant;

use yojaka_core::scan::{ScanStatus, ScanSummary};
use yojaka_core::stats::Stats;
use yojaka_core::{CoreEvent, TaskKind};

use crate::browser::{BrowserState, Focus};
use crate::fmt;
use crate::worker::{Bg, Request, Response};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Browser,
    Duplicates,
    Organize,
    History,
}

impl Screen {
    pub fn title(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Browser => "Browser",
            Screen::Duplicates => "Duplicates",
            Screen::Organize => "Organize",
            Screen::History => "History",
        }
    }
    /// Iteration in which the screen gets real content.
    pub fn arrives_in(self) -> u8 {
        match self {
            Screen::Dashboard | Screen::Browser => 3,
            Screen::Duplicates => 4,
            Screen::Organize => 6,
            Screen::History => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadOnly,
    Write,
}

#[derive(Debug)]
pub enum Msg {
    Quit,
    /// Ctrl-C: stops a running scan first, quits when none is running.
    Interrupt,
    ToggleHelp,
    /// Esc: closes help, clears the filter, then stops a running scan.
    Back,
    Goto(Screen),
    Resize(u16, u16),
    Core(CoreEvent),
    Bg(Bg),
    /// About four times a second.
    Tick,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Tab,
    Enter,
    CycleKind,
    CycleSort,
    ReverseSort,
    StartFilter,
    Char(char),
    Backspace,
    FilterAccept,
    FilterCancel,
    Rescan,
    CancelScan,
    ToggleDebug,
    /// Debug builds only (Ctrl-\): proves the terminal is restored after a panic.
    #[cfg(debug_assertions)]
    DebugPanic,
}

/// Work for the runtime. `update` never does I/O itself.
#[derive(Debug)]
pub enum Cmd {
    StartScan,
    CancelScan,
    Worker(Request),
}

/// What the scan gauge shows.
#[derive(Debug, Default)]
pub struct ScanView {
    pub running: bool,
    pub cancelling: bool,
    pub phase: Option<TaskKind>,
    pub label: String,
    pub done: u64,
    pub total: Option<u64>,
    pub bytes: u64,
    pub current: Option<PathBuf>,
    /// Items per second in the current phase.
    pub rate: Option<f64>,
    pub started: Option<Instant>,
    /// Per-file problems seen during this run (the full list is in the log).
    pub issues: u64,
    /// One line about the last finished run.
    pub outcome: Option<String>,
    /// The last run was stopped early; its results are kept but incomplete.
    pub partial: bool,
}

impl ScanView {
    fn begin_phase(&mut self, task: TaskKind, label: String) {
        self.phase = Some(task);
        self.label = label;
        self.done = 0;
        self.total = None;
        self.bytes = 0;
        self.current = None;
        self.rate = None;
        self.started = Some(Instant::now());
    }

    /// 0..=1 when the phase knows its size.
    pub fn ratio(&self) -> Option<f64> {
        let total = self.total?;
        (total > 0).then(|| (self.done as f64 / total as f64).clamp(0.0, 1.0))
    }
}

#[derive(Debug, Default)]
pub struct DashboardState {
    pub stats: Option<Box<Stats>>,
    stats_pending: bool,
    /// The folder has no index yet.
    pub no_index: bool,
    /// Fatal problem with the folder or index, shown instead of numbers.
    pub error: Option<String>,
    pub scan: ScanView,
}

#[derive(Debug, Default)]
pub struct DebugInfo {
    pub frame_ms: f64,
    pub frame_max_ms: f64,
    pub fetch_ms: f64,
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub show_help: bool,
    pub mode: Mode,
    pub root: PathBuf,
    pub should_quit: bool,
    pub size: (u16, u16),
    pub dashboard: DashboardState,
    pub browser: BrowserState,
    pub debug: bool,
    pub debug_info: DebugInfo,
    /// Set by `update` when the screen needs a redraw.
    pub dirty: bool,
    pub ticks: u64,
    auto_scan_tried: bool,
}

/// Number of ticks (¼ s) between statistics refreshes while a scan runs.
const LIVE_STATS_TICKS: u64 = 12;

impl App {
    pub fn new(root: PathBuf, mode: Mode) -> Self {
        Self {
            screen: Screen::Dashboard,
            show_help: false,
            mode,
            root,
            should_quit: false,
            size: (80, 24),
            dashboard: DashboardState::default(),
            browser: BrowserState::default(),
            debug: false,
            debug_info: DebugInfo::default(),
            dirty: true,
            ticks: 0,
            auto_scan_tried: false,
        }
    }

    /// Commands to run right after start-up.
    pub fn init(&mut self) -> Vec<Cmd> {
        self.layout();
        self.dashboard.stats_pending = true;
        let mut cmds = vec![Cmd::Worker(Request::Stats), Cmd::Worker(Request::Folders)];
        cmds.extend(self.browser.refresh());
        cmds
    }

    fn layout(&mut self) {
        let (tree_rows, table_rows) = crate::view::browser_rows(self.size.0, self.size.1);
        self.browser.tree_rows = tree_rows;
        self.browser.table_rows = table_rows;
    }

    /// One-line index/scan status for the header.
    pub fn status_line(&self) -> String {
        let s = &self.dashboard.scan;
        if s.running {
            let what = match s.phase {
                Some(TaskKind::Metadata) => "reading metadata",
                Some(TaskKind::Geocode) => "resolving places",
                _ if s.cancelling => "stopping",
                _ => "scanning",
            };
            return match s.ratio() {
                Some(r) => format!("{what} {:.0}%", r * 100.0),
                None => format!("{what} {} files", fmt::thousands(s.done)),
            };
        }
        if let Some(e) = &self.dashboard.error {
            return format!("error: {}", fmt::truncate(e, 40));
        }
        match &self.dashboard.stats {
            Some(st) => format!(
                "{} files indexed",
                fmt::thousands(st.photos.count + st.videos.count)
            ),
            None if self.dashboard.no_index => "not indexed yet".to_string(),
            None => "loading index…".to_string(),
        }
    }

    fn request_stats(&mut self) -> Vec<Cmd> {
        if self.dashboard.stats_pending {
            return vec![];
        }
        self.dashboard.stats_pending = true;
        vec![Cmd::Worker(Request::Stats)]
    }

    /// Everything that depends on the index, after a scan changed it.
    fn reload_index(&mut self) -> Vec<Cmd> {
        self.dashboard.no_index = false;
        self.dashboard.error = None;
        self.dashboard.stats_pending = true;
        let mut cmds = vec![
            Cmd::Worker(Request::Reload),
            Cmd::Worker(Request::Stats),
            Cmd::Worker(Request::Folders),
        ];
        cmds.extend(self.browser.refresh());
        cmds
    }

    fn start_scan(&mut self) -> Vec<Cmd> {
        if self.dashboard.scan.running {
            return vec![];
        }
        let s = &mut self.dashboard.scan;
        *s = ScanView {
            running: true,
            partial: s.partial,
            phase: Some(TaskKind::Scan),
            label: "Scanning".into(),
            started: Some(Instant::now()),
            ..Default::default()
        };
        self.dirty = true;
        vec![Cmd::StartScan]
    }

    fn cancel_scan(&mut self) -> Vec<Cmd> {
        let s = &mut self.dashboard.scan;
        if s.running && !s.cancelling {
            s.cancelling = true;
            self.dirty = true;
            return vec![Cmd::CancelScan];
        }
        vec![]
    }

    /// Applies `msg`. The caller must run the returned commands and redraw when `self.dirty`.
    pub fn update(&mut self, msg: Msg) -> Vec<Cmd> {
        // Everything except background chatter redraws; the few exceptions say so below.
        self.dirty = true;
        match msg {
            Msg::Quit => {
                self.should_quit = true;
                vec![]
            }
            Msg::Interrupt => {
                if self.dashboard.scan.running && !self.dashboard.scan.cancelling {
                    self.cancel_scan()
                } else {
                    self.should_quit = true;
                    vec![]
                }
            }
            Msg::ToggleHelp => {
                self.show_help = !self.show_help;
                vec![]
            }
            Msg::Back => {
                if self.show_help {
                    self.show_help = false;
                    vec![]
                } else if self.screen == Screen::Browser && !self.browser.query.text.is_empty() {
                    self.browser.set_text(String::new())
                } else if self.dashboard.scan.running {
                    self.cancel_scan()
                } else {
                    self.dirty = false;
                    vec![]
                }
            }
            Msg::Goto(s) => {
                self.dirty = self.screen != s || self.show_help;
                self.screen = s;
                self.show_help = false;
                self.browser.editing_filter = false;
                vec![]
            }
            Msg::Resize(w, h) => {
                self.size = (w, h);
                self.layout();
                // The visible range may have grown past the cached window.
                self.browser.move_cursor(0)
            }
            Msg::Core(ev) => self.on_core(ev),
            Msg::Bg(bg) => self.on_bg(bg),
            Msg::Tick => self.on_tick(),
            Msg::Rescan => self.start_scan(),
            Msg::CancelScan => self.cancel_scan(),
            Msg::ToggleDebug => {
                self.debug = !self.debug;
                vec![]
            }
            Msg::Up
            | Msg::Down
            | Msg::Left
            | Msg::Right
            | Msg::PageUp
            | Msg::PageDown
            | Msg::Top
            | Msg::Bottom
            | Msg::Tab
            | Msg::Enter
                if self.screen != Screen::Browser =>
            {
                self.dirty = false;
                vec![]
            }
            Msg::Up => self.move_selection(-1),
            Msg::Down => self.move_selection(1),
            Msg::PageUp => self.browser.page(false),
            Msg::PageDown => self.browser.page(true),
            Msg::Top => self.browser.jump(false),
            Msg::Bottom => self.browser.jump(true),
            Msg::Left => {
                if self.browser.focus == Focus::Tree {
                    self.browser.tree_collapse_or_parent()
                } else {
                    self.browser.focus = Focus::Tree;
                    vec![]
                }
            }
            Msg::Right => {
                if self.browser.focus == Focus::Tree {
                    self.browser.tree_expand_or_child()
                } else {
                    vec![]
                }
            }
            Msg::Tab => {
                self.browser.focus = match self.browser.focus {
                    Focus::Tree => Focus::Table,
                    Focus::Table => Focus::Tree,
                };
                vec![]
            }
            Msg::Enter => {
                self.browser.focus = Focus::Table;
                vec![]
            }
            Msg::CycleKind | Msg::CycleSort | Msg::ReverseSort | Msg::StartFilter
                if self.screen != Screen::Browser =>
            {
                self.dirty = false;
                vec![]
            }
            Msg::CycleKind => self.browser.cycle_kind(),
            Msg::CycleSort => self.browser.cycle_sort(),
            Msg::ReverseSort => self.browser.reverse_sort(),
            Msg::StartFilter => {
                self.browser.editing_filter = true;
                self.browser.focus = Focus::Table;
                vec![]
            }
            Msg::Char(c) => {
                if self.browser.editing_filter {
                    let mut t = self.browser.query.text.clone();
                    t.push(c);
                    self.browser.set_text(t)
                } else {
                    self.dirty = false;
                    vec![]
                }
            }
            Msg::Backspace => {
                let mut t = self.browser.query.text.clone();
                t.pop();
                self.browser.set_text(t)
            }
            Msg::FilterAccept => {
                self.browser.editing_filter = false;
                vec![]
            }
            Msg::FilterCancel => {
                self.browser.editing_filter = false;
                self.browser.set_text(String::new())
            }
            #[cfg(debug_assertions)]
            #[allow(clippy::panic)]
            Msg::DebugPanic => panic!("debug panic requested (Ctrl-\\)"),
        }
    }

    fn move_selection(&mut self, delta: i64) -> Vec<Cmd> {
        match self.browser.focus {
            Focus::Tree => self.browser.tree_move(delta),
            Focus::Table => self.browser.move_cursor(delta),
        }
    }

    fn on_tick(&mut self) -> Vec<Cmd> {
        self.ticks += 1;
        if !self.dashboard.scan.running {
            self.dirty = false;
            return vec![];
        }
        // The spinner and the elapsed rate move even when no event arrives.
        if self.ticks.is_multiple_of(LIVE_STATS_TICKS) {
            return self.request_stats();
        }
        vec![]
    }

    fn on_core(&mut self, ev: CoreEvent) -> Vec<Cmd> {
        let s = &mut self.dashboard.scan;
        match ev {
            CoreEvent::TaskStarted { task, label } => {
                s.begin_phase(task, label);
            }
            CoreEvent::Progress {
                task,
                done,
                total,
                bytes,
                current,
            } => {
                if s.phase != Some(task) {
                    s.begin_phase(task, String::new());
                }
                s.done = done;
                s.total = total;
                s.bytes = bytes;
                if current.is_some() {
                    s.current = current;
                }
                s.rate = s.started.and_then(|t| {
                    let secs = t.elapsed().as_secs_f64();
                    (secs > 0.2).then(|| done as f64 / secs)
                });
            }
            CoreEvent::Issue(i) => {
                s.issues += 1;
                tracing::debug!(?i, "scan issue");
            }
            CoreEvent::TaskFinished { .. } => self.dirty = false,
        }
        vec![]
    }

    fn on_bg(&mut self, bg: Bg) -> Vec<Cmd> {
        match bg {
            Bg::ScanFinished(result) => {
                let s = &mut self.dashboard.scan;
                s.running = false;
                s.cancelling = false;
                s.phase = None;
                match result {
                    Ok(sum) => {
                        s.partial = sum.status == ScanStatus::Partial;
                        s.outcome = Some(outcome_line(&sum));
                        self.reload_index()
                    }
                    Err(e) => {
                        self.dashboard.error = Some(e);
                        vec![]
                    }
                }
            }
            Bg::Worker(r) => self.on_response(r),
        }
    }

    fn on_response(&mut self, r: Response) -> Vec<Cmd> {
        match r {
            Response::Stats(st) => {
                self.dashboard.stats_pending = false;
                self.dashboard.no_index = false;
                self.dashboard.error = None;
                self.dashboard.stats = Some(st);
                vec![]
            }
            Response::NoIndex => {
                self.dashboard.stats_pending = false;
                self.dashboard.no_index = true;
                self.browser.total = Some(0);
                self.browser.window = None;
                // First visit to a folder: build the index without asking.
                if !self.auto_scan_tried && !self.dashboard.scan.running {
                    self.auto_scan_tried = true;
                    return self.start_scan();
                }
                vec![]
            }
            Response::Error(e) => {
                self.dashboard.stats_pending = false;
                self.auto_scan_tried = true;
                self.dashboard.error = Some(e);
                vec![]
            }
            Response::Folders(t) => {
                self.browser.on_folders(t);
                vec![]
            }
            Response::Count { generation, total } => {
                self.dirty = generation == self.browser.generation;
                self.browser.on_count(generation, total)
            }
            Response::Page {
                generation,
                offset,
                limit,
                rows,
                ms,
            } => {
                self.debug_info.fetch_ms = ms;
                self.dirty = generation == self.browser.generation;
                self.browser.on_page(generation, offset, limit, rows)
            }
            Response::Detail { id, detail } => {
                self.browser.on_detail(id, detail.map(|d| *d));
                vec![]
            }
        }
    }
}

#[cfg(test)]
impl App {
    pub(crate) fn dashboard_stats(&mut self, st: Stats) {
        self.dashboard.stats = Some(Box::new(st));
    }
    pub(crate) fn dashboard_scan_rate(&mut self, rate: f64) {
        self.dashboard.scan.rate = Some(rate);
    }
    pub(crate) fn dashboard_stop_scan(&mut self) {
        self.dashboard.scan = ScanView::default();
    }
}

fn outcome_line(s: &ScanSummary) -> String {
    let how = match s.status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "stopped early, partial results kept",
    };
    format!(
        "Last scan {how}: {} files indexed, {} new or changed, {:.1}s",
        fmt::thousands(s.files_indexed),
        fmt::thousands(s.new_or_changed),
        s.elapsed_ms as f64 / 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new("/tmp".into(), Mode::ReadOnly)
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        a.update(Msg::Quit);
        assert!(a.should_quit);
    }

    #[test]
    fn question_mark_toggles_help() {
        let mut a = app();
        a.update(Msg::ToggleHelp);
        assert!(a.show_help);
        a.update(Msg::ToggleHelp);
        assert!(!a.show_help);
    }

    #[test]
    fn three_switches_to_duplicates() {
        let mut a = app();
        a.update(Msg::Goto(Screen::Duplicates));
        assert_eq!(a.screen, Screen::Duplicates);
    }

    #[test]
    fn esc_closes_help_only_when_open() {
        let mut a = app();
        a.update(Msg::Back);
        assert!(!a.dirty);
        a.update(Msg::ToggleHelp);
        a.update(Msg::Back);
        assert!(a.dirty);
        assert!(!a.show_help);
    }

    #[test]
    fn startup_asks_for_stats_folders_and_the_first_page() {
        let mut a = app();
        let cmds = a.init();
        assert_eq!(cmds.len(), 4);
        assert!(matches!(cmds[0], Cmd::Worker(Request::Stats)));
    }

    #[test]
    fn a_missing_index_starts_one_scan_automatically() {
        let mut a = app();
        let cmds = a.update(Msg::Bg(Bg::Worker(Response::NoIndex)));
        assert!(matches!(cmds.as_slice(), [Cmd::StartScan]));
        assert!(a.dashboard.scan.running);
        // a second "no index" answer (the browser asks too) must not start another
        assert!(a.update(Msg::Bg(Bg::Worker(Response::NoIndex))).is_empty());
    }

    #[test]
    fn a_protected_folder_does_not_trigger_a_scan() {
        let mut a = app();
        let cmds = a.update(Msg::Bg(Bg::Worker(Response::Error("protected".into()))));
        assert!(cmds.is_empty());
        assert!(a.update(Msg::Bg(Bg::Worker(Response::NoIndex))).is_empty());
        assert!(a.dashboard.error.is_some());
    }

    #[test]
    fn rescan_cancel_and_finish() {
        let mut a = app();
        assert!(matches!(a.update(Msg::Rescan).as_slice(), [Cmd::StartScan]));
        // already running: ignored
        assert!(a.update(Msg::Rescan).is_empty());
        assert!(matches!(
            a.update(Msg::CancelScan).as_slice(),
            [Cmd::CancelScan]
        ));
        assert!(a.dashboard.scan.cancelling);
        // asking twice does not cancel twice
        assert!(a.update(Msg::CancelScan).is_empty());
        let sum = ScanSummary {
            scan_id: 1,
            status: ScanStatus::Partial,
            files_seen: 5,
            files_indexed: 5,
            new_or_changed: 5,
            unchanged: 0,
            pruned: 0,
            cloud_only: 0,
            bytes: 0,
            issues: 0,
            permission_denied: 0,
            elapsed_ms: 1500,
            metadata: None,
        };
        let cmds = a.update(Msg::Bg(Bg::ScanFinished(Ok(sum))));
        assert!(!a.dashboard.scan.running);
        assert!(
            a.dashboard
                .scan
                .outcome
                .as_deref()
                .is_some_and(|o| o.contains("partial"))
        );
        // the index changed: statistics, folders and the table are all reloaded
        assert!(matches!(cmds[0], Cmd::Worker(Request::Reload)));
        assert!(cmds.len() >= 5);
    }

    #[test]
    fn ctrl_c_stops_the_scan_first_then_quits() {
        let mut a = app();
        a.update(Msg::Rescan);
        a.update(Msg::Interrupt);
        assert!(!a.should_quit);
        assert!(a.dashboard.scan.cancelling);
        a.update(Msg::Interrupt);
        assert!(a.should_quit);
    }

    #[test]
    fn esc_clears_the_filter_before_it_cancels_a_scan() {
        let mut a = app();
        a.update(Msg::Goto(Screen::Browser));
        a.update(Msg::Rescan);
        a.browser.query.text = "abc".into();
        a.update(Msg::Back);
        assert!(a.browser.query.text.is_empty());
        assert!(!a.dashboard.scan.cancelling);
        a.update(Msg::Back);
        assert!(a.dashboard.scan.cancelling);
    }

    #[test]
    fn progress_events_drive_the_gauge() {
        let mut a = app();
        a.update(Msg::Rescan);
        a.update(Msg::Core(CoreEvent::Progress {
            task: TaskKind::Metadata,
            done: 50,
            total: Some(200),
            bytes: 0,
            current: Some("/x".into()),
        }));
        let s = &a.dashboard.scan;
        assert_eq!(s.phase, Some(TaskKind::Metadata));
        assert_eq!(s.ratio(), Some(0.25));
        assert!(a.status_line().contains("25%"), "{}", a.status_line());
    }

    #[test]
    fn live_statistics_refresh_while_scanning_but_never_pile_up() {
        let mut a = app();
        a.update(Msg::Rescan);
        let mut asked = 0;
        for _ in 0..(LIVE_STATS_TICKS * 3) {
            asked += a
                .update(Msg::Tick)
                .iter()
                .filter(|c| matches!(c, Cmd::Worker(Request::Stats)))
                .count();
        }
        // answered never, so only the first request goes out
        assert_eq!(asked, 1);
    }

    #[test]
    fn idle_ticks_do_not_redraw() {
        let mut a = app();
        a.update(Msg::Tick);
        assert!(!a.dirty);
    }

    #[test]
    fn browser_keys_do_nothing_on_other_screens() {
        let mut a = app();
        a.update(Msg::Down);
        assert!(!a.dirty);
        a.update(Msg::CycleKind);
        assert!(!a.dirty);
    }

    #[test]
    fn filter_typing_edits_the_query() {
        let mut a = app();
        a.update(Msg::Goto(Screen::Browser));
        a.update(Msg::StartFilter);
        a.update(Msg::Char('a'));
        a.update(Msg::Char('b'));
        a.update(Msg::Backspace);
        assert_eq!(a.browser.query.text, "a");
        a.update(Msg::FilterAccept);
        assert!(!a.browser.editing_filter);
        assert_eq!(a.browser.query.text, "a");
        a.update(Msg::StartFilter);
        a.update(Msg::FilterCancel);
        assert!(a.browser.query.text.is_empty());
    }
}
