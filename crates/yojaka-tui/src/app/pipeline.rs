//! The Actions tab's jobs, driven step by step: start the work of a step, take its result, go on
//! to the next (plan `10-actions-plan.md` §10). The bookkeeping lives in `crate::actions` and the
//! screen in `screens::actions`.
//!
//! Nothing new writes here: Confirm shows the same plan the Review box would, and Run hands it to
//! the same executor (`Cmd::ApplyPlan`), only after write mode is on and the user said yes.

use yojaka_core::actions::ACTIONS;
use yojaka_core::dupes::{DupGroup, Tier, parse_rules};
use yojaka_core::exec::ApplyReport;
use yojaka_core::plan::{OpMode, Plan};
use yojaka_core::scan::{ScanStatus, ScanSummary};
use yojaka_core::stats::Stats;

use super::{App, Cmd, Mode, Msg, Screen};
use crate::actions::{DestRow, Job, KEEP_CHOICES, RunProgress, Step};
use crate::dupes::Phase as DupPhase;
use crate::fmt;
use crate::modal::{self, Modal, Review};
use crate::organize::{Editing, Phase as OrgPhase, STOPPED};
use crate::worker::ExecOutcome;

/// Countries named in the summary.
const TOP_COUNTRIES: usize = 3;
/// Ticks between redraws of the job's clock while a step waits for a choice (4 × 250 ms).
const CLOCK_TICKS: u64 = 4;

impl App {
    /// The Confirm step has its plan on screen: keys are text (the phrase, or `y`).
    pub fn pipeline_confirming(&self) -> bool {
        self.screen == Screen::Actions
            && self.modal.is_none()
            && self.actions.going(Step::Confirm)
            && self.actions.review.is_some()
    }

    /// Something on the Actions tab is moving (a spinner, a progress bar, the clock). While a
    /// step waits for a choice only the job's clock moves, so once a second is enough.
    pub(super) fn pipeline_busy(&self) -> bool {
        self.screen == Screen::Actions
            && self.actions.pipeline.as_ref().is_some_and(|p| {
                p.going(p.step())
                    && (!p.step().asks()
                        || self.actions.wants_plan()
                        || self.ticks.is_multiple_of(CLOCK_TICKS))
            })
    }

    /// ↑ ↓ PgUp PgDn g G ← → Enter on the Actions tab (not while typing or confirming).
    pub(super) fn pipeline_key(&mut self, msg: Msg) -> Vec<Cmd> {
        match msg {
            Msg::Up => self.actions.move_cursor(-1),
            Msg::Down => self.actions.move_cursor(1),
            Msg::PageUp | Msg::Top => self.actions.jump(false),
            Msg::PageDown | Msg::Bottom => self.actions.jump(true),
            Msg::Left | Msg::Right
                if self.actions.going(Step::Destination)
                    && self.actions.dest_row == DestRow::Transfer =>
            {
                self.actions.org.toggle_transfer();
            }
            Msg::Enter => return self.pipeline_enter(),
            _ => self.dirty = false,
        }
        vec![]
    }

    /// Enter: start the highlighted job, or take the current step's choice and go on.
    fn pipeline_enter(&mut self) -> Vec<Cmd> {
        let Some(p) = &self.actions.pipeline else {
            return self.start_pipeline(self.actions.job());
        };
        if p.finished {
            self.actions.close();
            return vec![];
        }
        if p.failed.is_some() {
            self.say("Press Esc to go back and change it".into());
            return vec![];
        }
        match p.step() {
            Step::Layout => {
                let act = &ACTIONS[self.actions.layout.min(ACTIONS.len() - 1)];
                self.actions.org.set_template(act.template());
                self.pipeline_complete(vec![format!("{}   {}", act.title, act.examples[0])])
            }
            Step::Keep => self.keep_chosen(),
            Step::Destination => match self.actions.dest_row {
                DestRow::Folder => {
                    self.actions.org.start_edit(Editing::Dest);
                    vec![]
                }
                DestRow::Transfer => self.destination_chosen(),
            },
            Step::Confirm => {
                self.say("Wait a moment: the plan is still being worked out".into());
                vec![]
            }
            _ => {
                self.dirty = false;
                vec![]
            }
        }
    }

