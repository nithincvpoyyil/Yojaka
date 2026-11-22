//! State of the Actions tab: a short list of jobs, and the one that is going. A job is a fixed
//! row of steps run top to bottom, like a CI pipeline (plan `10-actions-plan.md` §10).
//!
//! Scan, Summary and Find identical run by themselves; Layout, Keep, Destination and Confirm wait
//! for a choice; Run applies the plan that Confirm showed. This file only keeps the books: the app
//! starts the work and reports back. Nothing here touches a file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use yojaka_core::actions::ACTIONS;

use crate::modal::{ReportLine, Review};
use crate::organize::OrganizeState;

/// The jobs on the list, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Organize,
    Dupes,
}

pub const JOBS: [Job; 2] = [Job::Organize, Job::Dupes];

impl Job {
    pub fn title(self) -> &'static str {
        match self {
            Job::Organize => "Organize into folders",
            Job::Dupes => "Remove duplicates",
        }
    }

    /// What it does, in plain words, under its name on the list.
    pub fn description(self) -> &'static str {
        match self {
            Job::Organize => {
                "Copies or moves every photo and video into new folders: by place, by year, or \
                 all in one. Identical files are copied once."
            }
            Job::Dupes => {
                "Finds files with exactly the same bytes and moves the extra copies to the Trash, \
                 keeping one of each. Look-alikes are left for the Duplicates tab."
            }
        }
    }

    pub fn steps(self) -> &'static [Step] {
        match self {
            Job::Organize => &[
                Step::Scan,
                Step::Summary,
                Step::Layout,
                Step::Destination,
                Step::Confirm,
                Step::Run,
            ],
            Job::Dupes => &[
                Step::Scan,
                Step::Summary,
                Step::FindIdentical,
                Step::Keep,
                Step::Confirm,
                Step::Run,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Scan,
    Summary,
    Layout,
    Destination,
    FindIdentical,
    Keep,
    Confirm,
    Run,
}

impl Step {
    pub fn label(self) -> &'static str {
        match self {
            Step::Scan => "Scan",
            Step::Summary => "Summary",
            Step::Layout => "Layout",
            Step::Destination => "Destination",
            Step::FindIdentical => "Find identical",
            Step::Keep => "Keep which copy",
            Step::Confirm => "Confirm",
            Step::Run => "Run",
        }
    }

    /// The name in a stage box when the full names do not fit across the screen.
    pub fn short_label(self) -> &'static str {
        match self {
            Step::FindIdentical => "Find",
            Step::Keep => "Keep",
            s => s.label(),
        }
    }

    /// Waits for a choice. The other steps run by themselves.
    pub fn asks(self) -> bool {
        matches!(
            self,
            Step::Layout | Step::Destination | Step::Keep | Step::Confirm
        )
    }
}

/// How a step looks on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Current,
    Done,
    Failed,
    /// The job ended before this step (nothing to do).
    Skipped,
}

/// The Keep step's choices: (name, what it means). Same order as the keep rules `p` cycles on the
/// Duplicates tab ([`crate::dupes::PRESETS`]), which hold the rules themselves.
pub const KEEP_CHOICES: [(&str, &str); 4] = [
    ("The oldest", "the copy with the oldest change date"),
    ("The shortest path", "the copy closest to the top folder"),
    ("The newest", "the copy with the latest change date"),
    (
        "The largest picture",
        "the copy with the highest resolution",
    ),
];

/// The Destination step's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestRow {
    Folder,
    Transfer,
}

/// Progress of the Run step, from the executor's events.
#[derive(Debug, Default, Clone)]
pub struct RunProgress {
    pub done: u64,
    pub total: Option<u64>,
    pub bytes: u64,
    pub current: Option<PathBuf>,
    pub cancelling: bool,
}

