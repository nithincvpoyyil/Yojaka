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
    PartlyUndone,
    Undone,
}

impl RunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Incomplete => "incomplete",
            Self::PartlyUndone => "partly undone",
            Self::Undone => "undone",
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
    pub undone: usize,
    /// Bytes of the files that were removed and not yet put back.
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
    let undone_ops: std::collections::HashSet<u64> = journal::last_states(&lines, Run::Undo)
        .into_iter()
        .filter(|e| e.state == State::Done)
        .map(|e| e.op)
        .collect();
    let done_ops: Vec<u64> = applied
        .iter()
        .filter(|e| e.state == State::Done)
        .map(|e| e.op)
        .collect();
    let undone = done_ops.iter().filter(|o| undone_ops.contains(o)).count();

    let bytes = match journal::load_plan(paths, plan_id) {
        Ok(plan) => plan
            .ops
            .iter()
            .filter_map(|o| match o {
                Op::Trash { id, expect, .. } | Op::Delete { id, expect, .. }
                    if done_ops.contains(id) && !undone_ops.contains(id) =>
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
    let status = if done > 0 && undone == done {
        RunStatus::Undone
    } else if undone > 0 {
        RunStatus::PartlyUndone
    } else if done + skipped + failed < header.ops {
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
        undone,
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
