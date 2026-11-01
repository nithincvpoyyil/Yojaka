# Yojaka — Product Spec (v0.1, 2026-10-30)

> *Yojaka* (Sanskrit: "one who joins / combines").
> A fast, memory-efficient, **safe** terminal app for indexing, de-duplicating and organizing **photo and video** files on macOS (Apple Silicon).

Source: `what-is-it.md`. This document turns that note into concrete requirements. Items marked **[ASSUMPTION]** were not stated in the original note. Confirm or change them before the iteration that needs them.

---

## 1. Goals

| # | Goal | How we measure it |
|---|------|-------------------|
| G1 | **Fast**: uses all cores (multi-threaded pipeline) | 100k media files scanned and indexed with metadata in < 30 s on an M-series SSD (see tech spec §10) |
| G2 | **Memory-efficient** | Peak RSS < 250 MB for a 1M-file tree; memory does not grow with library size (streaming + on-disk index) |
| G3 | **Safe**: nothing changes without a dry-run and explicit confirmation | Read-only by default; every change is planned, previewed, journaled and undoable |
| G4 | **Clear feedback**: the user always sees what is happening | Live progress, cancel at any time, before/after views |
| G5 | **Lean**: one small native binary, no runtime dependencies | Single static binary; 
| G6 | **Ready for growth**: GUI and LLM features can be added later without a rewrite | The core logic lives in a library crate with no UI code |

## 2. Non-goals (for now)

- Windows, Linux, Intel Macs (the code should stay portable, but we test and ship only `aarch64-apple-darwin`).
- Editing images or videos, or writing metadata into media files. Yojaka **never modifies file contents**.
- Transcoding videos, or detecting visually similar videos (frame hashing). Exact, likely and name duplicates for video **are** in v1.
- Managing the Apple Photos library. `*.photoslibrary` bundles are **excluded** by default because changing them corrupts Photos.
- LLM features. These are planned for a later stage (§7). The architecture leaves room for them.
- GUI. Planned for a later stage. The core library is shared.

## 3. Users and main scenarios

**Primary user:** a Mac user with years of photos and videos scattered across folders and drives (downloads, old phone backups, external disks, exported albums), often full of duplicates.

| ID | Scenario |
|----|----------|
| S1 | "Show me what's in this drive": open Yojaka in a folder and get counts, total size, a format breakdown (most-used formats first), date range and cameras. |
| S2 | "Find my duplicates": see duplicate groups, choose which copy to keep (manually or with a rule), and bulk-delete the rest safely. |
| S3 | "Organize this mess": choose a layout such as `YYYY/MM/` or **by place** (`India/Goa/2023-08/`), see the proposed folder tree, and move or copy files into it. |
| S4 | "Undo that": revert the last operation completely. |
| S5 | "Just tell me, don't touch anything": read-only analysis and export (CSV/JSON). This is the default mode. |

## 4. Functional requirements

### 4.1 Launch and modes
- **FR-1** `yojaka` with no arguments opens the TUI on the current directory. `yojaka <path>` opens it on that path, which may be a whole volume such as `/Volumes/Backup`.
- **FR-2** Two operating modes:
  - **Read-only** (default): scan, index, analyze and export. No filesystem writes outside Yojaka's own data folder.
  - **Write** (`--write` flag, or switching in the TUI with a confirmation): allows delete, move and copy operations. Even in write mode, every change goes through Plan → Dry-run → Confirm (FR-30).
- **FR-3** Every TUI feature also has a non-interactive CLI subcommand (`scan`, `stats`, `dupes`, `organize`, `apply`, `undo`, `export`). This allows scripting and automated tests.

### 4.2 Scan and index
- **FR-10** Walk the target recursively in parallel and detect image **and video** files by extension **and** by magic bytes, so renamed or extension-less media files are found. Each file is tagged with a media kind: `image` or `video`.
- **FR-11** Supported formats for v1: JPEG, PNG, GIF, HEIC/HEIF, WebP, TIFF, BMP, AVIF. RAW formats (DNG, CR2/CR3, NEF, ARW, RAF, ORF) are detected and indexed with EXIF where it is available.
  - **Video (v1):** MOV (QuickTime), MP4, M4V, 3GP, AVI, MKV/WebM, MTS/M2TS (AVCHD). The scan can be limited with `--kind image|video|all` (default `all`).