    fn start_pipeline(&mut self, job: Job) -> Vec<Cmd> {
        if self.dupes.searching() {
            self.say("Wait for the duplicate search, or press c to stop it".into());
            return vec![];
        }
        if self.planning() {
            self.say("Wait for the Organize plan to be worked out".into());
            return vec![];
        }
        self.actions.start(job);
        if job == Job::Dupes {
            // The keep rule chosen on the Duplicates tab (or saved), when it is one of the four.
            self.actions.keep = if self.dupes.preset < KEEP_CHOICES.len() {
                self.dupes.preset
            } else {
                0
            };
        }
        self.enter_step()
    }

    /// The current step is done: record `result` and start the next step if it runs by itself.
    fn pipeline_complete(&mut self, result: Vec<String>) -> Vec<Cmd> {
        if let Some(p) = &mut self.actions.pipeline {
            p.complete(result);
        }
        self.enter_step()
    }

    fn pipeline_fail(&mut self, why: String) {
        if let Some(p) = &mut self.actions.pipeline {
            p.fail(why);
        }
    }

    fn finish_early(&mut self, result: &str) {
        if let Some(p) = &mut self.actions.pipeline {
            p.finish_early(vec![result.to_string()]);
        }
    }

    /// Starts the work of a step that runs by itself; a step that asks waits for the user.
    fn enter_step(&mut self) -> Vec<Cmd> {
        let Some(p) = &self.actions.pipeline else {
            return vec![];
        };
        if !p.going(p.step()) {
            return vec![];
        }
        match p.step() {
            // A scan already going (the one at start-up) is followed, not started again.
            Step::Scan if self.dashboard.scan.running => vec![],
            Step::Scan => self.begin_scan(false),
            Step::Summary => {
                let ready = !self.dashboard.scan.running && !self.dashboard.stats_pending;
                match &self.dashboard.stats {
                    Some(st) if ready => {
                        if st.photos.count + st.videos.count == 0 {
                            self.finish_early("no photos or videos in this folder");
                            return vec![];
                        }
                        let lines = summary_lines(st);
                        self.pipeline_complete(lines)
                    }
                    _ => vec![],
                }
            }
            Step::FindIdentical if self.dupes.searching() => vec![],
            Step::FindIdentical => self.find_dupes(false),
            _ => vec![],
        }
    }

    /// Esc: stop what runs, then back one step (or to the list).
    pub(super) fn pipeline_back(&mut self) -> Vec<Cmd> {
        self.dirty = true;
        let Some(p) = &self.actions.pipeline else {
            return vec![];
        };
        if p.finished {
            self.actions.close();
            return vec![];
        }
        if self.actions.running() {
            self.say("Running: Ctrl-C stops after the current file".into());
            return vec![];
        }
        let mut cmds = vec![];
        if p.failed.is_none() {
            match p.step() {
                Step::Scan => cmds.extend(self.cancel_scan()),
                Step::FindIdentical => cmds.extend(self.cancel_dupes()),
                _ => {}
            }
        }
        self.dirty = true;
        let went_back = self.actions.pipeline.as_mut().is_some_and(|p| p.back());
        if went_back {
            // Confirm's plan goes (it is worked out again); the choices stay.
            self.actions.review = None;
            self.actions.org.invalidate();
        } else {
            self.actions.close();
        }
        cmds
    }

    /// Enter on the copy / move row: the folder must be usable before Confirm plans.
    fn destination_chosen(&mut self) -> Vec<Cmd> {
        let o = &mut self.actions.org;
        match o.request() {
            Err(e) => {
                o.problem = Some(e);
                vec![]
            }
            Ok(req) => {
                o.problem = None;
                o.invalidate();
                let verb = yojaka_core::organize::transfer_word(req.transfer);
                let line = format!("{verb} to {}", fmt::home(&req.dest));
                self.pipeline_complete(vec![line])
            }
        }
    }