/// One job, going or finished.
#[derive(Debug)]
pub struct Pipeline {
    pub job: Job,
    /// Index into `job.steps()` of the current step (the failed one, or the last one reached).
    pub at: usize,
    /// What each finished step found or chose: one or two lines.
    pub results: Vec<Vec<String>>,
    /// How long each finished step took.
    pub took: Vec<Option<Duration>>,
    /// When the current step began.
    pub since: Instant,
    /// When the job began, and how long it took once it finished or a step failed.
    pub started: Instant,
    pub ended: Option<Duration>,
    /// Why the current step failed.
    pub failed: Option<String>,
    /// Nothing is left to do: the last step finished, the job ended early, or Run failed.
    pub finished: bool,
    pub run: RunProgress,
    /// What the run reported, under the Run line.
    pub report: Vec<ReportLine>,
}

impl Pipeline {
    pub fn new(job: Job) -> Self {
        let n = job.steps().len();
        Self {
            job,
            at: 0,
            results: vec![Vec::new(); n],
            took: vec![None; n],
            since: Instant::now(),
            started: Instant::now(),
            ended: None,
            failed: None,
            finished: false,
            run: RunProgress::default(),
            report: Vec::new(),
        }
    }

    pub fn step(&self) -> Step {
        self.job.steps()[self.at]
    }

    /// How long the job has been going, choices included; it stops when the job ends or fails.
    pub fn elapsed(&self) -> Duration {
        self.ended.unwrap_or_else(|| self.started.elapsed())
    }

    /// The current step, while the job still needs it.
    pub fn going(&self, step: Step) -> bool {
        !self.finished && self.failed.is_none() && self.step() == step
    }

    pub fn status(&self, i: usize) -> Status {
        use std::cmp::Ordering;
        match i.cmp(&self.at) {
            Ordering::Less => Status::Done,
            Ordering::Equal if self.failed.is_some() => Status::Failed,
            Ordering::Equal if self.finished => Status::Done,
            Ordering::Equal => Status::Current,
            Ordering::Greater if self.finished => Status::Skipped,
            Ordering::Greater => Status::Waiting,
        }
    }

    /// The current step is done with `result`: on to the next one, or the job is finished.
    pub fn complete(&mut self, result: Vec<String>) {
        self.results[self.at] = result;
        self.took[self.at] = Some(self.since.elapsed());
        if self.at + 1 < self.job.steps().len() {
            self.at += 1;
            self.since = Instant::now();
        } else {
            self.finished = true;
            self.ended = Some(self.started.elapsed());
        }
    }

    /// The current step is done and nothing is left to do (no identical files, every file
    /// already in place): the steps below are skipped.
    pub fn finish_early(&mut self, result: Vec<String>) {
        self.results[self.at] = result;
        self.took[self.at] = Some(self.since.elapsed());
        self.finished = true;
        self.ended = Some(self.started.elapsed());
    }

    pub fn fail(&mut self, why: String) {
        self.took[self.at] = Some(self.since.elapsed());
        self.failed = Some(why);
        self.ended = Some(self.started.elapsed());
    }

    /// Esc: back to the step before that asks for a choice, forgetting what came after it.
    /// False when there is none (the caller goes back to the list).
    pub fn back(&mut self) -> bool {
        let steps = self.job.steps();
        let Some(i) = (0..self.at).rev().find(|&i| steps[i].asks()) else {
            return false;
        };
        for j in i..steps.len() {
            self.results[j].clear();
            self.took[j] = None;
        }
        self.at = i;
        self.failed = None;
        self.finished = false;
        self.ended = None;
        self.since = Instant::now();
        true
    }
}

#[derive(Debug)]
pub struct ActionsState {
    /// The highlighted job on the list.
    pub cursor: usize,
    /// The job going (or finished and still on screen). `None`: the list.
    pub pipeline: Option<Pipeline>,
    /// Layout step: index into `ACTIONS`.
    pub layout: usize,
    /// Keep step: index into `KEEP_CHOICES`.
    pub keep: usize,
    pub dest_row: DestRow,
    /// Destination, copy / move and the organize plan. Identical files once, places from GPS.
    pub org: OrganizeState,
    /// What Confirm shows and runs, once the plan is ready (and the phrase typed so far).
    pub review: Option<Review>,
}

