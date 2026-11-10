//! The journal: one JSON line per step, written before and after every operation.
//!
//! `journal/<plan-id>.jsonl` holds the history; `journal/<plan-id>.plan.json` is a copy of the
//! plan, so undo and resume do not depend on the user keeping the plan file.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hash::ContentHash;
use crate::plan::{OpMode, Plan, PlanKind, opt_path_serde, path_serde};
use crate::{AppPaths, Result, YojakaError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Run {
    Apply,
    Undo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Begin,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub ts: i64,
    pub run: Run,
    pub op: u64,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// For a trashed file: where it is now. Undo puts it back from here.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_path_serde"
    )]
    pub trashed_to: Option<PathBuf>,
    /// Copies and cross-volume moves: the content hash that was verified. Undo checks it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<ContentHash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub plan_id: Uuid,
    pub ts: i64,
    #[serde(with = "path_serde")]
    pub root: PathBuf,
    pub kind: PlanKind,
    pub mode: OpMode,
    pub ops: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Line {
    Header(Header),
    Entry(Entry),
}

pub fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub fn journal_path(paths: &AppPaths, plan_id: Uuid) -> PathBuf {
    paths.journal_dir().join(format!("{plan_id}.jsonl"))
}

pub fn plan_copy_path(paths: &AppPaths, plan_id: Uuid) -> PathBuf {
    paths.journal_dir().join(format!("{plan_id}.plan.json"))
}

/// An open journal. Every append is flushed to disk before it returns.
#[derive(Debug)]
pub struct Journal {
    file: std::fs::File,
    path: PathBuf,
}

impl Journal {
    /// Starts the journal of a plan that has never run. Fails if one already exists.
    pub fn create(paths: &AppPaths, plan: &Plan) -> Result<Self> {
        std::fs::create_dir_all(paths.journal_dir())
            .map_err(|e| YojakaError::io(paths.journal_dir(), e))?;
        let path = journal_path(paths, plan.id);
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    YojakaError::Internal(format!(
                        "plan {} has already been applied; use --resume to finish it or `yojaka undo` to reverse it",
                        plan.id
                    ))
                } else {
                    YojakaError::io(&path, e)
                }
            })?;
        let copy = plan_copy_path(paths, plan.id);
        plan.save(&copy)?;
        let mut j = Self { file, path };
        j.write(&Line::Header(Header {
            plan_id: plan.id,
            ts: now(),
            root: plan.root.clone(),
            kind: plan.kind,
            mode: plan.mode,
            ops: plan.ops.len(),
            bytes: plan.summary.bytes,
        }))?;
        Ok(j)
    }

    /// Re-opens the journal of a plan that was started before (resume, undo).
    pub fn open(paths: &AppPaths, plan_id: Uuid) -> Result<Self> {
        let path = journal_path(paths, plan_id);
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| YojakaError::io(&path, e))?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write(&mut self, line: &Line) -> Result<()> {
        let mut bytes = serde_json::to_vec(line)
            .map_err(|e| YojakaError::Internal(format!("journal entry: {e}")))?;
        bytes.push(b'\n');
        // A single write call keeps a line from being split by a crash.
        self.file
            .write_all(&bytes)
            .and_then(|()| self.file.sync_data())
            .map_err(|e| YojakaError::io(&self.path, e))
    }

    pub fn append(
        &mut self,
        run: Run,
        op: u64,
        state: State,
        detail: Option<String>,
        trashed_to: Option<PathBuf>,
    ) -> Result<()> {
        self.append_hashed(run, op, state, detail, trashed_to, None)
    }

    pub fn append_hashed(
        &mut self,
        run: Run,
        op: u64,
        state: State,
        detail: Option<String>,
        trashed_to: Option<PathBuf>,
        hash: Option<ContentHash>,
    ) -> Result<()> {
        self.write(&Line::Entry(Entry {
            ts: now(),
            run,
            op,
            state,
            detail,
            trashed_to,
            hash,
        }))
    }
}

/// Reads a whole journal. A last line cut short by a crash is ignored.
pub fn read(paths: &AppPaths, plan_id: Uuid) -> Result<Vec<Line>> {
    let path = journal_path(paths, plan_id);
    read_file(&path)
}

pub(crate) fn read_file(path: &Path) -> Result<Vec<Line>> {
    let text = std::fs::read_to_string(path).map_err(|e| YojakaError::io(path, e))?;
    let mut lines = Vec::new();
    let all: Vec<&str> = text.lines().collect();
    for (i, l) in all.iter().enumerate() {
        match serde_json::from_str::<Line>(l) {
            Ok(line) => lines.push(line),
            Err(_) if i + 1 == all.len() => {}
            Err(e) => {
                return Err(YojakaError::Internal(format!(
                    "{} is damaged at line {}: {e}",
                    path.display(),
                    i + 1
                )));
            }
        }
    }
    Ok(lines)
}

/// The saved copy of the plan a journal belongs to.
pub fn load_plan(paths: &AppPaths, plan_id: Uuid) -> Result<Plan> {
    Plan::load(&plan_copy_path(paths, plan_id))
}

/// The last entry of `run` for each op, in the order the ops first appeared.
pub fn last_states(lines: &[Line], run: Run) -> Vec<Entry> {
    let mut order: Vec<u64> = Vec::new();
    let mut last: std::collections::HashMap<u64, Entry> = std::collections::HashMap::new();
    for l in lines {
        if let Line::Entry(e) = l
            && e.run == run
        {
            if !last.contains_key(&e.op) {
                order.push(e.op);
            }
            // A `begin` never hides the `trashed_to` of an earlier `done`.
            last.insert(e.op, e.clone());
        }
    }
    order.into_iter().filter_map(|o| last.remove(&o)).collect()
}