    /// Enter on the Keep step: one copy of each identical group stays, chosen by the rule; the
    /// others make the plan Confirm shows. Only identical files (ADR-0008).
    fn keep_chosen(&mut self) -> Vec<Cmd> {
        if self.dupes.phase != DupPhase::Done {
            self.actions.close();
            self.say("The duplicate search changed: start the job again".into());
            return vec![];
        }
        let i = self.actions.keep.min(KEEP_CHOICES.len() - 1);
        let rules = parse_rules(&[crate::dupes::PRESETS[i].1]).unwrap_or_default();
        let mut groups: Vec<DupGroup> = self
            .dupes
            .groups
            .iter()
            .filter(|g| g.tier == Tier::Exact)
            .cloned()
            .collect();
        for g in &mut groups {
            g.apply_rules(&rules);
        }
        let plan = Plan::from_dupes(&self.root, &groups);
        for w in &plan.warnings {
            tracing::warn!("plan: {}", w.message);
        }
        if plan.file_ops() == 0 {
            self.finish_early("nothing to remove");
            return vec![];
        }
        let line = format!(
            "one copy of each stays: {}",
            KEEP_CHOICES[i].0.to_lowercase()
        );
        let cmds = self.pipeline_complete(vec![line]);
        self.actions.review = Some(Review::new(plan));
        cmds
    }

    /// Keys while Confirm shows its plan.
    pub(super) fn confirm_key(&mut self, msg: Msg) -> Vec<Cmd> {
        let write = self.mode == Mode::Write;
        let Some(rv) = self.actions.review.as_mut() else {
            return vec![];
        };
        let back = matches!(msg, Msg::Back | Msg::Interrupt);
        if !write {
            match msg {
                Msg::Char('y' | 'Y') | Msg::Enter => {
                    self.modal = Some(Modal::EnableWrite { then_review: true });
                }
                Msg::Char('n' | 'N') => return self.pipeline_back(),
                _ if back => return self.pipeline_back(),
                _ => self.dirty = false,
            }
            return vec![];
        }
        let typing = rv.needs_typing();
        match msg {
            Msg::Char(c) if typing => rv.typed.push(c),
            Msg::Backspace => {
                rv.typed.pop();
            }
            Msg::Enter if rv.ready() => return self.pipeline_run(),
            Msg::Enter => {
                let phrase = rv.plan.confirm_phrase();
                self.say(format!("Type \"{phrase}\" first"));
            }
            Msg::Char('y' | 'Y') => return self.pipeline_run(),
            Msg::Char('n' | 'N') => return self.pipeline_back(),
            _ if back => return self.pipeline_back(),
            _ => self.dirty = false,
        }
        vec![]
    }

    /// Confirmed: Run takes the plan to the executor.
    fn pipeline_run(&mut self) -> Vec<Cmd> {
        if self.mode != Mode::Write {
            self.say("Write mode is off".into());
            return vec![];
        }
        let Some(rv) = self.actions.review.take() else {
            return vec![];
        };
        let plan = rv.plan;
        let line = confirm_line(&plan);
        if let Some(p) = &mut self.actions.pipeline {
            p.complete(vec![line]);
            p.run = RunProgress {
                total: Some(plan.ops.len() as u64),
                ..Default::default()
            };
        }
        vec![Cmd::ApplyPlan(Box::new(plan))]
    }

    /// Ctrl-C while Run is going: stop after the current file. Other keys wait.
    pub(super) fn running_key(&mut self, msg: &Msg) -> Vec<Cmd> {
        if matches!(msg, Msg::Interrupt)
            && let Some(p) = &mut self.actions.pipeline
            && !p.run.cancelling
        {
            p.run.cancelling = true;
            return vec![Cmd::CancelExec];
        }
        self.say("Running: Ctrl-C stops after the current file".into());
        vec![]
    }