- **FR-12** Build a persistent **index** (SQLite) of path, size, timestamps, inode, format and metadata.
- **FR-13** **Incremental rescan**: files with unchanged size and mtime are not re-read. A rescan of an unchanged 100k-file tree takes seconds.
- **FR-14** Safe defaults while walking:
  - Do not follow symlinks.
  - Skip `.photoslibrary`, `.Trash`, `.git`, `node_modules`, `Library/` and hidden folders. The skip list can be configured.
  - **Do not read iCloud "dataless" placeholder files.** Reading them triggers a download. List them as "cloud-only" instead.
  - Treat hard links to the same inode as one file, not as duplicates. Deleting one hard link frees no space.
  - Report permission-denied paths. Never crash on them.
- **FR-14a** **Protected macOS locations (hard safety rule).** Yojaka must never run against macOS-specific or system folders. This applies to every scan root, every source file and every destination of a move, copy or delete. It covers the TUI, every CLI subcommand, and plans applied with `yojaka apply`.
  - **Protected paths:** `/` (the volume root itself), `/System`, `/Library`, `/Applications`, `/usr`, `/bin`, `/sbin`, `/etc`, `/var`, `/private`, `/opt`, `/cores`, `/dev`, `~/Library`, `~/Applications`, `~/.Trash`, and on every volume `.Spotlight-V100`, `.fseventsd`, `.Trashes`, `.DocumentRevisions-V100`, `.TemporaryItems`. App bundles (`*.app`) and `*.photoslibrary` bundles are also protected. Paths inside them are protected as well.
  - **Check on the resolved path.** Resolve symlinks, `..` and firmlinks (for example `/etc` → `/private/etc`) to the canonical path first, then compare. A symlink that points into a protected path is refused.
  - **Refuse, don't skip:** if the requested root is protected or lies inside a protected path, exit with a clear error and do nothing. A root that merely *contains* a protected folder (for example `~` or `/Volumes/Backup`) is allowed. The walker skips the protected subfolders and reports them.
  - **No override.** No flag, config setting or `--write` mode turns this off. The configurable skip list (FR-14) can add more entries but cannot remove protected ones.
  - **Plans are re-checked.** `apply` re-validates every path in a plan against the protected list, even for a plan file made earlier or edited by hand (see FR-53).
  - **Only exception:** Yojaka's own index and log folders (see Footprint in §5). These are never treated as media sources or destinations.
- **FR-15** Show live progress (files found, files processed, files per second, current folder). Allow cancel with `Esc`/`Ctrl-C`. Keep partial results.

### 4.3 Metadata
- **FR-20** Extract the following per file:
  - Dimensions.
  - Format and MIME type.
  - Color type and bit depth where cheap to get.
  - EXIF: date taken (`DateTimeOriginal`), camera make and model, lens, orientation, GPS present yes/no (coordinates stored, not shown by default).
  - PNG text chunks.
  - GIF frame count (animated or not).
- **FR-20a** Extract the following per video:
  - Duration.
  - Resolution and rotation.
  - Frame rate.
  - Video and audio codec (for example HEVC, H.264, AAC).
  - Container.
  - Creation date (QuickTime `creationdate`, or the `mvhd` creation time).
  - Camera or device make and model, and GPS, when recorded (iPhone records them).
- **FR-20b** **Live Photos:** pair an iPhone HEIC/JPEG with its companion `.MOV` (same content identifier, or the same name and date in the same folder). A pair is treated as **one item**:
  - It is moved or copied together.
  - It is deleted together.
  - Yojaka never trashes one half of a pair on its own.
- **FR-21** Statistics view:
  - Totals split into photos and videos (count, size, total video duration).
  - Count and size per format, sorted by most used.
  - Files per year and month.
  - Top cameras.
  - Top countries and cities (from GPS), and the share of files with and without location.
  - Largest files.
  - Files with no date.
  - Longest and largest videos.
- **FR-22** Export the index or any filtered view to CSV or JSON.

