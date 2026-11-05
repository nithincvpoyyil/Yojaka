# ADR-0001: Workspace layout and the core boundary

Status: accepted (Iteration 0)

## Context
Yojaka starts as a terminal app and is expected to grow a GUI (Iteration 10). Safety rules (journaled writes, Trash only, never overwrite) must hold regardless of which front end drives the engine.

## Decision
- `yojaka-core` holds all domain logic and is the only crate that touches the index, files and the journal.
- `yojaka-tui` and `yojaka-cli` are thin front ends over the `Engine` facade and the `CoreEvent` channel.
- `yojaka-core` must never depend on `ratatui`, `crossterm`, `clap` or `indicatif`. Check: `cargo tree -p yojaka-core -e normal | grep -E "ratatui|crossterm|clap|indicatif"` prints nothing.
- Paths and config are injected (`AppPaths`, `Config`), never global, so tests run entirely inside temp dirs.
- `unsafe` is denied workspace-wide; the only planned exception is a scoped `#[allow]` in `yojaka-core::sys::macos` (Iterations 5–6).

## Consequences
A new front end only needs to implement an event loop. Front-end crates cannot bypass the safety engine because they have no file-operation code of their own.