    pub(super) fn pipeline_exec_progress(
        &mut self,
        done: u64,
        total: Option<u64>,
        bytes: u64,
        current: Option<&std::path::Path>,
    ) {
        if !self.actions.running() {
            return;
        }
        if let Some(p) = &mut self.actions.pipeline {
            p.run.done = done;
            p.run.total = total.or(p.run.total);
            p.run.bytes = bytes;
            if let Some(c) = current {
                p.run.current = Some(c.to_path_buf());
            }
        }
    }

    /// The executor finished. True when the job took the result (no Report box then).
    pub(super) fn pipeline_exec_finished(&mut self, outcome: &ExecOutcome) -> bool {
        if !self.actions.running() {
            return false;
        }
        let Some(p) = &mut self.actions.pipeline else {
            return false;
        };
        match outcome {
            ExecOutcome::Applied(r) => {
                // The Run line says what happened; the rest of the report goes under it.
                let mut lines = modal::apply_report(r);
                if !lines.is_empty() {
                    lines.remove(0);
                }
                p.report = lines;
                let line = run_line(p.job, r);
                p.complete(vec![line]);
            }
            ExecOutcome::Failed(e) => {
                p.fail(format!("Nothing more was changed: {e}"));
                p.finished = true;
            }
        }
        true
    }

    /// A scan ended. The Scan step takes it; any other scan means what the job found is stale.
    pub(super) fn pipeline_on_scan(&mut self, result: Result<&ScanSummary, &str>) -> Vec<Cmd> {
        if self.actions.running() {
            return vec![];
        }
        let Some(p) = &mut self.actions.pipeline else {
            return vec![];
        };
        if p.finished {
            return vec![];
        }
        if !p.going(Step::Scan) {
            self.actions.close();
            self.say("The folder was scanned again: start the job again".into());
            return vec![];
        }
        match result {
            Ok(sum) if sum.status == ScanStatus::Partial => {
                p.fail("the scan was stopped before the end".into());
                vec![]
            }
            Ok(sum) => {
                let changed = if sum.new_or_changed == 0 {
                    "nothing changed".to_string()
                } else {
                    format!("{} new or changed", fmt::thousands(sum.new_or_changed))
                };
                let line = format!("{} files · {changed}", fmt::thousands(sum.files_indexed));
                self.pipeline_complete(vec![line])
            }
            Err(e) if self.stale_index => {
                p.fail(format!("{e}; press R to scan from scratch"));
                vec![]
            }
            Err(e) => {
                p.fail(e.to_string());
                vec![]
            }
        }
    }

    /// New statistics: the Summary step was waiting for them.
    pub(super) fn pipeline_on_stats(&mut self) -> Vec<Cmd> {
        if self.actions.going(Step::Summary) {
            self.enter_step()
        } else {
            vec![]
        }
    }

    /// The duplicate search ended (the Duplicates tab has its results too).
    pub(super) fn pipeline_on_dupes(&mut self) -> Vec<Cmd> {
        if !self.actions.going(Step::FindIdentical) {
            return vec![];
        }
        match &self.dupes.phase {
            DupPhase::Done => {
                let exact: Vec<&DupGroup> = self
                    .dupes
                    .groups
                    .iter()
                    .filter(|g| g.tier == Tier::Exact)
                    .collect();
                if exact.is_empty() {
                    self.finish_early("no two files are identical: nothing to remove");
                    return vec![];
                }
                let extra: usize = exact.iter().map(|g| g.members.len() - 1).sum();
                let bytes: u64 = exact
                    .iter()
                    .map(|g| {
                        g.members.first().map_or(0, |m| m.total_size())
                            * (g.members.len() as u64 - 1)
                    })
                    .sum();
                let line = format!(
                    "{} groups · {} extra copies · {}",
                    fmt::thousands(exact.len() as u64),
                    fmt::thousands(extra as u64),
                    fmt::bytes(bytes)
                );
                self.pipeline_complete(vec![line])
            }
            DupPhase::Cancelled => {
                self.pipeline_fail("the search was stopped".into());
                vec![]
            }
            DupPhase::Failed(e) => {
                let e = e.clone();
                self.pipeline_fail(e);
                vec![]
            }
            _ => vec![],
        }
    }

