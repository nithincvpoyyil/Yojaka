# ADR-0004: TUI architecture

Status: accepted (Iteration 3)

## Context
FR-60: the UI must respond with millions of files. Tech spec §3.1/§7: Elm-style app, `TestBackend` snapshot tests, core free of UI crates.

## Decision
- **Elm loop.** `App::update(Msg) -> Vec<Cmd>` changes state and never does I/O; `view::render(&App, Frame)` only reads. The event loop (`event_loop.rs`) turns key, resize, tick, `CoreEvent` and background results into `Msg`s and runs the returned `Cmd`s (start/cancel a scan, send a request to a worker).
- **Three kinds of background threads**, all owned by the event loop: the scan thread (`Engine::scan`), a *browse* worker (counts, pages, one file's details) and a *stats* worker (statistics, folder tree). The UI thread never opens SQLite. A slow statistics query on the stats worker cannot delay scrolling on the browse worker.
- **Latest wins.** Each worker drains its queue and keeps only the newest request of each kind. Table queries carry a generation number; answers for an older query are dropped. At most one page request is in flight.
- **Windowed table.** The UI holds one 256-row window of the result plus its total, shows `…` for rows outside it and asks for the next window 48 rows before the edge. Memory stays flat at any index size.
- **Core queries live in `yojaka_core::browse`** (`Browser`), so a GUI can reuse them. `Browser` keeps one temp table with the whole result sorted for the current query. It is built on the first deep page or the first page of a column without an index, then pages are primary-key range reads. Measured on 1M rows: 5 s per deep page before, 0.8 s once per query and < 1 ms per page after (`docs/perf.md`).
- **Folder tree** is built in memory from `GROUP BY parent_folder` (one node per folder, not per file; 82 ms for 20k folders).
- **Layout is a pure function of the terminal size** (`view::browser_layout`), shared by `update` (how many rows fit) and `render`. Columns drop by priority when narrow.
- **Auto-scan only when there is no index.** An existing index is shown untouched; `r` rescans.
- **Keys:** vim keys and arrows; `t` (not `k`, which is "up") cycles the kind filter. The help overlay is generated from `keymap::HELP`.

## Consequences
- Every screen is testable without a terminal: snapshots at 80×24 and 160×48, plus `update()` tests that drive key sequences.
- A text filter with few matches scans the whole index (1.5 s at 1M rows). An FTS table could fix it later if it matters.
- The temp table lives as long as the worker's connection; a rescan sends `Reload`, which drops the connection and the table.
