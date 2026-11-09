//! Pop-up questions and progress for the write path: turn on write mode, review a plan, put a
//! run back, watch it run, read the result. One at a time; while one is open it owns the keys.

use yojaka_core::exec::{ApplyReport, RunSummary, UndoReport};
use yojaka_core::plan::Plan;
use yojaka_core::plan::tree::GlimpseLine;

use crate::fmt;

#[derive(Debug)]
pub enum Modal {
    /// "Turn on write mode?"
    EnableWrite,
    Review(Box<Review>),
    /// "Put back the files of this run?"
    Undo(Box<RunSummary>),
    Running(RunView),
    Report(Vec<ReportLine>),
}

/// The dry run: what would happen, and the confirmation.
#[derive(Debug)]
pub struct Review {
    pub plan: Plan,
    pub typed: String,
    /// Removals by folder (the tree glimpse).
    pub glimpse: Vec<GlimpseLine>,
}

impl Review {
    pub fn new(plan: Plan) -> Self {
        let glimpse = plan.removal_tree().glimpse(40, 8);
        Self {
            plan,
            typed: String::new(),
            glimpse,
        }
    }

    /// Large plans need the phrase typed; small ones just a "y".
    pub fn needs_typing(&self) -> bool {
        self.plan.needs_typed_confirmation()
    }

    pub fn ready(&self) -> bool {
        !self.needs_typing() || self.typed.trim() == self.plan.confirm_phrase()
    }
}

#[derive(Debug)]
pub struct RunView {
    pub label: &'static str,
    pub done: u64,
    pub total: Option<u64>,
    pub cancelling: bool,
}

impl RunView {
    pub fn ratio(&self) -> Option<f64> {
        let t = self.total?;
        (t > 0).then(|| (self.done as f64 / t as f64).clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone)]
pub struct ReportLine {
    pub text: String,
    /// Shown in the warning colour.
    pub bad: bool,
}

fn line(text: impl Into<String>, bad: bool) -> ReportLine {
    ReportLine {
        text: text.into(),
        bad,
    }
}

/// At most this many skipped / failed files are listed; the rest are counted.
const LISTED: usize = 6;

fn problems(out: &mut Vec<ReportLine>, label: &str, notes: &[yojaka_core::exec::OpNote]) {
    for n in notes.iter().take(LISTED) {
        let name = n.path.file_name().map_or_else(
            || n.path.display().to_string(),
            |f| f.to_string_lossy().into_owned(),
        );
        out.push(line(format!("{label} {name}: {}", n.reason), true));
    }
    if notes.len() > LISTED {
        out.push(line(
            format!("… and {} more {label}", notes.len() - LISTED),
            true,
        ));
    }
}

pub fn apply_report(r: &ApplyReport) -> Vec<ReportLine> {
    let mut out = vec![line(
        format!(
            "{} files moved to the Trash ({} freed).",
            fmt::thousands((r.trashed + r.deleted) as u64),
            fmt::bytes(r.bytes)
        ),
        false,
    )];
    if r.cancelled {
        out.push(line(
            "Stopped early. Run `yojaka apply <plan> --resume --write` to finish it.",
            true,
        ));
    }
    if !r.skipped.is_empty() {
        out.push(line(
            format!("{} left alone (changed since the search):", r.skipped.len()),
            true,
        ));
        problems(&mut out, "skipped", &r.skipped);
    }
    if !r.failed.is_empty() {
        out.push(line(
            format!("{} could not be moved:", r.failed.len()),
            true,
        ));
        problems(&mut out, "failed", &r.failed);
    }
    out.push(line("Undo it any time from the History screen (5).", false));
    out
}

pub fn undo_report(r: &UndoReport) -> Vec<ReportLine> {
    let mut out = vec![line(
        format!(
            "{} files put back ({}).",
            fmt::thousands(r.restored as u64),
            fmt::bytes(r.bytes)
        ),
        false,
    )];
    if r.already_undone > 0 {
        out.push(line(
            format!("{} had been put back before.", r.already_undone),
            false,
        ));
    }
    problems(&mut out, "not restored", &r.not_undoable);
    problems(&mut out, "not restored", &r.failed);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn typing_is_only_asked_of_large_plans() {
        let mut p = empty_plan(3);
        let r = Review::new(p.clone());
        assert!(!r.needs_typing() && r.ready());
        p = empty_plan(Plan::LARGE_PLAN_FILES);
        let mut r = Review::new(p);
        assert!(r.needs_typing() && !r.ready());
        r.typed = "trash 50 files".into();
        assert!(r.ready());
        r.typed = "trash 50 file".into();
        assert!(!r.ready());
    }

    fn empty_plan(n: usize) -> Plan {
        use yojaka_core::model::Fingerprint;
        use yojaka_core::plan::{DupRef, Op};
        let mut p = Plan::from_dupes(std::path::Path::new("/p"), &[]);
        for i in 0..n {
            p.ops.push(Op::Trash {
                id: i as u64,
                path: format!("/p/{i}.jpg").into(),
                expect: Fingerprint {
                    size: 1,
                    mtime_ns: 0,
                    hash: None,
                },
                reason: DupRef {
                    group: 1,
                    kept: "/p/k.jpg".into(),
                    partner: None,
                },
            });
        }
        p
    }
}