impl ActionsState {
    pub fn new(root: &Path) -> Self {
        let mut org = OrganizeState::new(root);
        org.skip_identical = true;
        org.infer = false;
        org.set_template(ACTIONS[0].template());
        Self {
            cursor: 0,
            pipeline: None,
            layout: 0,
            keep: 0,
            dest_row: DestRow::Folder,
            org,
            review: None,
        }
    }

    pub fn job(&self) -> Job {
        JOBS[self.cursor.min(JOBS.len() - 1)]
    }

    pub fn step(&self) -> Option<Step> {
        self.pipeline.as_ref().map(Pipeline::step)
    }

    /// The pipeline is at `step` and still needs it.
    pub fn going(&self, step: Step) -> bool {
        self.pipeline.as_ref().is_some_and(|p| p.going(step))
    }

    /// Organize is at Confirm without a plan: the app works one out (it only reads).
    pub fn wants_plan(&self) -> bool {
        self.going(Step::Confirm)
            && self
                .pipeline
                .as_ref()
                .is_some_and(|p| p.job == Job::Organize)
            && self.review.is_none()
    }

    /// Run is going: the keyboard only stops it.
    pub fn running(&self) -> bool {
        self.going(Step::Run)
    }

    pub fn start(&mut self, job: Job) {
        self.pipeline = Some(Pipeline::new(job));
        self.review = None;
        self.dest_row = DestRow::Folder;
        self.org.invalidate();
    }

    /// Back to the list: the job and its plan are dropped (a plan being worked out is stopped).
    pub fn close(&mut self) {
        self.pipeline = None;
        self.review = None;
        self.org.invalidate();
    }

    /// ↑ ↓ on the list or on the current step's choices.
    pub fn move_cursor(&mut self, delta: isize) {
        let step = |i: usize, n: usize| i.saturating_add_signed(delta).min(n - 1);
        match self.step() {
            None => self.cursor = step(self.cursor, JOBS.len()),
            Some(Step::Layout) => self.layout = step(self.layout, ACTIONS.len()),
            Some(Step::Keep) => self.keep = step(self.keep, KEEP_CHOICES.len()),
            Some(Step::Destination) if delta != 0 => {
                self.dest_row = if delta < 0 {
                    DestRow::Folder
                } else {
                    DestRow::Transfer
                };
            }
            _ => {}
        }
    }

