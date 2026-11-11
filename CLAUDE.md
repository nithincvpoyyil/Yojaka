# Yojaka

A terminal app (Rust + ratatui) for indexing, de-duplicating and organizing photo and video files on macOS (Apple Silicon only).

## Specs (read before working)
- `all-specs/what-is-it.md`: the original idea
- `all-specs/01-product-spec.md`: requirements (FR-xx), open questions
- `all-specs/02-tech-spec.md`: architecture, crates, data model, pipelines, safety engine
- `all-specs/03-iteration-plan.md`: iterations I0–I11, with acceptance criteria. **Work one iteration at a time.**
- `all-specs/04-boilerplate-impl-plan.md`: step-by-step plan for the Iteration 0 skeleton (files, crates, code shapes, CI, done checklist)
- `all-specs/05-scan-walker-fix-plan.md`: scan hang fix (jwalk → dua-core, honest progress). Done 2026-11-09 (ADR-0005).
- `all-specs/06-i8-hardening-release-plan.md`: Iteration 8 (hardening: FR audit, saved preferences, fault injection), done; its decisions D1-D9 still apply.
- `all-specs/09-i9-features-release-plan.md`: Iteration 9 (detailed dashboard, real-library tests, release engineering, performance last, sign-off and v1.0.0).
- `all-specs/07-scan-speed-plan.md`: I9 scan speed work (scheduled last) (measure, cheap fixes, pipeline overlap, rescans), with goals.
- `all-specs/08-homebrew-tap-plan.md`: I9 Homebrew tap, release tarball and the unsigned-binary notes.
- `docs/STATUS.md`: where the work stands, keys, decisions and open items. Read it when resuming.

## Rules
- **Safety first.** Never write to the user's files outside the Plan → Dry-run → Confirm → Execute (journaled) path. Read-only is the default. Delete means move to the Trash. Never overwrite a file.
- `yojaka-core` holds all domain logic and must not depend on ratatui or crossterm.
- Never log to stdout or stderr while the TUI is running. Use `tracing`, which writes to a file.
- Per-file errors are collected, never fatal.
- Never open iCloud dataless files. Skip `*.photoslibrary` bundles.
- Only identical files (the exact tier) may be marked by a keep rule or planned from the CLI. Likely, similar and name groups are guesses: a person marks each group (ADR-0008).
- Never split a Live Photo pair (HEIC/JPEG plus MOV). The two halves are always moved, copied or deleted together.
- Never read a whole video for metadata. Seek to the `moov` box.
- Location lookup is offline only (embedded GeoNames data). Coordinates never leave the machine, and GPS is left out of exports unless `--include-gps` is given.
- Tests are run against `tools/fixturegen` trees in temp dirs, never against real user folders.

## Commands (once I0 is done)
- `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo nextest run`
- `cargo run -p yojaka-cli -- scan <path>`

## Iteration 0 reference
- Skeleton plan: `all-specs/04-boilerplate-impl-plan.md`
- Full check: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check`
- Test data: `cargo run -p fixturegen -- out/ --files 1000` (writes `out/tree` and `out/manifest.json`)
- Integration tests use `crates/yojaka-core/tests/common` (`TestEnv`)
