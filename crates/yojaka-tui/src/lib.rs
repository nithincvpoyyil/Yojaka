//! Yojaka terminal UI. Never writes to stdout/stderr outside ratatui's own drawing.

mod app;
mod event_loop;
mod keymap;
mod screens;
mod terminal;
mod theme;
mod view;

use std::path::PathBuf;

pub use app::Mode;
use yojaka_core::{Engine, EventSink};

#[derive(Debug, Clone)]
pub struct TuiOptions {
    pub mode: Mode,
}

/// Open the TUI on `root` and block until the user quits.
pub fn run(engine: Engine, root: PathBuf, opts: TuiOptions) -> std::io::Result<()> {
    tracing::info!(root = %root.display(), "tui starting");
    let (_sink, core_rx) = EventSink::channel(1024);
    let mut app = app::App::new(root, opts.mode);
    let _ = &engine; // used from Iteration 1 on (scan, stats)
    let mut guard = terminal::TerminalGuard::enter()?;
    let result = event_loop::run(&mut guard, &mut app, &core_rx);
    drop(guard);
    tracing::info!("tui stopped");
    result
}