### 4.4 Duplicates
- **FR-30** Find duplicates in **confidence tiers**:

  | Tier | Rule | Default action allowed |
  |------|------|------------------------|
  | **Exact** | Same content hash (BLAKE3) | Bulk delete allowed |
  | **Likely** | Images: same EXIF date-taken + camera + dimensions, different bytes (for example re-saved or stripped copies). Videos: same creation date + duration (±1 s) + resolution (for example a re-exported or shared copy). | Manual review only |
  | **Name** | Normalized names match (`IMG_1234.jpg`, `IMG_1234 (1).jpg`, `IMG_1234 copy.jpg`) and sizes are within ±5% | Manual review only |
  | **Similar** (later iteration, images only) | Perceptual hash distance ≤ threshold (resized or recompressed copies) | Manual review only |

- **FR-31** **Keep rules** pick the copy to keep in each group. Rules can be chained, for example `prefer-path:~/Pictures` → `highest-resolution` → `oldest` → `shortest-path`. Available rules:
  - Oldest date taken.
  - Oldest mtime.
  - Shortest path.
  - Preferred folder.
  - Highest resolution.
  - Longest duration or highest bitrate (videos).
  - Largest file.
  - Has EXIF.
- **FR-32** In the TUI, review groups and mark keep/delete per file. Bulk-apply a rule to all groups. Show the space that will be reclaimed.
- **FR-33** Delete means **move to the macOS Trash** by default, which can be undone. Permanent delete requires `--permanent` and typing a confirmation phrase.

### 4.5 Organize (move or copy)
- **FR-40** Choose a destination root and a **path template**, for example:
  - `{year}/{month:02}/{name}`
  - `{year}/{camera}/{name}`
  - `{format}/{name}`
  - `{kind}/{year}/{month:02}/{name}`, which puts photos and videos in separate folders
  - `{country}/{city}/{year}-{month:02}/{name}`, which sorts by location (FR-44)

  The date comes from EXIF (photos) or the container creation date (videos), and falls back to file mtime. Files with no date go to `_undated/`.
- **FR-41** Choose the operation: **copy** (originals untouched), **move**, or **index-only** (just report).
- **FR-42** Resolve name conflicts automatically and never overwrite. Use `name (1).jpg`. If the existing file is an exact duplicate, skip it and report it.
- **FR-43** **Show the proposed directory tree before any change.** For large plans, show a *glimpse*:
  - The top N folders, with file counts and sizes.
  - Collapsed nodes ("… 1,284 more folders").
  - Expand on demand in the TUI.

### 4.5a Organize by location (GPS → approximate place)
- **FR-44** Turn GPS latitude and longitude into an **approximate place name** for both photos (EXIF GPS) and videos (QuickTime ISO 6709 location). The lookup is **fully offline**, using a place database built into the app (GeoNames, towns with population ≥ 1,000). Each file gets:
  - Country
  - State or region
  - District
  - Nearest town or city
  - Distance to that town (km)
- **FR-45** Location placeholders for templates: `{country}`, `{country_code}`, `{state}`, `{district}`, `{city}`.
  - **Precision guard:** if the nearest town is more than 25 km away (configurable), `{city}` becomes `Near <city>`. If it is more than 100 km away, for example at sea or in a remote area, only `{country}` or `{state}` is used, where those are known.
  - Files with no location go to `_no_location/` (the folder name can be changed).
- **FR-46** **Inferred location (opt-in, `--infer-location`):** a file without GPS borrows the place of a geotagged file when:
  - It was taken by the **same camera or phone**, **and**
  - It was taken within **±2 hours** (configurable) of the geotagged file.
  Example: DSLR shots next to geotagged iPhone shots. Inferred places are marked `≈` in the UI and the plan, and are never mixed up with real GPS.
- **FR-47** A Live Photo pair always uses one place (the photo's GPS, or the video's if the photo has none).
- **FR-48** Privacy:
  - Coordinates and place names never leave the machine. There is no online geocoding in v1.
  - CSV and JSON exports leave out GPS columns unless `--include-gps` is given.
  - The TUI shows place names. Raw coordinates appear only in the detail pane.
