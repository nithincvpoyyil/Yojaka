//! Journaled execution: apply a plan, undo it, and list what was run.

pub mod apply;
pub mod history;
pub mod journal;
pub mod trash;
pub mod undo;

pub use apply::{ApplyOptions, ApplyReport, OpNote, apply, check_plan};
pub use history::{RunStatus, RunSummary};
pub use trash::{DirTrash, SystemTrash, Trasher};
pub use undo::{UndoReport, undo};
