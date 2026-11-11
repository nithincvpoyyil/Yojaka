//! Yojaka terminal UI. Never writes to stdout/stderr outside ratatui's own drawing.

mod app;
mod browser;
mod dupes;
mod event_loop;
mod fmt;
mod history;
mod keymap;
mod modal;
mod organize;
mod screens;
mod terminal;
mod theme;
mod view;
mod worker;

#[cfg(test)]
mod snapshots_tests;

use std::path::PathBuf;
use std::sync::Arc;

pub use app::Mode;
use yojaka_core::Engine;

#[derive(Debug, Clone)]
pub struct TuiOptions {
    pub mode: Mode,
    /// Keep the index inside the opened folder (`--local-index`).
    pub local_index: bool,
    /// `--refresh`: scan on open when the last scan is over an hour old. Never clears anything.
    pub refresh: bool,
    /// `--force`: ask on start-up whether to wipe the folder's saved data and scan again.
    pub force: bool,
    /// Tests and tooling: use this folder as the Trash instead of the real one.
    pub trash_dir: Option<PathBuf>,
}

/// Open the TUI on `root` and block until the user quits.
pub fn run(engine: Engine, root: PathBuf, opts: TuiOptions) -> std::io::Result<()> {
    tracing::info!(root = %root.display(), "tui starting");
    let mut app = app::App::new(root, opts.mode);
    app.use_config(&engine.config);
    app.refresh_when_stale = opts.refresh;
    app.confirm_flush = opts.force;
    let mut guard = terminal::TerminalGuard::enter()?;
    let result = event_loop::run(
        &mut guard,
        &mut app,
        Arc::new(engine),
        opts.local_index,
        opts.trash_dir,
    );
    drop(guard);
    tracing::info!("tui stopped");
    result
}
