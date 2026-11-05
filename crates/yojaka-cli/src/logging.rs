//! `tracing` writes to a daily-rotated file. Never to stdout/stderr (the TUI owns them).

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Keep the returned guard alive until exit so buffered lines are flushed.
pub fn init(logs_dir: &Path) -> Option<WorkerGuard> {
    let appender = tracing_appender::rolling::daily(logs_dir, "yojaka.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("YOJAKA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init()
        .ok()?;
    Some(guard)
}
