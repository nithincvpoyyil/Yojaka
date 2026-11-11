//! Elm-style core: `App` holds all state, `update(Msg)` changes it and returns `Cmd`s for the
//! runtime to execute, `view` (see `view.rs`) only reads it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use yojaka_core::scan::{ScanStatus, ScanSummary};
use yojaka_core::stats::Stats;
use yojaka_core::{CoreEvent, TaskKind};

use crate::browser::{BrowserState, Focus};
use crate::dupes::{DupesState, Pane, Toggled};
use crate::fmt;
use crate::history::{self, HistoryState};
use crate::modal::{self, Modal, Review, RunView};
use crate::organize::{Editing, OrganizeState, Phase as OrgPhase, PlanRequest};
use crate::worker::{Bg, DupOutcome, ExecOutcome, Request, Response};

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
            Screen::Browser => "File browser",
            Screen::Duplicates => "Duplicates",
            Screen::Organize => "Organize",
            Screen::History => "History",
        }
    }
    pub const ALL: [Screen; 5] = [
        Screen::Dashboard,
        Screen::Browser,
        Screen::Duplicates,
        Screen::Organize,
        Screen::History,
    ];

    /// The neighbouring tab; wraps around at the ends.
    pub fn step(self, forward: bool) -> Screen {
        let n = Self::ALL.len();
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
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
    /// `<` / `>` (and ←/→ where they have no other job): the tab on the left / right.
    PrevScreen,
    NextScreen,
    /// `T`, or Esc when there is nothing else to close: ←/→ then pick a tab.
    TabMode,
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
    /// Space: mark / unmark the selected copy (Duplicates).
    Toggle,
    /// `f`: look for duplicates (identical, likely and same-name copies).
    FindDupes,
    /// `F`: the same, and also compare how pictures look (reads every picture once).
    FindSimilar,
    /// `v`: show all tiers / one tier at a time.
    CycleTier,
    /// `p`: next keep rule, applied to every group.
    NextRule,
    /// `x`: keep every copy in the selected group.
    KeepAllInGroup,
    /// `W`: write mode on (asks first) / off.
    ToggleWrite,
    /// `a` (Duplicates): review the marked copies before moving them to the Trash.
    ReviewPlan,
    /// `P` (Duplicates, Organize): save the current plan as a file in Yojaka's data folder.
    SavePlan,
    /// `E` (File browser): export the files shown as a CSV file in Yojaka's data folder.
    ExportView,
    /// `M` (Duplicates, Organize): remember the current choices in the config file.
    Remember,
    /// `u` (History): put back the files of the selected run.
    UndoRun,
    /// `e` (Organize): edit the folder pattern.
    EditTemplate,
    /// `d` (Organize): edit the destination folder.
    EditDest,
    /// `m` (Organize): copy / move.
    ToggleTransfer,
    /// `i` (Organize): infer places for files without GPS.
    ToggleInfer,
    CycleKind,
    CycleSort,
    ReverseSort,
    StartFilter,
    Char(char),
    Backspace,
    FilterAccept,
    FilterCancel,
    Rescan,
    /// `R`: asks before flushing the index and scanning the folder from scratch.
    FullRescan,
    ConfirmYes,
    ConfirmNo,
    CancelScan,
    ToggleDebug,
    /// Debug builds only (Ctrl-\): proves the terminal is restored after a panic.
    #[cfg(debug_assertions)]
    DebugPanic,
}

/// Work for the runtime. `update` never does I/O itself.
#[derive(Debug)]
pub enum Cmd {
    /// `force`: delete the index first, then scan from scratch.
    StartScan {
        force: bool,
    },
    CancelScan,
    /// Look for duplicates (reads files, changes none). `similar` also decodes pictures.
    FindDupes {
        similar: bool,
    },
    CancelDupes,
    /// Hand the file to the OS to open with its default app. Never modifies it.
    OpenFile(PathBuf),
    Worker(Request),
    /// Run a reviewed plan (moves files to the Trash; journaled).
    /// Work out an organize plan (reads the index and the destination's names, changes nothing).
    PlanOrganize(Box<PlanRequest>),
    CancelPlan,
    /// Save choices as the defaults in `config.toml`.
    Remember(Remember),
    /// Write a plan to a new file (never overwrites; changes no photos).
    SavePlan(Box<yojaka_core::plan::Plan>),
    /// Write the files the browser shows to a new CSV file.
    ExportView(Box<yojaka_core::browse::BrowseQuery>),
    ApplyPlan(Box<yojaka_core::plan::Plan>),
    /// Put back everything a journaled run moved.
    UndoRun(uuid::Uuid),
    CancelExec,
    LoadHistory,
}

/// Choices `M` writes to the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remember {
    /// A `--keep` chain, e.g. `oldest-modified,shortest-path`.
    KeepRules(String),
    /// Folder pattern and `copy` / `move`.
    Organize { template: String, mode: String },
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
    /// When the whole run began (all phases).
    pub run_started: Option<Instant>,
    /// When the last progress event arrived; used to tell the user when nothing is moving.
    pub last_event: Option<Instant>,
    /// Last `(time, done)` used for the smoothed rate.
    sample: Option<(Instant, u64)>,
    /// Per-file problems seen during this run (the full list is in the log).
    pub issues: u64,
    /// One line about the last finished run.
    pub outcome: Option<String>,
    /// The last run was stopped early; its results are kept but incomplete.
    pub partial: bool,
}

/// Nothing heard from the scan for this long: tell the user instead of showing a frozen number.
pub const STALL_AFTER: Duration = Duration::from_secs(5);

