# Changelog

All notable changes are recorded here, in [Keep a Changelog](https://keepachangelog.com) format.

## [0.3.0] — Iteration 3

### Added
- `yojaka [PATH]` opens the TUI (read-only). A folder with no index is scanned automatically the first time; an existing index is shown as it is, and `r` rescans.
- **Dashboard** (`1`): live scan gauge (walk is an animated bar, the metadata pass a real percentage) with files per second and the current folder, photo and video totals with sizes and total video time, Live Photo pairs, error / iCloud-only / located counts, format bars (most used first), year bars, top cameras. Totals refresh every 3 s while a scan runs.
- **Browser** (`2`): folder tree (lazy expand, recursive counts) and a virtualized file table. Columns: name (with a `LIVE` badge), size, taken, length, format, pixels, kind, camera, place (`≈` when inferred); columns drop from the right on narrow terminals. `/` filters by name, camera or place; `t` cycles all / photos / videos; `s` / `S` pick and reverse the sort column; detail pane shows the full metadata, Live Photo partner, codecs. GPS coordinates are never shown.
- **Help** overlay (`?`, generated from the key map), **READ-ONLY** badge in the header, status in the header, `D` debug overlay (frame time, fetch time).
- Esc / `c` cancel a running scan and keep the partial results; Ctrl-C stops the scan first, then quits. Quitting waits for the scan to flush its batch.
- Core: `yojaka_core::browse` (`Browser`, `BrowseQuery`, `FolderTree`, `FileRow`, `FileDetail`) and `Engine::browser()`: read-only paged queries over the index. Pages of an unindexed sort, or deep pages, are served from a temp table that is sorted once per query.
- Tests: `TestBackend` snapshots of every screen at 80×24 and 160×48, key-sequence tests for `update()`, a render-time test, browse integration tests, and an ignored 1M-row perf test (`browse_perf`).

### Changed
- The `k` key of tech spec §7 is `t` now: `k` is "up" in the vim keys (FR-61). Spec updated.
- `update(Msg)` returns `Vec<Cmd>` (the runtime executes them) as the spec says; before, it returned a bool.
- `TuiOptions` has `local_index` (from the global `--local-index` flag).

### Known gaps
- Duplicates, Organize and History screens are placeholders until Iterations 4, 6 and 5.
- Statistics for a 1M-row index take about 4 s; they run on their own thread, so the dashboard shows "waiting for data" meanwhile but scrolling is never blocked.
- A text filter that matches few rows scans the whole index (about 1.5 s per query at 1M rows).
- No mouse support.

## [0.2.0] — Iteration 2

### Added
- Metadata pass (runs after every `yojaka scan`; skip with `--no-metadata`): EXIF for JPEG, TIFF/RAW, HEIC/AVIF, PNG and WebP; dimensions for every image; APNG, GIF and animated WebP frame counts; video duration, resolution, rotation, frame rate, codecs, container, creation date, make/model and GPS for MOV/MP4/M4V/3GP and MKV/WebM. Optional `exiftool` for AVI/MTS (and other failures with `scan.exiftool_fallback`).
- Header reads are capped; videos are read by seeking box by box (a 4 GB video costs under 1 MB of reads wherever `moov` sits). No pixels are decoded; cloud-only files are never opened.
- Live Photo pairing (content identifier first, then same folder + stem + capture time within 2 s). Ambiguous cases stay unpaired. `meta.pair_id` is shared by both halves; a pair counts once in stats.
- Offline reverse geocoder (GeoNames `cities1000`, 171,800 places, 2.9 MB blob, own k-d tree). Country, state, district, city and distance are cached in `meta.place_*`. Live Photo halves share one place.
- `yojaka info FILE [--json]`, `yojaka places [PATH] [--json]`, `yojaka export [PATH] --format csv|json [--include-gps] [--out FILE]`. Exports leave out GPS unless `--include-gps`; CSV cells that start with `= + - @` are prefixed with `'`; `--out` never overwrites a file.
- `yojaka stats` gains: total video duration, longest and largest videos, codec breakdown, Live Photo pairs, photos by year with a month histogram, top cameras, largest files, undated count, location coverage with top countries and cities, and unreadable-metadata counts.
- Index schema v3 (`meta` table, `0003_meta.sql`). Existing indexes upgrade in place; run `yojaka scan` again to fill the metadata.
- `fixturegen --videos F --live-photos N`: synthetic iPhone-style MOV, Android-style MP4, `moov`-last, no-metadata and broken videos (sparse `mdat`, so a "4 GB" file costs no disk), Live Photo pairs, fallback pairs and decoys that must not pair. The manifest records durations and true pairs.
- `tools/geodata` builds the blob. ADR-0002 (metadata crates) and ADR-0003 (geocoder).

### Changed
- Dropped `nom-exif`, `mp4parse` and `kiddo` from the workspace (own walkers and k-d tree; see the ADRs).
- Metadata errors are collected as `BadMetadata` issues and `scan_error` rows; a file that failed is not retried until it changes.

### Known gaps
- No real iPhone, Android, RAW/DNG or Live Photo samples yet; those formats are tested with synthetic files only.
- Location inference for files without GPS (`--infer-location`) is not part of this iteration.
- GeoNames attribution text is not yet in `--about` or the README.

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
