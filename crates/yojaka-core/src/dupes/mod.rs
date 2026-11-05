//! Duplicate detection (Iteration 4+). Shapes only.

use crate::model::FileId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Exact,
    Likely,
    Name,
    Similar,
}

#[derive(Debug, Clone)]
pub struct DupGroup {
    pub tier: Tier,
    pub members: Vec<FileId>,
    pub keep: Option<FileId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepRule {
    Oldest,
    Newest,
    ShortestPath,
    LargestSize,
}
