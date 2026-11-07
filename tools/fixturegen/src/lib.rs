//! Deterministic photo-tree generator used by every integration test and benchmark.
//!
//! Layout of `OUT_DIR`:
//! - `manifest.json`: ground truth
//! - `tree/`: the scan root (media, duplicates, links, noise, skip-dir bait)
//! - `outside/`: symlink targets that live outside the scan root

mod exif_writer;
mod rng;
mod tree;
pub mod video;

pub use tree::{Entry, Expected, Gps, Manifest, Spec, generate};

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("image encoding failed: {0}")]
    Image(String),
    #[error("{0}")]
    Other(String),
}

pub(crate) fn io<T>(path: &std::path::Path, r: std::io::Result<T>) -> Result<T, GenError> {
    r.map_err(|source| GenError::Io {
        path: path.to_path_buf(),
        source,
    })
}
