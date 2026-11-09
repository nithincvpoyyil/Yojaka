//! Undo: put every file a run moved to the Trash back where it was.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use uuid::Uuid;

use super::apply::OpNote;
use super::journal::{self, Journal, Run, State};
use super::trash::restore;
use crate::plan::Op;
use crate::{
    AppPaths, CancelToken, CoreEvent, EventSink, FileIssue, IssueKind, Result, TaskKind,
    TaskOutcome, YojakaError,
};

#[derive(Debug, Clone, Default)]
pub struct UndoReport {
    pub plan_id: Option<Uuid>,
    pub restored: usize,
    pub bytes: u64,
    /// Restored by an earlier undo.
    pub already_undone: usize,
    /// Permanent deletes cannot be reversed.
    pub not_undoable: Vec<OpNote>,
    pub failed: Vec<OpNote>,
    pub cancelled: bool,
}

impl UndoReport {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty() && self.not_undoable.is_empty() && !self.cancelled
    }
}

/// Reverses a plan's journal, newest operation first. Safe to run again after a failure or crash.
pub fn undo(
    plan_id: Uuid,
    paths: &AppPaths,
    sink: &EventSink,
    cancel: &CancelToken,
) -> Result<UndoReport> {
    let lines = journal::read(paths, plan_id)?;
    let plan = journal::load_plan(paths, plan_id)?;
    let ops: HashMap<u64, &Op> = plan
        .ops
        .iter()
        .filter_map(|o| match o {
            Op::Trash { id, .. } | Op::Delete { id, .. } => Some((*id, o)),
            _ => None,
        })
        .collect();

    let applied = journal::last_states(&lines, Run::Apply);
    let undone: HashSet<u64> = journal::last_states(&lines, Run::Undo)
        .into_iter()
        .filter(|e| e.state == State::Done)
        .map(|e| e.op)
        .collect();

    let mut jr = Journal::open(paths, plan_id)?;
    let mut report = UndoReport {
        plan_id: Some(plan_id),
        ..Default::default()
    };
    let total = applied.iter().filter(|e| e.state != State::Skipped).count() as u64;
    sink.important(CoreEvent::TaskStarted {
        task: TaskKind::Undo,
        label: "Undoing plan".into(),
    });

    let mut seen = 0u64;
    for entry in applied.iter().rev() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let Some(op) = ops.get(&entry.op) else {
            continue;
        };
        let (path, expect) = match op {
            Op::Trash { path, expect, .. } | Op::Delete { path, expect, .. } => (path, expect),
            _ => continue,
        };
        let note = |reason: &str, list: &mut Vec<OpNote>| {
            sink.important(CoreEvent::Issue(FileIssue {
                path: path.clone(),
                kind: IssueKind::Other,
                message: reason.to_string(),
            }));
            list.push(OpNote {
                op: entry.op,
                path: path.clone(),
                reason: reason.to_string(),
            });
        };
        match entry.state {
            State::Done => {}
            State::Begin => {
                note(
                    "this step was interrupted; run `yojaka apply --resume` first",
                    &mut report.failed,
                );
                continue;
            }
            State::Failed | State::Skipped => continue,
        }
        seen += 1;
        sink.progress(CoreEvent::Progress {
            task: TaskKind::Undo,
            done: seen,
            total: Some(total),
            bytes: report.bytes,
            current: Some(path.clone()),
        });
        if undone.contains(&entry.op) {
            report.already_undone += 1;
            continue;
        }
        if matches!(op, Op::Delete { .. }) {
            note(
                "permanently deleted files cannot be restored",
                &mut report.not_undoable,
            );
            continue;
        }
        let Some(trashed) = entry.trashed_to.clone() else {
            note(
                "the Trash location was not recorded; restore it by hand from the Trash",
                &mut report.failed,
            );
            continue;
        };
        jr.append(Run::Undo, entry.op, State::Begin, None, None)?;
        let result = restore_one(&trashed, path, expect.size);
        match result {
            Ok(detail) => {
                jr.append(Run::Undo, entry.op, State::Done, detail, None)?;
                report.restored += 1;
                report.bytes += expect.size;
            }
            Err(msg) => {
                jr.append(Run::Undo, entry.op, State::Failed, Some(msg.clone()), None)?;
                note(&msg, &mut report.failed);
            }
        }
    }

    sink.important(CoreEvent::TaskFinished {
        task: TaskKind::Undo,
        outcome: if report.cancelled {
            TaskOutcome::Cancelled
        } else if report.is_clean() {
            TaskOutcome::Complete
        } else {
            TaskOutcome::Partial
        },
    });
    Ok(report)
}

fn restore_one(
    trashed: &PathBuf,
    original: &PathBuf,
    size: u64,
) -> std::result::Result<Option<String>, String> {
    let in_trash = std::fs::symlink_metadata(trashed);
    let at_home = std::fs::symlink_metadata(original);
    match (in_trash, at_home) {
        (Ok(t), Err(_)) => {
            if t.len() != size {
                return Err(format!(
                    "{} in the Trash has a different size now",
                    trashed.display()
                ));
            }
            restore(trashed, original).map_err(|e: YojakaError| e.to_string())?;
            Ok(None)
        }
        (Ok(_), Ok(_)) => Err(format!(
            "{} already exists, so the copy in the Trash was left there",
            original.display()
        )),
        (Err(_), Ok(h)) if h.len() == size => Ok(Some("already back in place".into())),
        (Err(_), _) => Err(format!(
            "{} is no longer in the Trash (it may have been emptied)",
            trashed.display()
        )),
    }
}