impl ScanView {
    fn begin_phase(&mut self, task: TaskKind, label: String) {
        let now = Instant::now();
        if self.phase.is_none() {
            self.run_started = Some(now);
        }
        self.phase = Some(task);
        self.label = label;
        self.done = 0;
        self.total = None;
        self.bytes = 0;
        self.current = None;
        self.rate = None;
        self.started = Some(now);
        self.last_event = Some(now);
        self.sample = None;
    }

    /// Smoothed items per second: recent speed, so a slow stretch shows up and a fast one too.
    fn update_rate(&mut self, now: Instant, done: u64) {
        match self.sample {
            None => self.sample = Some((now, done)),
            Some((t, d)) => {
                let dt = now.duration_since(t).as_secs_f64();
                if dt >= 0.5 {
                    let inst = done.saturating_sub(d) as f64 / dt;
                    self.rate = Some(self.rate.map_or(inst, |r| r * 0.7 + inst * 0.3));
                    self.sample = Some((now, done));
                }
            }
        }
        if self.rate.is_none() {
            self.rate = self.started.and_then(|t| {
                let secs = t.elapsed().as_secs_f64();
                (secs > 0.5).then(|| done as f64 / secs)
            });
        }
    }

    /// Which of the two steps is running: 1 = finding files, 2 = reading their details.
    pub fn step(&self) -> u8 {
        match self.phase {
            Some(TaskKind::Scan) | None => 1,
            _ => 2,
        }
    }

    /// 0..=1 when the phase knows its size. While finding files the size is only an estimate
    /// (the last scan), so the bar stops at 99% until the step really ends.
    pub fn ratio(&self) -> Option<f64> {
        let total = self.total?;
        if total == 0 {
            return None;
        }
        if self.finishing() {
            return Some(1.0);
        }
        let r = (self.done as f64 / total as f64).clamp(0.0, 1.0);
        Some(if self.phase == Some(TaskKind::Scan) {
            r.min(0.99)
        } else {
            r
        })
    }

    /// The size of the current phase is a guess (shown with a `~`).
    pub fn total_is_estimate(&self) -> bool {
        self.phase == Some(TaskKind::Scan)
    }

    /// Estimated time left in this phase. Needs a known size and a couple of seconds of data.
    pub fn eta(&self) -> Option<Duration> {
        if self.finishing() || self.stalled_for().is_some() {
            return None;
        }
        let (total, rate) = (self.total?, self.rate?);
        if rate < 0.5 || self.done == 0 || self.started?.elapsed() < Duration::from_secs(2) {
            return None;
        }
        let left = total.saturating_sub(self.done) as f64 / rate;
        Some(Duration::from_secs_f64(left.min(99.0 * 3600.0)))
    }

    pub fn elapsed(&self) -> Option<Duration> {
        self.run_started.map(|t| t.elapsed())
    }

    /// The countable work of this step is done. What follows (saving, pairing Live Photos,
    /// sharing places) reports no numbers, so silence is expected.
    pub fn finishing(&self) -> bool {
        self.total.is_some_and(|t| t > 0 && self.done >= t)
    }

    /// How long nothing has arrived, once that is noticeable.
    pub fn stalled_for(&self) -> Option<Duration> {
        if self.finishing() {
            return None;
        }
        self.last_event
            .map(|t| t.elapsed())
            .filter(|d| *d >= STALL_AFTER)
    }
}

#[derive(Debug, Default)]
pub struct DashboardState {
    pub stats: Option<Box<Stats>>,
    stats_pending: bool,
    /// When the running statistics request was sent.
    stats_asked: Option<Instant>,
    /// How long the last statistics request took to answer.
    stats_cost: Duration,
    /// The tick at which the last statistics answer arrived.
    stats_answered_tick: Option<u64>,
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
    /// The "flush index and rescan?" prompt is open.
    pub confirm_flush: bool,
    /// The tab strip has the keyboard: ←/→ switch screens, Enter / Esc / ↓ go back in.
    pub tab_mode: bool,
    /// A short message in the footer, and the tick at which it goes away.
    pub notice: Option<(String, u64)>,
    /// `--refresh`: scan on open when the last scan is more than an hour old.
    pub refresh_when_stale: bool,
    auto_scan_tried: bool,
    pub mode: Mode,
    pub root: PathBuf,
    pub should_quit: bool,
    pub size: (u16, u16),
    pub dashboard: DashboardState,
    pub browser: BrowserState,
    pub dupes: DupesState,
    pub history: HistoryState,
    pub organize: OrganizeState,
    /// The question, plan review or progress that has the keyboard.
    pub modal: Option<Modal>,
    pub debug: bool,
    pub debug_info: DebugInfo,
    /// Set by `update` when the screen needs a redraw.
    pub dirty: bool,
    pub ticks: u64,
}

/// Number of ticks (¼ s) between statistics refreshes while a scan runs.
const LIVE_STATS_TICKS: u64 = 12;

/// How long a footer message stays (¼ s ticks).
const NOTICE_TICKS: u64 = 12;

