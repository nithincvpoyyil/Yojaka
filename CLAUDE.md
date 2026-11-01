# Yojaka

A terminal app (Rust + ratatui) for indexing, de-duplicating and organizing photo and video files on macOS (Apple Silicon only).

## Specs (read before working)
- `all-specs/what-is-it.md`: the original idea
- `all-specs/01-product-spec.md`: requirements (FR-xx), open questions
- `all-specs/02-tech-spec.md`: architecture, crates, data model, pipelines, safety engine
- `all-specs/03-iteration-plan.md`: iterations I0–I10, with acceptance criteria. **Work one iteration at a time.**
- `all-specs/04-boilerplate-impl-plan.md`: step-by-step plan for the Iteration 0 skeleton (files, crates, code shapes, CI, done checklist)

## Rules
- **Safety first.** Never write to the user's files outside the Plan → Dry-run → Confirm → Execute (journaled) path. Read-only is the default. Delete means move to the Trash. Never overwrite a file.
- `yojaka-core` holds all domain logic and must not depend on ratatui or crossterm.
- Never log to stdout or stderr while the TUI is running. Use `tracing`, which writes to a file.
- Per-file errors are collected, never fatal.
- Never open iCloud dataless files. Skip `*.photoslibrary` bundles.
- Never split a Live Photo pair (HEIC/JPEG plus MOV). The two halves are always moved, copied or deleted together.
- Never read a whole video for metadata. Seek to the `moov` box.
- Location lookup is offline only (embedded GeoNames data). Coordinates never leave the machine, and GPS is left out of exports unless `--include-gps` is given.
- Tests are run against `tools/fixturegen` trees in temp dirs, never against real user folders.

## Commands (once I0 is done)
- `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo nextest run`
- `cargo run -p yojaka-cli -- scan <path>`
