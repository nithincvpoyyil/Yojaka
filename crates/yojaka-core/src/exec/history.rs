//! What has been run: one summary per journal.

use std::path::PathBuf;

use uuid::Uuid;

use super::journal::{self, Line, Run, State};
use crate::plan::{Op, OpMode, PlanKind};
use crate::{AppPaths, Result, YojakaError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Every step finished.
    Applied,
    /// Stopped before the end; `apply --resume` finishes it.
    Incomplete,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Incomplete => "incomplete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub plan_id: Uuid,
    pub started: i64,
    pub root: PathBuf,
    pub kind: PlanKind,
    pub mode: OpMode,
    pub ops: usize,
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Bytes of the files that were moved, copied or removed.
    pub bytes: u64,
    pub status: RunStatus,
}

/// Summary of one plan's journal.
pub fn summarize(paths: &AppPaths, plan_id: Uuid) -> Result<RunSummary> {
    let lines = journal::read(paths, plan_id)?;
    let header = lines
        .iter()
        .find_map(|l| match l {
            Line::Header(h) => Some(h.clone()),
            Line::Entry(_) => None,
        })
        .ok_or_else(|| YojakaError::Internal(format!("journal {plan_id} has no header")))?;
    let applied = journal::last_states(&lines, Run::Apply);
    let count = |s: State| applied.iter().filter(|e| e.state == s).count();
    let done_ops: Vec<u64> = applied
        .iter()
        .filter(|e| e.state == State::Done)
        .map(|e| e.op)
        .collect();

    let bytes = match journal::load_plan(paths, plan_id) {
        Ok(plan) => plan
            .ops
            .iter()
            .filter_map(|o| match o {
                Op::Trash { id, expect, .. }
                | Op::Delete { id, expect, .. }
                | Op::Move { id, expect, .. }
                | Op::Copy { id, expect, .. }
                    if done_ops.contains(id) =>
                {
                    Some(expect.size)
                }
                _ => None,
            })
            .sum(),
        Err(_) => header.bytes,
    };

    let (done, skipped, failed) = (
        count(State::Done),
        count(State::Skipped),
        count(State::Failed),
    );
    let status = if done + skipped + failed < header.ops {
        RunStatus::Incomplete
    } else {
        RunStatus::Applied
    };
    Ok(RunSummary {
        plan_id,
        started: header.ts,
        root: header.root,
        kind: header.kind,
        mode: header.mode,
        ops: header.ops,
        done,
        skipped,
        failed,
        bytes,
        status,
    })
}

/// Every journal, newest first. A journal that cannot be read is left out (and logged).
pub fn list(paths: &AppPaths) -> Result<Vec<RunSummary>> {
    let dir = paths.journal_dir();
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".jsonl") else {
            continue;
        };
        let Ok(id) = stem.parse::<Uuid>() else {
            continue;
        };
        match summarize(paths, id) {
            Ok(s) => out.push(s),
            Err(e) => tracing::warn!("skipping journal {name}: {e}"),
        }
    }
    out.sort_by(|a, b| b.started.cmp(&a.started).then(b.plan_id.cmp(&a.plan_id)));
    Ok(out)
}

/// What [`clear`] did.
#[derive(Debug, Default, Clone)]
pub struct ClearReport {
    /// Runs whose history was deleted.
    pub removed: usize,
    /// Runs that stopped before the end. They are kept: `apply --resume` needs their journal.
    pub kept_incomplete: usize,
    /// Runs whose history could not be deleted: `(run id, reason)`.
    pub failed: Vec<(Uuid, String)>,
}

/// Deletes the history of every finished run (its journal and the copy of its plan), so the
/// History list starts again. Only Yojaka's own records go; no photo or video is touched.
///
/// Runs that stopped before the end are kept, because finishing them needs the journal. The ids of
/// the deleted runs are written to the cleared list first, so their plans still cannot be applied
/// a second time. A run that cannot be deleted is reported and the others carry on.
pub fn clear(paths: &AppPaths) -> Result<ClearReport> {
    let runs = list(paths)?;
    let finished: Vec<Uuid> = runs
        .iter()
        .filter(|r| r.status == RunStatus::Applied)
        .map(|r| r.plan_id)
        .collect();
    let mut report = ClearReport {
        kept_incomplete: runs.len() - finished.len(),
        ..Default::default()
    };
    if finished.is_empty() {
        return Ok(report);
    }
    journal::remember_cleared(paths, &finished)?;
    for id in finished {
        // The journal first: once it is gone the run has left the list.
        let problem = [
            journal::journal_path(paths, id),
            journal::plan_copy_path(paths, id),
        ]
        .into_iter()
        .find_map(|path| match std::fs::remove_file(&path) {
            Ok(()) => None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(format!("{}: {e}", path.display())),
        });
        match problem {
            None => report.removed += 1,
            Some(why) => {
                tracing::warn!("cannot delete the history of run {id}: {why}");
                report.failed.push((id, why));
            }
        }
    }
    tracing::info!(
        removed = report.removed,
        kept = report.kept_incomplete,
        failed = report.failed.len(),
        "history deleted"
    );
    Ok(report)
}

/// Finds a run by a full id or a unique prefix of one.
pub fn find(paths: &AppPaths, wanted: &str) -> Result<RunSummary> {
    let all = list(paths)?;
    let mut hits = all
        .into_iter()
        .filter(|s| s.plan_id.to_string().starts_with(wanted));
    match (hits.next(), hits.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => Err(YojakaError::Internal(format!("no run matches '{wanted}'"))),
        (Some(_), Some(_)) => Err(YojakaError::Internal(format!(
            "'{wanted}' matches more than one run; use more of the id"
        ))),
    }
}
