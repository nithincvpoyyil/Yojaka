//! Journaled execution (Iteration 5). Shapes only.

use crate::plan::Op;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpState {
    Pending,
    Done,
    Failed,
    Undone,
}

#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub op: Op,
    pub state: OpState,
}