impl App {
    pub fn new(root: PathBuf, mode: Mode) -> Self {
        Self {
            screen: Screen::Dashboard,
            show_help: false,
            confirm_flush: false,
            tab_mode: false,
            notice: None,
            refresh_when_stale: false,
            auto_scan_tried: false,
            organize: OrganizeState::new(&root),
            mode,
            root,
            should_quit: false,
            size: (80, 24),
            dashboard: DashboardState::default(),
            browser: BrowserState::default(),
            dupes: DupesState::default(),
            history: HistoryState::default(),
            modal: None,
            debug: false,
            debug_info: DebugInfo::default(),
            dirty: true,
            ticks: 0,
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
        self.dupes.page_rows = crate::view::dupes_rows(self.size.0, self.size.1);
        self.history.page_rows = crate::view::history_rows(self.size.0, self.size.1);
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
            if let Some(d) = s.stalled_for() {
                return format!("{what} · no progress for {}s", d.as_secs());
            }
            if s.finishing() {
                return format!("{what} · finishing up");
            }
            return match (s.ratio(), s.eta()) {
                (Some(r), Some(eta)) => format!(
                    "{what} {:.0}% · ~{} left",
                    r * 100.0,
                    fmt::span(eta.as_millis() as u64)
                ),
                (Some(r), None) => format!("{what} {:.0}%", r * 100.0),
                (None, _) => format!("{what} {} files", fmt::thousands(s.done)),
            };
        }
        if self.dupes.searching() {
            return match self.dupes.ratio() {
                Some(r) => format!("comparing files {:.0}%", r * 100.0),
                None => "looking for duplicates".to_string(),
            };
        }
        if let Some(e) = &self.dashboard.error {
            return format!("error: {}", fmt::truncate(e, 40));
        }
        match &self.dashboard.stats {
            Some(st) => format!(
                "{} files",
                fmt::thousands(st.photos.count + st.videos.count)
            ),
            None if self.dashboard.no_index => "not scanned yet".to_string(),
            None => "loading…".to_string(),
        }
    }

    fn request_stats(&mut self) -> Vec<Cmd> {
        if self.dashboard.stats_pending {
            return vec![];
        }
        self.dashboard.stats_pending = true;
        self.dashboard.stats_asked = Some(Instant::now());
        vec![Cmd::Worker(Request::Stats)]
    }

