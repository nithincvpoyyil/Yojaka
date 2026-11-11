# Changelog

All notable changes are recorded here, in [Keep a Changelog](https://keepachangelog.com) format.

## [Unreleased]

### Iteration 8 (hardening)
- Plan: `all-specs/06-i8-hardening-release-plan.md`. Performance, real-library tests and the release moved to Iteration 9 (`09-i9-features-release-plan.md`).
- `yojaka export` can be limited: `--kind image|video`, `--year`, `--find TEXT` (name, camera or place, as in the file browser) and `--folder DIR`.
- File browser `E` exports the files shown to a new CSV in Yojaka's data folder. Duplicates and Organize `P` save the plan to a file there (`yojaka plan show <file>`); neither needs write mode.
- Two more keep rules: `largest-file` and `has-exif`.
- `docs/fr-checklist.md`: each functional requirement against the code and its tests.
- Remembered choices. `yojaka dupes --keep RULES --remember` and `yojaka organize ... --remember` save the keep rules, or the folder pattern and copy / move, in `config.toml`; in the app `M` on Duplicates or Organize does the same. They are the starting choices next time (`yojaka dupes` without `--keep`, `yojaka organize` without a pattern, and the app's first keep rule and Organize settings). Nothing is saved unless you ask. A saved value that cannot be used (a mistyped rule or pattern) is ignored with a warning and the default is used. `--copy` overrides a saved `move`. New config key `organize.mode` (`copy` or `move`).

- Fault-injection tests (Phase 3): the real program is killed with `kill -9` in the middle of `apply` and of `undo` (trash, copy and move plans) and must resume, undo and give back the identical tree; read-only destination, unwritable journal, a file swapped for a symlink, Live Photo pairs at every crash point, and (on a small disk image, run in CI) a disk that is too full or fills up during the run.

### Changed (Iteration 8)
- The default keep rules in `config.toml` are now `oldest-modified, shortest-path`, the same as the built-in default (the file used to say `oldest`, and the app ignored it). `dupes.keep_rules` is now read.

### Fixed (Iteration 8)
- After a crash in the middle of a run, `yojaka undo` used before `yojaka apply --resume` could put back one half of a Live Photo while the resume later moved the other half to the Trash. `undo` now refuses such a run, changes nothing, and prints the command that finishes it; undo it afterwards as usual. A run stopped with Ctrl-C is not affected.
- `yojaka history` printed times in UTC while the History screen used local time. Both now show local time.

### Added (Iteration 7: more duplicate tiers)
- Three more kinds of duplicate, besides identical files. They are guesses, so they are only listed for review: no rule marks them and the command line never puts them in a plan.
  - **likely**: photos with the same date taken, camera and size in pixels; videos with the same recorded date and size in pixels whose lengths differ by at most a second. Catches re-saved and re-exported copies.
  - **name**: the same name apart from what copying adds (`IMG_1234 (1).jpg`, `IMG_1234 copy.jpg`, `IMG_1234-1.jpg`), with sizes within 5%.
  - **similar** (photos, opt-in): pictures that look alike, such as resized or re-compressed copies. Reads every picture once (JPEG, PNG, GIF, WebP, TIFF, BMP, HEIC, AVIF and raw: DNG, CR2, CR3, NEF, ARW, RAF, ORF) and remembers the result. HEIC, AVIF and raw are decoded by macOS itself (ImageIO); a raw photo is compared through the camera's own preview inside it, so it matches the JPEG the camera saved with it, and only that preview is read (a few MB of the file). Pictures are turned upright first, so an iPhone portrait photo (stored sideways with a rotation flag) matches its upright copy.
- `yojaka dupes --tier exact|likely|name|similar|all`, `--similar` and `--similar-distance BITS` (0 to 32, default 6; also `dupes.similar_distance` in the config). The default is still `exact`. Groups of the other tiers print `review` with each file's size, pixels, date and camera. JSON groups carry `tier` and `needs_review`.
- Duplicates screen: `f` now looks for identical, likely and same-name copies; `F` also compares how pictures look. Each group has a tier badge and `v` shows one tier at a time. A group that is not identical shows `? review` and a comparison of its files (size, pixels, date, length, camera, differences highlighted); it is marked by hand with Space or Enter. `p` changes the rule for identical copies only.
- The Review box says how many removals are not identical to the copy that stays.
- `fixturegen --variants N`: look-alike sets (re-encoded, resized, stripped, renamed and re-exported copies, plus burst, name and length decoys) with the answers in the manifest.
- `docs/dupes-accuracy.md`: precision and recall of each tier on the fixture.

### Changed
- A plan remembers the tier of each removal. For a removal that is not an identical copy, the kept file's own size and date are recorded and checked again before anything is moved to the Trash.
- When two groups share a file, a plan removes it at most once and never removes a file another group keeps.
- Saved data from before this version cannot hold what a picture looks like: the similar search asks for a scan from scratch (`--force`). Everything else works on old saved data.


### Fixed
- Raw photos: Fuji RAF, Canon CR3 and Olympus ORF files got no metadata at all ("malformed metadata": no size, date, camera or place), and Nikon NEF files got the size of their small thumbnail (160×120). Raw files are now read by macOS itself (ImageIO), which gives the size Finder shows and reads only a few hundred KB of each file; the old EXIF reader is kept as a fallback. Raw files already scanned are read again on the next scan, once; no scan from scratch is needed.
- The metadata pass and the picture comparison never follow a symlink that replaced a file after it was opened.
- A scan could freeze for good when one folder held more than 50,000 entries (a camera dump, or `target/debug/deps` in a Rust project). The directory walker is now `dua-core`; memory stays flat.
- A folder whose own name is on the skip list (`Library`, `node_modules`, `.git`, hidden names) was scanned as empty. The skip list now applies only inside the opened folder.

### Added
- `--debug`: writes a detailed log of what Yojaka is doing to `<home>/logs/debug-<time>.log` (the path is printed at start). Each line has a time, level, thread name, module and source line. It records the start and end of each step (scan, metadata, Live Photo pairing, places, duplicates), worker and writer threads starting and stopping, UI commands, slow requests (500 ms or more), every error with the file it came from, and panics with a backtrace. `YOJAKA_LOG=trace` adds more. Without the flag no log file is written. File paths appear in the log; GPS coordinates do not.
- `R` in the TUI flushes the index of the opened folder and scans it from scratch, after a confirmation (`y` / Enter, `n` / Esc). Only Yojaka's own index files are deleted. Files whose metadata failed earlier are read again. `r` stays an incremental rescan. Core: `Engine::flush(root, local)`.
- The TUI header has a tab bar, `< 1 Dashboard  [2 Browser]  3 Duplicates … >`, with the current screen highlighted. Narrow terminals show numbers and name only the current screen.

- `yojaka --refresh [PATH]`: scans on open when the last scan is more than an hour old. It adds new and changed files and clears nothing; what is known stays on screen meanwhile. Without the flag an existing scan is shown as it is, and a never-scanned folder is scanned automatically.

### Changed
- Scan progress shows two steps, a percentage and time left when the size is known, 'Finishing up' between steps, and a red notice when nothing has moved for 5 s. Live statistics refresh less often when they are slow.
- The header no longer shows the folder path.
- Cobalt2 colors (terminalcolors.com/themes/cobalt2/default) are the default theme: dark blue background, yellow titles and keys, blue borders, blue selection bar. All colors live in `theme.rs`. Terminals without 24-bit color (Terminal.app) get the nearest of the 256 colors.
- While a scan runs, every screen shows a loader at the right of the header: a spinner, what it is doing, and a progress bar (a sliding block while the size is unknown, a filling bar with a percentage later).
- `yojaka --force [PATH]`: asks on start-up (y / n) before it forgets what is saved about the folder and scans it from scratch, which is slower than a normal scan. `n` carries on as usual. `--refresh` does not delete anything; `--force` does, after the question.
- Enter on a file in the File browser opens it with the system's default app (`/usr/bin/open`; macOS decides which). Enter in the folder pane still jumps to the files. iCloud-only files are not opened, and a missing file gets a footer message. Yojaka never changes the file.
- Dashboard: a **Top places** panel under the cameras: the most photographed cities (`City, Country` and a count), then a line with the number of countries and the files without a place. Coordinates are never shown.
- `yojaka --about` prints the MIT license and the GeoNames credit (CC BY 4.0: credit, licence link, note that the data was converted, no endorsement). The README has the same text. This closes the attribution gap of Iteration 2.
- Tab mode: `T` anywhere, or Esc in the File browser when there is nothing else to close, gives ←/→ to the tab strip (the current tab turns into a blue bar and the hint line says so). Enter, Esc, ↑ or ↓ go back into the screen. While a scan runs Esc still stops it, so use `T` then.
- `<` and `>` switch to the previous / next screen (wrapping), and so do ←/→ and Shift+←/→. The exception is the Browser's folder pane, where ←/→ collapse and expand; there only `<` `>` and Shift+←/→ switch.
- The bottom hint line shows each key in yellow and its action in a muted color.
- The screens are folder-style tabs above the panels; the current one is yellow and open at the bottom. The header now takes four rows.
- Simpler wording: no "index" in the TUI (it says "scan" and "saved"). `--local-index` is hidden from `--help`; it is only for tests and tooling.
- The header uses two rows, so the browser table and tree are one row shorter.

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
