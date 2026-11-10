//! Organize: turn a path template into a plan of moves or copies (Iteration 6).

pub mod planner;
pub mod template;

pub use planner::{OrganizeOptions, OrganizePlan, OrganizeStats, Transfer, plan};
pub use template::{Facts, PRESETS, Rendered, Template, UNDATED_DIR, Where, Ymd};