    /// An organize plan came back for the Actions tab.
    pub(super) fn pipeline_on_planned(&mut self) {
        if !self.actions.wants_plan() {
            return;
        }
        match &self.actions.org.phase {
            OrgPhase::Ready => {
                let Some(plan) = self.actions.org.plan.as_ref().map(|p| p.plan.clone()) else {
                    return;
                };
                if plan.file_ops() == 0 {
                    self.finish_early("nothing to do: every file is already in place");
                } else {
                    self.actions.review = Some(Review::new(plan));
                }
            }
            OrgPhase::Failed(e) if e != STOPPED => {
                let why = format!("could not plan: {e}");
                self.pipeline_fail(why);
            }
            _ => {}
        }
    }
}

/// What is in the folder: counts, size, years; then the places.
fn summary_lines(st: &Stats) -> Vec<String> {
    let mut first = vec![
        format!("{} photos", fmt::thousands(st.photos.count)),
        format!("{} videos", fmt::thousands(st.videos.count)),
        fmt::bytes(st.photos.bytes + st.videos.bytes),
    ];
    let years: Vec<i32> = st.by_year.iter().map(|y| y.year).collect();
    match (years.iter().min(), years.iter().max()) {
        (Some(a), Some(b)) if a == b => first.push(a.to_string()),
        (Some(a), Some(b)) => first.push(format!("{a} – {b}")),
        _ => {}
    }
    let loc = &st.location;
    let places = if loc.countries.is_empty() {
        "no files have location (GPS) data".to_string()
    } else {
        let mut parts: Vec<String> = loc
            .countries
            .iter()
            .take(TOP_COUNTRIES)
            .map(|c| format!("{} {}", c.country, fmt::thousands(c.count)))
            .collect();
        if loc.countries.len() > TOP_COUNTRIES {
            parts.push(format!("{} more", loc.countries.len() - TOP_COUNTRIES));
        }
        if loc.without_place > 0 {
            parts.push(format!("{} without GPS", fmt::thousands(loc.without_place)));
        }
        parts.join(" · ")
    };
    vec![first.join(" · "), places]
}

/// The Run line once it is over: `315 files copied · 2 folders made · 692.7 KB`.
fn run_line(job: Job, r: &ApplyReport) -> String {
    let count = |n: usize| fmt::files(n as u64);
    let mut parts = Vec::new();
    if job == Job::Dupes {
        parts.push(format!(
            "{} moved to the Trash ({} freed)",
            count(r.trashed + r.deleted),
            fmt::bytes(r.bytes)
        ));
    } else {
        if r.copied > 0 || r.moved == 0 {
            parts.push(format!("{} copied", count(r.copied)));
        }
        if r.moved > 0 {
            parts.push(format!("{} moved", count(r.moved)));
        }
        let folders = if r.created == 1 { "folder" } else { "folders" };
        parts.push(format!("{} {folders} made", r.created));
        parts.push(fmt::bytes(r.bytes));
    }
    if !r.failed.is_empty() {
        parts.push(format!("{} failed", r.failed.len()));
    }
    if r.cancelled {
        parts.push("stopped early".into());
    }
    parts.join(" · ")
}

/// The Confirm line once confirmed: what goes where.
fn confirm_line(plan: &Plan) -> String {
    let n = fmt::files(plan.file_ops() as u64);
    let size = fmt::bytes(plan.summary.bytes);
    let dest = plan.dest.as_deref().map(fmt::home).unwrap_or_default();
    match plan.mode {
        OpMode::Copy => format!("copy {n} ({size}) → {dest}"),
        OpMode::Move => format!("move {n} ({size}) → {dest}"),
        _ => format!("move {n} ({size}) to the Trash"),
    }
}
