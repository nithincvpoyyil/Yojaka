//! Scanning (Iteration 1). Shapes only.

use std::path::PathBuf;

use crate::model::MediaKind;

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub kinds: Vec<MediaKind>,
    pub sniff_all: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ScanSummary {
    pub files_seen: u64,
    pub files_indexed: u64,
    pub bytes: u64,
    pub issues: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    pub done: u64,
    pub bytes: u64,
}
