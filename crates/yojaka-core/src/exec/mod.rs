//! Journaled execution: apply a plan and list what was run.

pub mod apply;
pub mod history;
pub mod journal;
mod organize_run;
pub mod transfer;
pub mod trash;

pub use apply::{ApplyOptions, ApplyReport, OpNote, apply, check_plan};
pub use history::{RunStatus, RunSummary};
pub use trash::{DirTrash, SystemTrash, Trasher};
