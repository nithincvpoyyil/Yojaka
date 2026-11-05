//! Yojaka core: all domain logic. Must never depend on UI crates.

pub mod cancel;
pub mod config;
pub mod error;
pub mod events;
pub mod index;
pub mod model;
pub mod paths;
pub mod sys;

pub mod detect;
pub mod dupes;
pub mod exec;
pub mod geo;
pub mod hash;
pub mod livephoto;
pub mod meta;
pub mod organize;
pub mod plan;
pub mod scan;

pub use cancel::CancelToken;
pub use config::Config;
pub use error::{FileIssue, IssueKind, Result, YojakaError};
pub use events::{CoreEvent, EventSink, TaskKind, TaskOutcome};
pub use paths::AppPaths;

/// Facade used by every UI (TUI, CLI, future GUI). Methods are added per iteration.
#[derive(Debug)]
pub struct Engine {
    pub paths: AppPaths,
    pub config: Config,
}

impl Engine {
    pub fn new(paths: AppPaths, config: Config) -> Self {
        Self { paths, config }
    }
}
