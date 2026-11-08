//! Worker pools with named threads, so a `--debug` log shows which thread did what.

use crate::error::{Result, YojakaError};

/// A rayon pool whose threads are called `{name}-{n}`. Start and exit are logged at debug level.
pub(crate) fn build(name: &'static str, threads: usize) -> Result<rayon::ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(move |i| format!("{name}-{i}"))
        .start_handler(move |i| tracing::debug!("thread {name}-{i} started"))
        .exit_handler(move |i| tracing::debug!("thread {name}-{i} stopped"))
        .build()
        .map_err(|e| YojakaError::Internal(e.to_string()))
}
