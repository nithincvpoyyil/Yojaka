//! Yojaka core: all domain logic. Must never depend on UI crates.

pub mod browse;
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
pub mod export;
pub mod geo;
pub mod hash;
pub mod livephoto;
pub mod meta;
pub mod organize;
pub mod plan;
mod pool;
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
        let mut summary = scan::run(
            &self.config,
            &own,
            &protected,
            &root,
            opts,
            &index,
            sink,
            cancel,
        )?;
        tracing::info!(?summary, "scan: finished");
        if opts.metadata && !cancel.is_cancelled() {
            summary.metadata = Some(self.run_metadata(&index, sink, cancel)?);
        }
        Ok(summary)
    }

    fn run_metadata(
        &self,
        index: &std::path::Path,
        sink: &EventSink,
        cancel: &CancelToken,
    ) -> Result<meta::MetaSummary> {
        let opts = meta::MetaOptions {
            exiftool_fallback: self.config.scan.exiftool_fallback,
        };
        meta::run(&self.config, index, opts, sink, cancel)
    }

    /// Reads metadata for files that do not have it yet (also pairs Live Photos, resolves places).
    pub fn extract_metadata(
        &self,
        root: &std::path::Path,
        local: bool,
        sink: &EventSink,
        cancel: &CancelToken,
    ) -> Result<meta::MetaSummary> {
        let root = Self::protected().check_root(root)?;
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        self.run_metadata(&index, sink, cancel)
    }

    /// Deletes the index of `root` so the next scan starts from nothing. Only Yojaka's own
    /// index files are removed, never anything in `root`. Refuses protected locations.
    pub fn flush(&self, root: &std::path::Path, local: bool) -> Result<()> {
        let root = Self::protected().check_root(root)?;
        Self::flush_index(&self.index_path(&root, local))
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
        tracing::info!(index = %index.display(), "index flushed");
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
        stats::compute(
            idx.connection(),
            &root,
            stats::StatsOptions {
                far_km: self.config.location.far_km,
            },
        )
    }

    /// Every place found in an already scanned `root` (countries and cities, most common first).
    pub fn places(&self, root: &std::path::Path, local: bool) -> Result<stats::LocationStats> {
        let root = Self::protected().check_root(root)?;
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        let idx = index::Index::open(&index)?;
        stats::location(
            idx.connection(),
            self.config.location.far_km,
            usize::MAX >> 1,
        )
    }

    /// Finds duplicates in an already scanned `root`, in the tiers `opts` asks for. Reads only
    /// files that share a size with another file (and, for the similar tier, pictures not looked
    /// at before), and saves what it learns in the index for next time. Changes no media file.
    pub fn dupes(
        &self,
        root: &std::path::Path,
        local: bool,
        opts: dupes::DupOptions,
        sink: &EventSink,
        cancel: &CancelToken,
    ) -> Result<dupes::DupReport> {
        let root = Self::protected().check_root(root)?;
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        dupes::find(&self.config, opts, &index, sink, cancel)
    }

    /// Plans how to arrange the files of an already scanned `root` under `opts.dest`. Reads the
    /// index and the destination's names; changes nothing. Refuses protected folders.
    pub fn organize(
        &self,
        root: &std::path::Path,
        local: bool,
        opts: &organize::OrganizeOptions,
        sink: &EventSink,
        cancel: &CancelToken,
    ) -> Result<organize::OrganizePlan> {
        let protected = Self::protected();
        let root = protected.check_root(root)?;
        // The destination may not exist yet: judge its nearest existing folder.
        let existing = opts
            .dest
            .ancestors()
            .find(|a| a.exists())
            .unwrap_or(&opts.dest);
        let canon = existing
            .canonicalize()
            .map_err(|e| YojakaError::io(existing, e))?;
        if let Some(reason) = protected.reason(&canon) {
            return Err(YojakaError::Protected {
                path: opts.dest.clone(),
                reason,
            });
        }
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        organize::plan(&index, &root, opts, sink, cancel)
    }

    /// A read-only handle for browsing the index of an already scanned `root`.
    pub fn browser(&self, root: &std::path::Path, local: bool) -> Result<browse::Browser> {
        let root = Self::protected().check_root(root)?;
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        browse::Browser::open(&index, root)
    }

    /// Metadata of one file, read straight from the file (no index needed).
    pub fn info(&self, file: &std::path::Path) -> Result<meta::FileReport> {
        meta::report(file, &self.config)
    }

    /// Writes the index of an already scanned `root` as CSV or JSON.
    pub fn export(
        &self,
        root: &std::path::Path,
        local: bool,
        format: export::ExportFormat,
        opts: export::ExportOptions,
        out: &mut dyn std::io::Write,
    ) -> Result<u64> {
        let root = Self::protected().check_root(root)?;
        let index = self.index_path(&root, local);
        if !index.is_file() {
            return Err(YojakaError::NoIndex(root));
        }
        let idx = index::Index::open(&index)?;
        export::export(idx.connection(), format, opts, out)
    }
}
