# Changelog

All notable changes are recorded here, in [Keep a Changelog](https://keepachangelog.com) format.

## [0.0.1] — Iteration 0

### Added
- Cargo workspace: `yojaka-core`, `yojaka-tui`, `yojaka-cli` (binary `yojaka`), `yojaka-llm` (placeholder), `tools/fixturegen`, `tools/geodata` (placeholder).
- Core plumbing: errors, `CancelToken`, event channel, `AppPaths`, `Config` (load/save), SQLite index with migration runner, `Format::from_extension`, `is_dataless`.
- TUI skeleton: terminal guard with panic-safe restore, event loop, Dashboard placeholder, help overlay, READ-ONLY badge.
- CLI: the full v1 command surface (stubs name the iteration that implements them), file logging, `yojaka doctor [--json]`.
- `fixturegen` v1: deterministic JPEG/PNG/GIF (optional HEIC) trees with EXIF and GPS, duplicates, name variants, corrupt files, hard/symlinks, noise, skip-dir bait, and a ground-truth `manifest.json`.
- nextest config, `TestEnv` harness, criterion baseline bench, `cargo-deny`, GitHub Actions workflow.
