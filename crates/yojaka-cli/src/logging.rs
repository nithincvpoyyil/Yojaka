//! The debug log. It exists only when `--debug` is given: one file per run in the logs folder,
//! `debug-<unix time>.log`. Never stdout/stderr while the TUI runs (the TUI owns them).
//!
//! Every line has a time, a level, the thread (name and id), the module and the source line.
//! Default level: `debug` for Yojaka's own crates, `warn` for the rest. `YOJAKA_LOG` overrides it
//! (for example `YOJAKA_LOG=trace` also logs every request and dropped progress event).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::EnvFilter;

const DEFAULT_FILTER: &str =
    "warn,yojaka_core=debug,yojaka_tui=debug,yojaka_cli=debug,yojaka=debug";

pub struct DebugLog {
    pub path: PathBuf,
    /// Keep alive until exit so buffered lines are flushed.
    _guard: WorkerGuard,
}

/// Starts the debug log. Returns `None` (and logs nothing anywhere) when it cannot be created.
pub fn init(logs_dir: &Path) -> Option<DebugLog> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let name = format!("debug-{secs}.log");
    let appender = tracing_appender::rolling::never(logs_dir, &name);
    // Not lossy: a debug log with holes is worse than a slightly slower run.
    let (writer, guard) = NonBlockingBuilder::default()
        .lossy(false)
        .thread_name("debug-log")
        .finish(appender);
    let filter =
        EnvFilter::try_from_env("YOJAKA_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_thread_names(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_env_filter(filter)
        .try_init()
        .ok()?;
    install_panic_hook();
    Some(DebugLog {
        path: logs_dir.join(name),
        _guard: guard,
    })
}

/// A panic on any thread is written to the log (with a backtrace) before the usual handling.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        tracing::error!(
            thread = thread.name().unwrap_or("unnamed"),
            location = %info.location().map(ToString::to_string).unwrap_or_default(),
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "PANIC: {info}"
        );
        previous(info);
    }));
}

/// The first lines of a debug log: what ran, on what, with which settings.
pub fn log_header(args: &[String], threads: usize) {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        git = env!("YOJAKA_GIT_SHA"),
        target = env!("YOJAKA_TARGET"),
        args = ?args,
        cores = std::thread::available_parallelism().map_or(0, |n| n.get()),
        threads_setting = threads,
        pid = std::process::id(),
        "yojaka starting (debug mode)"
    );
}
