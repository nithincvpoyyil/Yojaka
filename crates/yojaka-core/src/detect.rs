//! File type detection (Iteration 1).
//!
//! Planned entry point: `fn detect(path: &Path, md: &Metadata) -> Option<Format>`, which
//! combines the extension table with a magic-byte sniff of the first few bytes only.