- **FR-49** Location can be combined with date in any order:
  - `{year}/{country}/{city}` gives year first, then place.
  - `{country}/{city}/{year}` gives place first, then year.
  The default template stays `{year}/{month:02}/{name}`.

### 4.6 Safety: Plan → Dry-run → Confirm → Execute → Journal → Undo
- **FR-50** Every change is first built as a **Plan**: a list of operations with no side effects. Plans can be saved to a file (`--plan-out plan.json`) and applied later (`yojaka apply plan.json`).
- **FR-51** The **dry-run / before-after view** shows:
  - Totals: N moves, N trashes, N copies, bytes affected, space reclaimed.
  - The before tree next to the after tree.
  - Any warnings: cross-volume moves, low free space, files changed since the scan.
- **FR-52** Confirmation:
  - The user must confirm explicitly.
  - Large or destructive plans (more than 1,000 files, or any permanent delete) need the user to type a phrase.
- **FR-53** **Re-validate before each operation.** If a file's size or mtime changed since the scan, skip it and report it.
- **FR-54** Every executed operation is written to an append-only **journal** before and after it runs. `yojaka undo [op-id]` reverts a whole operation: it moves files back and restores them from the Trash.
- **FR-55** Cross-volume moves use copy → verify hash → remove source. The source is never removed before the copy is verified.
- **FR-56** Check free space before running. Refuse to start if the destination doesn't have enough space.

### 4.7 Feedback and UX
- **FR-60** The interface always responds, even with millions of files. Heavy work runs off the UI thread.
- **FR-61** Clear status bar, a help overlay (`?`), and vim-style keys plus arrow keys.
- **FR-62** "User feedback based" **[ASSUMPTION]** is read as two things:
  1. Rich live feedback during long tasks.
  2. Remembering the user's choices (preferred keep rules, templates, excluded folders) in a config file so they become defaults next time.

  Please confirm this is what you meant.

## 5. Non-functional requirements

| Area | Requirement |
|------|-------------|
| Platform | macOS 14+ **[ASSUMPTION]**, Apple Silicon (`aarch64-apple-darwin`) only |
| Performance | See tech spec §10 targets. Benchmarks are tracked in CI. |
| Memory | Streaming pipeline with bounded channels. The index lives on disk. The UI pages through data from the index instead of loading everything. |
| Reliability | Never lose data. Crash-safe journal. A killed process can always be resumed or undone. |
| Privacy | Fully offline. No telemetry. Future LLM features use local models by default; cloud models are opt-in only. |
| System safety | Never operates on macOS system or app-support folders, with no override (FR-14a). |
| Footprint | Single binary. Data stored in `~/Library/Application Support/yojaka/`. Logs in `~/Library/Logs/yojaka/`. |

## 6. Open questions (please answer)

1. ~~**Default organize template:**~~ **DECIDED (2026-10-30):** `{year}/{month:02}/{name}` for photos and videos alike, for example `2023/08/IMG_1234.jpg`. Files with no date go to `_undated/`.
2. ~~**Videos:**~~ **DECIDED (2026-10-30):** included in v1. That covers indexing, metadata, exact, likely and name duplicates, organizing, and Live Photo pairing. Similar-video detection comes later.
3. **Index location:** central (`~/Library/Application Support/yojaka/`, which keeps scanned drives untouched) or next to the data (`<root>/.yojaka/`, which travels with an external drive)? *Proposed: central by default, with a flag to store next to the data.*
4. **Permanent delete:** should it exist at all, or Trash only?
5. **Minimum macOS version:** 14 (Sonoma) acceptable?

## 7. Future stages (out of v1 scope, but designed for)

- **LLM assist** (pluggable provider: Ollama, Apple Foundation Models, Claude, OpenAI):
  - Suggest folder or event names ("2023-08 Goa trip").
  - Natural-language search ("photos of documents/receipts").
  - Pick the best shot among near-duplicates.
  - Describe or tag images with a local vision model.
- **GUI**: a native or Tauri front end on top of `yojaka-core`.
- Similar-video detection (keyframe hashing), more metadata (XMP sidecars), watch mode.