    pub fn jump(&mut self, to_end: bool) {
        self.move_cursor(if to_end {
            isize::MAX / 2
        } else {
            isize::MIN / 2
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_the_list_with_identical_files_once_and_no_guessed_places() {
        let a = ActionsState::new(Path::new("/photos/trip"));
        assert!(a.pipeline.is_none());
        assert_eq!(a.job(), Job::Organize);
        assert!(a.org.skip_identical && !a.org.infer);
        assert!(a.org.dest.ends_with("Yojaka Organized"));
        assert!(!a.org.dest.starts_with("/photos/trip/"));
        assert_eq!(KEEP_CHOICES.len(), crate::dupes::PRESETS.len());
    }

    #[test]
    fn steps_complete_in_order_and_the_last_one_finishes_the_job() {
        let mut p = Pipeline::new(Job::Organize);
        assert_eq!(p.step(), Step::Scan);
        assert_eq!(p.status(0), Status::Current);
        assert_eq!(p.status(1), Status::Waiting);
        p.complete(vec!["12 files".into()]);
        assert_eq!((p.step(), p.status(0)), (Step::Summary, Status::Done));
        for _ in 0..5 {
            p.complete(vec![]);
        }
        assert!(p.finished);
        assert_eq!(p.step(), Step::Run);
        assert_eq!(p.status(5), Status::Done);
    }

    #[test]
    fn back_goes_to_the_previous_choice_and_forgets_what_came_after() {
        let mut p = Pipeline::new(Job::Organize);
        for _ in 0..4 {
            p.complete(vec!["x".into()]);
        }
        assert_eq!(p.step(), Step::Confirm);
        assert!(p.back());
        assert_eq!(p.step(), Step::Destination);
        assert!(p.results[3].is_empty());
        assert_eq!(p.results[2], vec!["x".to_string()], "the layout stays");
        assert!(p.back());
        assert_eq!(p.step(), Step::Layout);
        assert!(
            !p.back(),
            "Scan and Summary are not choices: back to the list"
        );

        // A failed step goes back to the choice before it.
        let mut p = Pipeline::new(Job::Dupes);
        for _ in 0..4 {
            p.complete(vec![]);
        }
        p.fail("no".into());
        assert_eq!(p.status(4), Status::Failed);
        assert!(p.back());
        assert_eq!((p.step(), p.failed.as_deref()), (Step::Keep, None));
    }

    #[test]
    fn the_job_clock_stops_when_it_ends_or_fails_and_goes_on_after_back() {
        let mut p = Pipeline::new(Job::Dupes);
        assert!(p.ended.is_none());
        p.complete(vec![]);
        assert!(p.ended.is_none(), "a step done is not the job done");
        for _ in 0..3 {
            p.complete(vec![]);
        }
        p.fail("no".into());
        let stopped = p.elapsed();
        assert_eq!(p.ended, Some(stopped));
        assert!(p.back());
        assert!(p.ended.is_none(), "back to a choice: the clock goes on");
        p.finish_early(vec![]);
        assert!(p.ended.is_some());
        assert_eq!(Step::Keep.short_label(), "Keep");
        assert_eq!(Step::Destination.short_label(), "Destination");
    }

    #[test]
    fn finishing_early_skips_the_rest() {
        let mut p = Pipeline::new(Job::Dupes);
        p.complete(vec![]);
        p.complete(vec![]);
        p.finish_early(vec!["no two files are identical".into()]);
        assert!(p.finished);
        assert_eq!(p.status(2), Status::Done);
        assert_eq!(p.status(3), Status::Skipped);
        assert!(!p.going(Step::FindIdentical));
    }

    #[test]
    fn the_cursor_moves_within_the_current_step() {
        let mut a = ActionsState::new(Path::new("/p"));
        a.move_cursor(1);
        assert_eq!(a.job(), Job::Dupes);
        a.move_cursor(5);
        assert_eq!(a.cursor, 1);
        a.start(Job::Organize);
        let p = a.pipeline.as_mut().unwrap();
        p.complete(vec![]);
        p.complete(vec![]);
        a.jump(true);
        assert_eq!(a.layout, ACTIONS.len() - 1);
        a.jump(false);
        assert_eq!(a.layout, 0);
        assert_eq!(a.cursor, 1, "the list cursor stays");
        let p = a.pipeline.as_mut().unwrap();
        p.complete(vec![]);
        a.move_cursor(1);
        assert_eq!(a.dest_row, DestRow::Transfer);
        a.move_cursor(-1);
        assert_eq!(a.dest_row, DestRow::Folder);
    }

    #[test]
    fn only_organize_at_confirm_wants_a_plan() {
        let mut a = ActionsState::new(Path::new("/p"));
        assert!(!a.wants_plan());
        a.start(Job::Organize);
        for _ in 0..4 {
            a.pipeline.as_mut().unwrap().complete(vec![]);
        }
        assert!(a.wants_plan());
        a.close();
        assert!(!a.wants_plan());
        a.start(Job::Dupes);
        for _ in 0..4 {
            a.pipeline.as_mut().unwrap().complete(vec![]);
        }
        assert!(!a.wants_plan(), "the Keep step makes the duplicate plan");
    }
}
