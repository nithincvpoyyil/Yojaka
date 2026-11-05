//! Error types shared by every Yojaka crate.

use std::path::PathBuf;

/// Fatal errors. Per-file problems use [`FileIssue`] instead.
#[derive(Debug, thiserror::Error)]
pub enum YojakaError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("index error: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("{0}")]
    IndexVersion(String),
    #[error("config error in {path}: {message}")]
    Config { path: PathBuf, message: String },
    #[error("operation cancelled")]
    Cancelled,
    /// e.g. `("dupes", "Iteration 4")`
    #[error("{0} is not available yet (planned for {1})")]
    NotYet(&'static str, &'static str),
}

pub type Result<T, E = YojakaError> = std::result::Result<T, E>;

impl YojakaError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// A per-file problem: collected and reported, never fatal.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileIssue {
    pub path: PathBuf,
    pub kind: IssueKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum IssueKind {
    PermissionDenied,
    NotFound,
    Unreadable,
    BadMetadata,
    Changed,
    Other,
}
