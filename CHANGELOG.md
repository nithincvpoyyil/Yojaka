# Changelog

All notable changes are recorded here, in [Keep a Changelog](https://keepachangelog.com) format.

## [0.1.0] — Iteration 1

### Added
- `yojaka scan [PATH] [--kind image|video|all] [--sniff-all] [--local-index]`: parallel walk (`jwalk`), extension + magic-byte detection (ISO-BMFF `ftyp` brand separates HEIC from MOV/MP4), SQLite index (schema v2: `scan`, `file`, `scan_error`), single batching writer thread, incremental rescan on `(size, mtime, ino)`, pruning of vanished files, live progress, Ctrl-C keeps partial results (exit 3, never prunes).
- `yojaka scan --force`: deletes only this folder's Yojaka index (and its -wal/-shm files) and rebuilds it from scratch. Use it when an index is damaged or comes from a newer version.
- `yojaka scan --force`: deletes only this folder's Yojaka index (and its -wal/-shm files) and rebuilds it from scratch. Use it when an index is damaged or comes from a newer version.
- `yojaka stats [PATH] [--json]`: photos/videos totals, per-format counts sorted by use, cloud-only and error counts. Hard links are counted once.
- Protected macOS locations (FR-14a) refused for every scan root and skipped while walking; app bundles, `.photoslibrary`, configured skip dirs, and Yojaka's own folders are never scanned.
- Cloud-only (dataless) files are recorded from their extension and never opened.
- Index file name now uses blake3 of the canonical root.

### Changed
- Default `scan.skip_dirs` now also has `.Trash` and `Library`.
- Spec FR-14a: `/private/var/folders` (temp) is exempt from the protected list.

## [0.0.1] — Iteration 0

### Added
- Cargo workspace: `yojaka-core`, `yojaka-tui`, `yojaka-cli` (binary `yojaka`), `yojaka-llm` (placeholder), `tools/fixturegen`, `tools/geodata` (placeholder).
- Core plumbing: errors, `CancelToken`, event channel, `AppPaths`, `Config` (load/save), SQLite index with migration runner, `Format::from_extension`, `is_dataless`.
- TUI skeleton: terminal guard with panic-safe restore, event loop, Dashboard placeholder, help overlay, READ-ONLY badge.
- CLI: the full v1 command surface (stubs name the iteration that implements them), file logging, `yojaka doctor [--json]`.
- `fixturegen` v1: deterministic JPEG/PNG/GIF (optional HEIC) trees with EXIF and GPS, duplicates, name variants, corrupt files, hard/symlinks, noise, skip-dir bait, and a ground-truth `manifest.json`.
- nextest config, `TestEnv` harness, criterion baseline bench, `cargo-deny`, GitHub Actions workflow.
