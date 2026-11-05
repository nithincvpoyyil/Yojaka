//! Plans (Iteration 5). Shapes only. Nothing is written to user files without a plan.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Op {
    Trash { path: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone)]
pub enum SkipReason {
    DestinationExists,
    SourceChanged,
    Dataless,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct Warning {
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlanSummary {
    pub ops: usize,
    pub bytes: u64,
    pub skipped: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub ops: Vec<Op>,
    pub warnings: Vec<Warning>,
    pub summary: PlanSummary,
}
