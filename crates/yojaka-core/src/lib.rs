//! Yojaka core: all domain logic. Must never depend on UI crates.

pub mod cancel;
pub mod config;
pub mod error;
pub mod events;
pub mod index;
pub mod model;
pub mod paths;
pub mod protect;
pub mod stats;
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

    fn protected() -> protect::Protected {
        protect::Protected::system()
    }

    /// Where the index for `root` (canonical) lives.
    pub fn index_path(&self, root: &std::path::Path, local: bool) -> std::path::PathBuf {
        if local {
            root.join(".yojaka").join("index.db")
        } else {
            self.paths.index_for(root)
        }
    }

    /// Scans `opts.root` and updates its index. Refuses protected locations.
    pub fn scan(
        &self,
        opts: &scan::ScanOptions,
        sink: &EventSink,
        cancel: &CancelToken,
    ) -> Result<scan::ScanSummary> {
        let protected = Self::protected();
        let root = protected.check_root(&opts.root)?;
        let index = self.index_path(&root, opts.local_index);
        if opts.force {
            Self::flush_index(&index)?;
        }
        // Canonical, so they match the canonical paths produced by the walker.
        let own = [self.paths.data.clone(), self.paths.logs.clone()]
            .map(|p| p.canonicalize().unwrap_or(p));
        scan::run(
            &self.config,
            &own,
            &protected,
            &root,
            opts,
            &index,
            sink,
            cancel,
        )
    }

    /// Deletes Yojaka's own index files at `index` (never user files). Missing files are fine.
    fn flush_index(index: &std::path::Path) -> Result<()> {
        for suffix in ["", "-wal", "-shm"] {
            let mut name = index.as_os_str().to_os_string();
            name.push(suffix);
            let file = std::path::PathBuf::from(name);
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(YojakaError::io(file, e)),
            }
        }
        tracing::info!(index = %index.display(), "index flushed (--force)");
        Ok(())
    }

    /// Statistics for an already scanned `root`.
    pub fn stats(&self, root: &std::path::Path, local: bool) -> Result<stats::Stats> {
        let root = Self::protected().check_root(root)?;
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        let idx = index::Index::open(&index)?;
        stats::compute(idx.connection(), &root)
    }
}