    /// Live refreshes wait at least 4× the last query time (and never less than LIVE_STATS_TICKS).
    fn live_stats_due(&self) -> bool {
        let gap = ((self.dashboard.stats_cost.as_millis() as u64 * 4) / 250).max(LIVE_STATS_TICKS);
        self.dashboard
            .stats_answered_tick
            .is_none_or(|t| self.ticks.saturating_sub(t) >= gap)
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

    /// Once, when the first answer about the folder arrives: a never-scanned folder is scanned
    /// without asking; with `--refresh` a scan older than an hour is updated (nothing is
    /// cleared). Waits while the `--force` question is open.
    fn open_decision(&mut self) -> Vec<Cmd> {
        if self.auto_scan_tried || self.confirm_flush {
            return vec![];
        }
        let known = self.dashboard.no_index || self.dashboard.stats.is_some();
        if !known {
            return vec![];
        }
        self.auto_scan_tried = true;
        if self.dashboard.scan.running {
            return vec![];
        }
        let stale = self.refresh_when_stale
            && self
                .dashboard
                .stats
                .as_ref()
                .and_then(|st| st.last_scan.as_ref())
                .is_some_and(|l| is_stale(l.finished_at.unwrap_or(l.started_at), unix_now()));
        if self.dashboard.no_index || stale {
            self.start_scan()
        } else {
            vec![]
        }
    }

    /// Enter on a file: the OS opens it with whatever app it prefers.
    fn open_selected(&mut self) -> Vec<Cmd> {
        let Some(row) = self.browser.current_row() else {
            self.dirty = false;
            return vec![];
        };
        if row.cloud_only {
            // Opening would make macOS download it; Yojaka never touches iCloud-only files.
            let msg = format!("{} is only in iCloud, not opened", row.name);
            self.say(msg);
            return vec![];
        }
        let path = row.path.clone();
        self.say(format!("Opening {}", row.name));
        vec![Cmd::OpenFile(path)]
    }

    fn say(&mut self, msg: String) {
        self.notice = Some((msg, self.ticks + NOTICE_TICKS));
    }

    fn arrows_move_tree(&self) -> bool {
        self.screen == Screen::Browser && self.browser.focus == Focus::Tree
    }

    fn start_scan(&mut self) -> Vec<Cmd> {
        self.begin_scan(false)
    }

    fn begin_scan(&mut self, force: bool) -> Vec<Cmd> {
        if self.dashboard.scan.running {
            return vec![];
        }
        if self.dupes.searching() {
            self.say("Wait for the duplicate search, or press c to stop it".into());
            return vec![];
        }
        if self.organize.phase == OrgPhase::Planning {
            self.say("Wait for the plan to finish, or press Esc to stop it".into());
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
        let mut cmds = vec![];
        if force {
            // Nothing from the old index may stay on screen.
            self.dashboard.stats = None;
            self.dashboard.stats_pending = false;
            self.dashboard.no_index = true;
            self.dashboard.error = None;
            self.dashboard.scan.partial = false;
            self.browser.reset();
            self.dupes.reset();
        }
        cmds.push(Cmd::StartScan { force });
        cmds
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

    fn find_dupes(&mut self, similar: bool) -> Vec<Cmd> {
        if self.dupes.searching() {
            self.dirty = false;
            return vec![];
        }
        if self.dashboard.scan.running {
            self.say("Wait for the scan to finish first".into());
            return vec![];
        }
        if self.dashboard.no_index || self.dashboard.stats.is_none() {
            self.say("Scan this folder first".into());
            return vec![];
        }
        self.dupes.begin();
        vec![Cmd::FindDupes { similar }]
    }

    fn cancel_dupes(&mut self) -> Vec<Cmd> {
        if self.dupes.searching() {
            return vec![Cmd::CancelDupes];
        }
        self.dirty = false;
        vec![]
    }

    /// Keys on the Duplicates screen that move or mark.
    fn dupes_key(&mut self, msg: Msg) -> Vec<Cmd> {
        match msg {
            Msg::Up => self.dupes.move_by(-1),
            Msg::Down => self.dupes.move_by(1),
            Msg::PageUp => self.dupes.page(false),
            Msg::PageDown => self.dupes.page(true),
            Msg::Top => self.dupes.jump(false),
            Msg::Bottom => self.dupes.jump(true),
            Msg::Tab => self.dupes.switch_pane(),
            Msg::Enter => match self.dupes.pane {
                Pane::Groups => self.dupes.pane = Pane::Copies,
                Pane::Copies => self.dupes.keep_only(),
            },
            Msg::Toggle => {
                if self.dupes.toggle() == Toggled::LastCopy {
                    self.say("One copy of each group always stays".into());
                }
            }
            Msg::KeepAllInGroup => self.dupes.keep_all(),
            Msg::NextRule => {
                self.dupes.next_preset();
                let msg = format!(
                    "Keep rule for identical copies: {}",
                    self.dupes.preset_label()
                );
                self.say(msg);
            }
            Msg::CycleTier => {
                self.dupes.cycle_filter();
                let msg = format!("Showing: {}", self.dupes.filter_label());
                self.say(msg);
            }
            _ => self.dirty = false,
        }
        vec![]
    }

    fn toggle_write(&mut self) -> Vec<Cmd> {
        if self.is_busy_writing() {
            self.dirty = false;
            return vec![];
        }
        match self.mode {
            Mode::ReadOnly => self.modal = Some(Modal::EnableWrite),
            Mode::Write => {
                self.mode = Mode::ReadOnly;
                self.say("Write mode is off".into());
            }
        }
        vec![]
    }

    fn is_busy_writing(&self) -> bool {
        matches!(self.modal, Some(Modal::Running(_)))
    }

    /// `a` on the Duplicates screen: build the plan from the marks and show it for review.
    fn review_plan(&mut self) -> Vec<Cmd> {
        if self.mode != Mode::Write {
            self.say("Write mode is off. Press W to turn it on".into());
            return vec![];
        }
        if self.dashboard.scan.running || self.dupes.searching() {
            self.say("Wait for the scan or search to finish first".into());
            return vec![];
        }
        if self.dupes.marked_files() == 0 {
            self.say("Nothing is marked for removal".into());
            return vec![];
        }
        let plan = yojaka_core::plan::Plan::from_dupes(&self.root, &self.dupes.groups);
        for w in &plan.warnings {
            tracing::warn!("plan: {}", w.message);
        }
        if plan.ops.is_empty() {
            self.say("Nothing is marked for removal".into());
            return vec![];
        }
        self.modal = Some(Modal::Review(Box::new(Review::new(plan))));
        vec![]
    }

    /// Enter on the Organize screen: work out the plan for the current settings.
    fn plan_organize(&mut self) -> Vec<Cmd> {
        if self.organize.phase == OrgPhase::Planning {
            self.dirty = false;
            return vec![];
        }
        if self.dashboard.scan.running || self.dupes.searching() {
            self.say("Wait for the scan or search to finish first".into());
            return vec![];
        }
        if self.dashboard.no_index || self.dashboard.stats.is_none() {
            self.say("Scan this folder first".into());
            return vec![];
        }
        match self.organize.request() {
            Ok(req) => {
                self.organize.problem = None;
                self.organize.plan = None;
                self.organize.phase = OrgPhase::Planning;
                vec![Cmd::PlanOrganize(Box::new(req))]
            }
            Err(e) => {
                self.organize.problem = Some(e);
                vec![]
            }
        }
    }

    /// `a` on the Organize screen: review the plan before anything is done.
    fn review_organize(&mut self) -> Vec<Cmd> {
        if self.mode != Mode::Write {
            self.say("Write mode is off. Press W to turn it on".into());
            return vec![];
        }
        let Some(p) = self
            .organize
            .plan
            .as_ref()
            .filter(|_| self.organize.phase == OrgPhase::Ready)
        else {
            self.say("Press Enter first to plan".into());
            return vec![];
        };
        if p.plan.file_ops() == 0 {
            self.say("Nothing to do: every file is already in place".into());
            return vec![];
        }
        self.modal = Some(Modal::Review(Box::new(Review::new(p.plan.clone()))));
        vec![]
    }

    /// Starts from the choices saved in `config.toml`. A saved value that cannot be used is
    /// ignored and the footer says so.
    pub fn use_config(&mut self, config: &yojaka_core::Config) {
        let (_, keep_warning) = yojaka_core::dupes::configured_rules(&config.dupes.keep_rules);
        if keep_warning.is_none() {
            self.dupes.use_saved_chain(&config.dupes.keep_rules);
        }
        let organize_warning = self.organize.use_config(config);
        if let Some(w) = keep_warning.or(organize_warning) {
            tracing::warn!("{w}");
            self.say(w);
        }
    }

    /// `P` on the Duplicates screen: save the plan for the marked copies. Works in read-only mode.
    fn save_dupes_plan(&mut self) -> Vec<Cmd> {
        if self.dashboard.scan.running || self.dupes.searching() {
            self.say("Wait for the scan or search to finish first".into());
            return vec![];
        }
        let plan = yojaka_core::plan::Plan::from_dupes(&self.root, &self.dupes.groups);
        if plan.ops.is_empty() {
            self.say("Nothing is marked for removal".into());
            return vec![];
        }
        vec![Cmd::SavePlan(Box::new(plan))]
    }

    /// `P` on the Organize screen: save the plan that Enter made. Works in read-only mode.
    fn save_organize_plan(&mut self) -> Vec<Cmd> {
        match self
            .organize
            .plan
            .as_ref()
            .filter(|_| self.organize.phase == OrgPhase::Ready)
        {
            Some(p) if p.plan.file_ops() > 0 => vec![Cmd::SavePlan(Box::new(p.plan.clone()))],
            Some(_) => {
                self.say("Nothing to do: every file is already in place".into());
                vec![]
            }
            None => {
                self.say("Press Enter first to plan".into());
                vec![]
            }
        }
    }

    /// `u` on the History screen.
    fn ask_undo(&mut self) -> Vec<Cmd> {
        if self.mode != Mode::Write {
            self.say("Write mode is off. Press W to turn it on".into());
            return vec![];
        }
        if self.dashboard.scan.running {
            self.say("Wait for the scan to finish first".into());
            return vec![];
        }
        match self.history.current() {
            Some(run) if history::can_undo(run) => {
                self.modal = Some(Modal::Undo(Box::new(run.clone())));
            }
            Some(_) => self.say("Nothing left to put back for that run".into()),
            None => self.dirty = false,
        }
        vec![]
    }

    /// Keys while a question, review or progress box is open.
    fn modal_key(&mut self, msg: Msg) -> Vec<Cmd> {
        let Some(modal) = self.modal.take() else {
            return vec![];
        };
        let yes = matches!(msg, Msg::Char('y' | 'Y'));
        let no = matches!(msg, Msg::Back | Msg::Interrupt | Msg::Char('n' | 'N'));
        match modal {
            Modal::Running(mut r) => {
                if matches!(msg, Msg::Interrupt) && !r.cancelling {
                    r.cancelling = true;
                    self.modal = Some(Modal::Running(r));
                    return vec![Cmd::CancelExec];
                }
                self.modal = Some(Modal::Running(r));
                self.dirty = false;
                vec![]
            }
            Modal::Report(_) => vec![],
            Modal::EnableWrite => {
                if yes || matches!(msg, Msg::Enter) {
                    self.mode = Mode::Write;
                    self.say("Write mode is on. Nothing changes until you review a plan".into());
                } else if !no {
                    self.modal = Some(Modal::EnableWrite);
                    self.dirty = false;
                }
                vec![]
            }
            Modal::Undo(run) => {
                if yes || matches!(msg, Msg::Enter) {
                    self.modal = Some(Modal::Running(RunView {
                        label: "Putting files back",
                        done: 0,
                        total: Some(run.done as u64),
                        cancelling: false,
                    }));
                    vec![Cmd::UndoRun(run.plan_id)]
                } else {
                    if !no {
                        self.modal = Some(Modal::Undo(run));
                        self.dirty = false;
                    }
                    vec![]
                }
            }
            Modal::Review(mut rv) => {
                let typing = rv.needs_typing();
                match msg {
                    Msg::Char(c) if typing => rv.typed.push(c),
                    Msg::Backspace => {
                        rv.typed.pop();
                    }
                    Msg::Enter if rv.ready() => return self.start_apply(*rv),
                    Msg::Char('y' | 'Y') if !typing => return self.start_apply(*rv),
                    Msg::Back | Msg::Interrupt => return vec![],
                    Msg::Char('n' | 'N') if !typing => return vec![],
                    _ => self.dirty = false,
                }
                self.modal = Some(Modal::Review(rv));
                vec![]
            }
        }
    }

    fn start_apply(&mut self, rv: Review) -> Vec<Cmd> {
        if self.mode != Mode::Write {
            self.say("Write mode is off".into());
            return vec![];
        }
        let label = match rv.plan.mode {
            yojaka_core::plan::OpMode::Copy => "Copying files",
            yojaka_core::plan::OpMode::Move => "Moving files",
            _ => "Moving files to the Trash",
        };
        self.modal = Some(Modal::Running(RunView {
            label,
            done: 0,
            total: Some(rv.plan.ops.len() as u64),
            cancelling: false,
        }));
        vec![Cmd::ApplyPlan(Box::new(rv.plan))]
    }

    /// Applies `msg`. The caller must run the returned commands and redraw when `self.dirty`.
    pub fn update(&mut self, msg: Msg) -> Vec<Cmd> {
        // Everything except background chatter redraws; the few exceptions say so below.
        self.dirty = true;
        if self.modal.is_some()
            && matches!(
                msg,
                Msg::Back | Msg::Enter | Msg::Char(_) | Msg::Backspace | Msg::Interrupt
            )
        {
            return self.modal_key(msg);
        }
        if self.screen == Screen::Organize && self.organize.editing.is_some() {
            match msg {
                Msg::Char(c) => self.organize.type_char(c),
                Msg::Backspace => self.organize.backspace(),
                Msg::FilterAccept => self.organize.accept_edit(),
                Msg::FilterCancel => self.organize.cancel_edit(),
                _ => self.dirty = false,
            }
            return vec![];
        }
        if self.tab_mode {
            match msg {
                Msg::Left | Msg::PrevScreen => {
                    return self.update(Msg::Goto(self.screen.step(false)));
                }
                Msg::Right | Msg::NextScreen => {
                    return self.update(Msg::Goto(self.screen.step(true)));
                }
                Msg::Enter | Msg::Back | Msg::Down | Msg::Up | Msg::Tab | Msg::TabMode => {
                    self.tab_mode = false;
                    return vec![];
                }
                // keys that move inside a screen do nothing while the tabs are selected
                Msg::PageUp
                | Msg::PageDown
                | Msg::Top
                | Msg::Bottom
                | Msg::CycleKind
                | Msg::CycleSort
                | Msg::ReverseSort
                | Msg::StartFilter
                | Msg::Toggle
                | Msg::FindDupes
                | Msg::FindSimilar
                | Msg::CycleTier
                | Msg::NextRule
                | Msg::ReviewPlan
                | Msg::SavePlan
                | Msg::Remember
                | Msg::ExportView
                | Msg::UndoRun
                | Msg::EditTemplate
                | Msg::EditDest
                | Msg::ToggleTransfer
                | Msg::ToggleInfer
                | Msg::KeepAllInGroup => {
                    self.dirty = false;
                    return vec![];
                }
                _ => {}
            }
        }
        match msg {
            Msg::Quit => {
                self.should_quit = true;
                vec![]
            }
            Msg::Interrupt => {
                if self.dupes.searching() {
                    self.cancel_dupes()
                } else if self.dashboard.scan.running && !self.dashboard.scan.cancelling {
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
                if self.confirm_flush {
                    self.confirm_flush = false;
                    vec![]
                } else if self.show_help {
                    self.show_help = false;
                    vec![]
                } else if self.screen == Screen::Browser && !self.browser.query.text.is_empty() {
                    self.browser.set_text(String::new())
                } else if self.dupes.searching() {
                    self.cancel_dupes()
                } else if self.organize.phase == OrgPhase::Planning {
                    vec![Cmd::CancelPlan]
                } else if self.dashboard.scan.running {
                    self.cancel_scan()
                } else if self.screen == Screen::Browser {
                    // nothing left to close: hand the arrow keys to the tabs
                    self.tab_mode = true;
                    vec![]
                } else {
                    self.dirty = false;
                    vec![]
                }
            }
            Msg::TabMode => {
                self.tab_mode = true;
                vec![]
            }
            Msg::PrevScreen => self.update(Msg::Goto(self.screen.step(false))),
            Msg::NextScreen => self.update(Msg::Goto(self.screen.step(true))),
            // ←/→ change the tab, except on the folder tree where they collapse / expand.
            Msg::Left if !self.arrows_move_tree() => {
                self.update(Msg::Goto(self.screen.step(false)))
            }
            Msg::Right if !self.arrows_move_tree() => {
                self.update(Msg::Goto(self.screen.step(true)))
            }
            Msg::Goto(s) => {
                self.dirty = self.screen != s || self.show_help;
                self.screen = s;
                self.show_help = false;
                self.browser.editing_filter = false;
                if s == Screen::History {
                    vec![Cmd::LoadHistory]
                } else {
                    vec![]
                }
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
            Msg::FullRescan => {
                if !self.dashboard.scan.running {
                    self.confirm_flush = true;
                    self.show_help = false;
                } else {
                    self.dirty = false;
                }
                vec![]
            }
            Msg::ConfirmYes => {
                if std::mem::take(&mut self.confirm_flush) {
                    self.begin_scan(true)
                } else {
                    vec![]
                }
            }
            Msg::ConfirmNo => {
                self.confirm_flush = false;
                // Declined at start-up (`--force`): carry on as if the flag was not given.
                self.open_decision()
            }
            Msg::CancelScan if self.dupes.searching() => self.cancel_dupes(),
            Msg::CancelScan => self.cancel_scan(),
            Msg::ToggleDebug => {
                self.debug = !self.debug;
                vec![]
            }
            Msg::ToggleWrite => self.toggle_write(),
            Msg::EditTemplate | Msg::EditDest | Msg::ToggleTransfer | Msg::ToggleInfer
                if self.screen == Screen::Organize =>
            {
                match msg {
                    Msg::EditTemplate => self.organize.start_edit(Editing::Template),
                    Msg::EditDest => self.organize.start_edit(Editing::Dest),
                    Msg::ToggleTransfer => self.organize.toggle_transfer(),
                    _ => self.organize.toggle_infer(),
                }
                vec![]
            }
            Msg::EditTemplate | Msg::EditDest | Msg::ToggleTransfer | Msg::ToggleInfer => {
                self.dirty = false;
                vec![]
            }
            Msg::NextRule if self.screen == Screen::Organize => {
                self.organize.next_preset();
                vec![]
            }
            Msg::Enter if self.screen == Screen::Organize => self.plan_organize(),
            Msg::ReviewPlan if self.screen == Screen::Organize => self.review_organize(),
            Msg::ReviewPlan if self.screen == Screen::Duplicates => self.review_plan(),
            Msg::SavePlan if self.screen == Screen::Organize => self.save_organize_plan(),
            Msg::SavePlan if self.screen == Screen::Duplicates => self.save_dupes_plan(),
            Msg::Remember if self.screen == Screen::Duplicates => {
                let chain = self.dupes.chain_text().to_string();
                vec![Cmd::Remember(Remember::KeepRules(chain))]
            }
            Msg::Remember if self.screen == Screen::Organize => {
                vec![Cmd::Remember(Remember::Organize {
                    template: self.organize.template.clone(),
                    mode: yojaka_core::organize::transfer_word(self.organize.transfer).into(),
                })]
            }
            Msg::ExportView if self.screen == Screen::Browser => {
                if self.dashboard.no_index {
                    self.say("Scan this folder first".into());
                    vec![]
                } else {
                    vec![Cmd::ExportView(Box::new(self.browser.query.clone()))]
                }
            }
            Msg::UndoRun if self.screen == Screen::History => self.ask_undo(),
            Msg::ReviewPlan | Msg::UndoRun | Msg::SavePlan | Msg::Remember | Msg::ExportView => {
                self.dirty = false;
                vec![]
            }
            Msg::Up | Msg::Down | Msg::PageUp | Msg::PageDown | Msg::Top | Msg::Bottom
                if self.screen == Screen::History =>
            {
                match msg {
                    Msg::Up => self.history.move_by(-1),
                    Msg::Down => self.history.move_by(1),
                    Msg::PageUp => self.history.page(false),
                    Msg::PageDown => self.history.page(true),
                    Msg::Top => self.history.jump(false),
                    _ => self.history.jump(true),
                }
                vec![]
            }
            Msg::Up
            | Msg::Down
            | Msg::PageUp
            | Msg::PageDown
            | Msg::Top
            | Msg::Bottom
            | Msg::Tab
            | Msg::Enter
            | Msg::Toggle
            | Msg::NextRule
            | Msg::CycleTier
            | Msg::KeepAllInGroup
                if self.screen == Screen::Duplicates =>
            {
                self.dupes_key(msg)
            }
            Msg::FindDupes if self.screen == Screen::Duplicates => self.find_dupes(false),
            Msg::FindSimilar if self.screen == Screen::Duplicates => self.find_dupes(true),
            Msg::Toggle
            | Msg::FindDupes
            | Msg::FindSimilar
            | Msg::NextRule
            | Msg::CycleTier
            | Msg::KeepAllInGroup => {
                self.dirty = false;
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
            Msg::Left => self.browser.tree_collapse_or_parent(),
            Msg::Right => self.browser.tree_expand_or_child(),
            Msg::Tab => {
                self.browser.focus = match self.browser.focus {
                    Focus::Tree => Focus::Table,
                    Focus::Table => Focus::Tree,
                };
                vec![]
            }
            Msg::Enter if self.browser.focus == Focus::Tree => {
                self.browser.focus = Focus::Table;
                vec![]
            }
            Msg::Enter => self.open_selected(),
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
        let expired = self
            .notice
            .as_ref()
            .is_some_and(|(_, until)| self.ticks >= *until);
        if expired {
            self.notice = None;
        }
        if !self.dashboard.scan.running {
            self.dirty = expired;
            return vec![];
        }
        // The spinner and the elapsed rate move even when no event arrives.
        if self.ticks.is_multiple_of(LIVE_STATS_TICKS) && self.live_stats_due() {
            return self.request_stats();
        }
        vec![]
    }

    fn on_core(&mut self, ev: CoreEvent) -> Vec<Cmd> {
        // Applying or undoing a plan reports through the same channel; it has its own box.
        match &ev {
            CoreEvent::Progress {
                task: TaskKind::Execute | TaskKind::Undo,
                done,
                total,
                ..
            } => {
                if let Some(Modal::Running(r)) = &mut self.modal {
                    r.done = *done;
                    r.total = *total;
                }
                return vec![];
            }
            CoreEvent::TaskStarted {
                task: TaskKind::Execute | TaskKind::Undo,
                ..
            } => return vec![],
            CoreEvent::TaskFinished {
                task: TaskKind::Execute | TaskKind::Undo,
                ..
            } => {
                self.dirty = false;
                return vec![];
            }
            CoreEvent::Progress {
                task: TaskKind::Plan,
                ..
            }
            | CoreEvent::TaskStarted {
                task: TaskKind::Plan,
                ..
            } => {
                self.dirty = false;
                return vec![];
            }
            CoreEvent::TaskFinished {
                task: TaskKind::Plan,
                ..
            } => {
                self.dirty = false;
                return vec![];
            }
            CoreEvent::Issue(_) if self.is_busy_writing() => {
                self.dirty = false;
                return vec![];
            }
            _ => {}
        }
        // The duplicate search has its own progress; it must not disturb the scan gauge.
        match &ev {
            CoreEvent::Progress {
                task: TaskKind::Hash,
                done,
                total,
                bytes,
                ..
            } => {
                self.dupes.progress(*done, *total, *bytes);
                return vec![];
            }
            CoreEvent::TaskStarted {
                task: TaskKind::Hash,
                ..
            } => return vec![],
            CoreEvent::TaskFinished {
                task: TaskKind::Hash,
                ..
            } => {
                self.dirty = false;
                return vec![];
            }
            CoreEvent::Issue(_) if self.dupes.searching() && !self.dashboard.scan.running => {
                self.dupes.issues += 1;
                return vec![];
            }
            _ => {}
        }
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
                let now = Instant::now();
                s.last_event = Some(now);
                s.update_rate(now, done);
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
                        self.dupes.reset();
                        self.organize.reset();
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
            Bg::DupesFinished(outcome) => {
                match outcome {
                    DupOutcome::Done(report) => {
                        let skipped = report.stats.pictures_unsupported;
                        let looked = report.stats.pictures_hashed + report.stats.pictures_cached;
                        self.dupes.finish(*report);
                        let undecided = self.dupes.undecided();
                        if looked > 0 && skipped > 0 {
                            self.say(format!(
                                "{} pictures are in a format that cannot be compared by look",
                                crate::fmt::thousands(skipped)
                            ));
                        } else if undecided > 0 {
                            self.say(format!(
                                "{} groups are not identical copies: mark those yourself",
                                crate::fmt::thousands(undecided as u64)
                            ));
                        }
                    }
                    DupOutcome::Cancelled => self.dupes.cancelled(),
                    DupOutcome::Failed(e) => self.dupes.failed(e),
                }
                vec![]
            }
            Bg::Notice(m) => {
                self.say(m);
                vec![]
            }
            Bg::Planned(result) => {
                match result {
                    Ok(p) => {
                        self.organize.plan = Some(p);
                        self.organize.phase = OrgPhase::Ready;
                    }
                    Err(e) => self.organize.phase = OrgPhase::Failed(e),
                }
                vec![]
            }
            Bg::History(runs) => {
                self.history.set(runs);
                vec![]
            }
            Bg::ExecFinished(outcome) => {
                let lines = match outcome {
                    ExecOutcome::Applied(r) => modal::apply_report(&r),
                    ExecOutcome::Undone(r) => modal::undo_report(&r),
                    ExecOutcome::Failed(e) => vec![modal::ReportLine {
                        text: format!("Nothing more was changed: {e}"),
                        bad: true,
                    }],
                };
                self.modal = Some(Modal::Report(lines));
                // The files on disk changed: what was found is stale, and so is the index.
                self.dupes.reset();
                self.organize.reset();
                let mut cmds = vec![Cmd::LoadHistory];
                cmds.extend(self.start_scan());
                cmds
            }
            Bg::Worker(r) => self.on_response(r),
        }
    }

    fn on_response(&mut self, r: Response) -> Vec<Cmd> {
        match r {
            Response::Stats(st) => {
                self.dashboard.stats_pending = false;
                if let Some(t) = self.dashboard.stats_asked.take() {
                    self.dashboard.stats_cost = t.elapsed();
                }
                self.dashboard.stats_answered_tick = Some(self.ticks);
                self.dashboard.no_index = false;
                self.dashboard.error = None;
                self.dashboard.stats = Some(st);
                self.open_decision()
            }
            Response::NoIndex => {
                self.dashboard.stats_pending = false;
                self.dashboard.no_index = true;
                self.browser.total = Some(0);
                self.browser.window = None;
                self.open_decision()
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

/// A scan older than this counts as stale for `--refresh`.
const STALE_SECS: i64 = 3600;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn is_stale(scanned_at: i64, now: i64) -> bool {
    now - scanned_at > STALE_SECS
}

fn outcome_line(s: &ScanSummary) -> String {
    let how = match s.status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "stopped early, partial results kept",
    };
    format!(
        "Last scan {how}: {} files, {} new or changed, {:.1}s",
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
        assert!(matches!(cmds.as_slice(), [Cmd::StartScan { force: false }]));
        assert!(a.dashboard.scan.running);
        // a second "no index" answer (the browser asks too) must not start another
        assert!(a.update(Msg::Bg(Bg::Worker(Response::NoIndex))).is_empty());
    }

    #[test]
    fn staleness_is_more_than_an_hour() {
        assert!(!is_stale(1_000, 1_000 + 3600));
        assert!(is_stale(1_000, 1_000 + 3601));
        assert!(!is_stale(5_000, 1_000)); // clock went backwards
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
        assert!(matches!(
            a.update(Msg::Rescan).as_slice(),
            [Cmd::StartScan { force: false }]
        ));
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
    fn eta_needs_a_known_size_and_some_data() {
        let mut v = ScanView::default();
        v.begin_phase(TaskKind::Metadata, String::new());
        v.total = Some(1000);
        v.done = 100;
        v.rate = Some(100.0);
        assert!(v.eta().is_none(), "too early to guess");
        v.started = Some(Instant::now() - Duration::from_secs(10));
        assert_eq!(v.eta().map(|d| d.as_secs()), Some(9));
        v.total = None;
        assert!(v.eta().is_none(), "no size, no ETA");
    }

    #[test]
    fn finding_files_estimate_never_shows_a_finished_bar() {
        let mut v = ScanView::default();
        v.begin_phase(TaskKind::Scan, String::new());
        v.total = Some(1000);
        v.done = 995;
        assert_eq!(v.ratio(), Some(0.99));
        assert_eq!(v.step(), 1);
    }

    #[test]
    fn finishing_is_not_a_stall() {
        let mut v = ScanView::default();
        v.begin_phase(TaskKind::Metadata, String::new());
        v.total = Some(10);
        v.done = 10;
        v.rate = Some(100.0);
        v.last_event = Some(Instant::now() - STALL_AFTER - Duration::from_secs(1));
        assert!(v.finishing());
        assert!(v.stalled_for().is_none());
        assert!(v.eta().is_none());
        assert_eq!(v.ratio(), Some(1.0));
    }

    #[test]
    fn a_stall_hides_the_eta() {
        let mut v = ScanView::default();
        v.begin_phase(TaskKind::Metadata, String::new());
        v.total = Some(1000);
        v.done = 100;
        v.rate = Some(100.0);
        v.started = Some(Instant::now() - Duration::from_secs(10));
        v.last_event = Some(Instant::now() - Duration::from_secs(6));
        assert!(v.stalled_for().is_some());
        assert!(v.eta().is_none());
    }

    #[test]
    fn unknown_size_has_no_eta() {
        let mut v = ScanView::default();
        v.begin_phase(TaskKind::Scan, String::new());
        v.total = None;
        assert!(v.ratio().is_none());
        assert!(v.eta().is_none());
    }

    #[test]
    fn silence_is_reported() {
        let mut v = ScanView::default();
        v.begin_phase(TaskKind::Scan, String::new());
        assert!(v.stalled_for().is_none());
        v.last_event = Some(Instant::now() - STALL_AFTER - Duration::from_secs(1));
        assert!(v.stalled_for().is_some());
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
    fn slow_statistics_are_asked_for_less_often() {
        let mut a = app();
        a.update(Msg::Rescan);
        a.dashboard.stats_cost = Duration::from_secs(2); // gap = 32 ticks
        a.dashboard.stats_answered_tick = Some(a.ticks);
        a.dashboard.stats_pending = false;
        let t0 = a.ticks;
        let mut first = None;
        for _ in 0..80 {
            let hit = a
                .update(Msg::Tick)
                .iter()
                .any(|c| matches!(c, Cmd::Worker(Request::Stats)));
            if hit {
                first = Some(a.ticks);
                break;
            }
        }
        let first = first.expect("a request goes out eventually");
        assert!(first - t0 >= 32, "asked too soon: {}", first - t0);
        assert!(first <= t0 + 44, "asked too late: {}", first - t0);
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
