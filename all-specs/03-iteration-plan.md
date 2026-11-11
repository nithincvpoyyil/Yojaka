# Yojaka — Iteration Plan (v0.1, 2026-10-30)

Each iteration ends with a **working, demoable binary**. Earlier iterations are read-only on purpose: nothing that changes files ships until the safety engine (Iteration 5) exists.

Sizes are rough estimates for one developer working with Claude Code. **S** ≈ 2–4 days, **M** ≈ 1 week, **L** ≈ 2 weeks.

```
I0 Foundation ─► I1 Scan+Index ─► I2 Metadata+Stats ─► I3 TUI shell
                                                         │
          I6 Organize ◄─ I5 Safe executor ◄─ I4 Exact dupes (read-only) ◄┘
               │
               └─► I7 Likely/Name/Similar dupes ─► I8 Hardening ─► I9 Features+Release v1.0 ─► I10 LLM ─► I11 GUI
```

| Iter | Theme | Size | Writes to user files? |
|------|-------|------|-----------------------|
| 0 | Foundation | S | No |
| 1 | Scan and index (CLI) | M | No |
| 2 | Metadata and stats | M | No |
| 3 | TUI shell | M | No |
| 4 | Exact duplicates (review only) | M | No |
| 5 | Safe execution engine (trash, journal; undo dropped 2026-11-12, ADR-0009) | L | **Yes** |
| 6 | Organize (move/copy, tree preview) | L | Yes |
| 7 | More duplicate tiers | M | Yes |
| 8 | Hardening (FR audit, saved preferences, fault injection) | M | — |
| 9 | Features and release v1.0 (dashboard, real libraries, release, performance) | L | — |
| 10 | LLM assist | L | — |
| 11 | GUI | — | — |

The old Iteration 8 was split on 2026-11-12: hardening stayed in 8, and the rest became Iteration 9 with a feature phase in front. LLM and GUI moved from 9 and 10 to 10 and 11.

---

## Definition of Done (every iteration)
- `cargo fmt`, `cargo clippy -D warnings` and `cargo test` pass in CI on macOS arm64.
- New logic has unit tests. User-visible output has snapshot tests.
- The demo script in `docs/demos/iter-N.md` runs end to end on a fixture tree.
- The relevant spec sections are updated if anything changed.
- The `CHANGELOG.md` entry is written and the release is tagged `v0.N.0`.

---

## Iteration 0: Foundation (S)
**Goal:** a repo that is ready for fast, safe iteration.

> Detailed step-by-step plan: `04-boilerplate-impl-plan.md`.

- Install the toolchain:
  - `rustup` with the stable toolchain, `rustfmt` and `clippy`.
  - `cargo-nextest`, `cargo-deny`, `cargo-insta`, `hyperfine`.
- `git init`, `.gitignore`, and the workspace with the four crates from tech spec §2 (`yojaka-llm` as an empty placeholder).
- A `yojaka --version` binary.
- `tracing` writing to a log file, and a panic hook that restores the terminal.
- `thiserror` error type and a `CancelToken`.
- `config.rs`: load and save `config.toml` with defaults.
- `tools/fixturegen`:
  - Generates N images with real headers (small JPEG, PNG, GIF and HEIC samples, plus EXIF variants).
  - Generates synthetic MOV/MP4 files (valid `moov` boxes, with `moov` both before and after `mdat`) and Live Photo pairs. No ffmpeg is needed.
  - Nested folders, a set duplicate ratio, renamed copies, corrupted files, hard links and symlinks.
- GitHub Actions CI (macOS arm64): fmt, clippy, test, deny.
- **Spike (½ day):** compare the `reverse_geocoder` crate with our own GeoNames → `kiddo` blob (`tools/geodata`) on size, speed and admin-name quality, and pick one.
- **Spike (½ day):** test `nom-exif` against `kamadak-exif` + `mp4parse` on sample HEIC, JPEG, MOV and MP4 files, then lock the metadata crate choice in tech spec §1.
- `CLAUDE.md` with conventions (already created).

**Acceptance:**
- `cargo run -- --version` works.
- `cargo run -p fixturegen -- --files 1000 --dupes 0.1 out/` creates a tree.
- CI is green.

---

## Iteration 1: Scan and index, CLI only (M)
**Goal:** product spec S1 at the most basic level: "what photos and videos are here, and how many".
**Covers:** FR-10, FR-12, FR-13, FR-14, FR-15, plus the counts part of FR-21.

- Parallel walker (`dua-core`, ADR-0005) with skip rules, no symlink following, dataless detection, and hard link `(dev, ino)` recording.
- Detection by extension plus magic bytes (including the ISO-BMFF `ftyp` brand, which separates HEIC from MOV/MP4), with `--sniff-all` and `--kind image|video|all`.
- SQLite index: schema v1 (`scan`, `file`, `scan_error`), WAL mode, migrations, and a single writer thread with batched transactions.
- Incremental rescan using `(size, mtime, ino)`, and pruning of files that disappeared.
- Bounded channels and progress events. The CLI shows a progress bar using `indicatif`, in the CLI only.
- `yojaka scan PATH` and `yojaka stats PATH`: totals split into photos and videos, count and size per format sorted by most used, cloud-only count, and error count.
- Ctrl-C stops the scan cleanly and marks it `partial`.

**Acceptance:**
- On a 100k-file fixture: correct counts (checked against fixturegen's manifest), and a rescan is at least 5× faster than the first scan.
- The scan never opens a dataless file (test with a mocked flag check).
- Hard links are counted once.
- Peak RSS is under 150 MB on a 1M-entry tree.
- Initial walk and scan benchmark numbers are recorded in `docs/perf.md`.

---

## Iteration 2: Metadata and stats (M)
**Goal:** a rich understanding of the library.
**Covers:** FR-11, FR-20, FR-20a, FR-20b, FR-21, FR-22, FR-44, FR-47, FR-48.

- `meta` table, the `Extractor` trait, and extractors for:
  - JPEG (EXIF)
  - PNG (dimensions, text chunks, APNG frames)
  - GIF (frames)
  - HEIC/HEIF
  - WebP
  - TIFF and RAW (EXIF through the TIFF container)
  - generic (dimensions only)
  - **Video:** `isobmff` (MOV/MP4/M4V/3GP: duration, resolution, rotation, fps, codecs, creation date, make/model, GPS), `matroska` (MKV/WebM), and AVI/MTS through the exiftool fallback.
- Video extractors **seek** to `moov` and never read `mdat`.
- **Live Photo pairing** pass (content identifier first, then the name and time fallback), with a `pair_id`.
- **Location:**
  - GPS parsing for photos (EXIF) and videos (ISO 6709).
  - Embedded offline reverse geocoder, with results cached in the `meta.place_*` columns.
  - `yojaka places` command, plus location stats (top countries and cities, coverage).
  - Exports leave out GPS unless `--include-gps` is given.
- Header reads are capped. No pixels are decoded.
- `meta_status` is set per file, and errors are collected.
- Optional `--exiftool-fallback`.
- `stats` gains: total video duration, longest and largest videos, codec breakdown, Live Photo pair count, year/month histogram, top cameras, largest files, undated count, and GPS-present count.
- `yojaka export --format csv|json` and `yojaka info FILE`.

**Acceptance:**
- Each format extractor is tested against at least 3 real sample files, including at least 1 broken one. Videos include an iPhone HEVC MOV, an Android MP4, and a file whose `moov` comes after `mdat`.
- Metadata for a 4 GB video reads less than 1 MB of the file (checked with a counting reader in tests).
- Live Photo pairs in the fixture are all paired, and there are no false pairs.
- The geo test set (at least 20 known coordinates worldwide, including sea, poles, the antimeridian and border towns) all resolve as expected.
- Geocoding 100k GPS points adds less than 1 s.
- The binary grows by less than 6 MB.
- 100k media files (10% videos) are processed in under 30 s on the dev machine (warm cache), or the gap is documented with a profile.
- Stats output matches the snapshot.

---

## Iteration 3: TUI shell (M)
**Goal:** the interactive app. Typing `yojaka` in a folder works.
**Covers:** FR-1, FR-15 (UI side), FR-60, FR-61.

- `yojaka-tui`:
  - Elm-style `App`, `update` and `view`.
  - Event loop that combines crossterm input with `CoreEvent`s.
  - Terminal setup and restore.
  - Resize handling.
- Screens:
  - **Dashboard:** live scan gauge, photo and video totals, format bar chart, year chart, top cameras.
  - **Browser:** folder tree and a virtualized file table with sort, filter (including by kind) and a detail pane. Duration column for videos, and a `LIVE` badge on Live Photos.
  - **Help** overlay.
  - Header showing the **READ-ONLY** mode badge.
- Cancel and rescan from inside the UI.

**Acceptance:**
- `TestBackend` snapshot tests pass for each screen at 80×24 and 160×48.
- Smooth scrolling through a 1M-row index, with frame time under 16 ms measured in a debug overlay.
- The UI stays responsive while a scan runs.

---

## Iteration 4: Exact duplicates, review only (M)
**Goal:** product spec S2, up to "here is what I *would* delete". Nothing is deleted yet.
**Covers:** FR-30 (exact tier), FR-31, FR-32.

- Staged hashing: size groups → partial hash → full hash. Hashes are cached in the index, and hashing runs in parallel with progress.
- `DupGroup` model, keep rules with chaining and a deterministic tie-break, and reclaim calculation.
- `yojaka dupes PATH [--keep ...] [--json]`.
- **Duplicates** TUI screen: groups, members, keep/delete toggles, apply a rule to all, and the reclaim total.
- Produces an (unexecuted) `Plan` and saves it with `--plan-out`.

**Acceptance:**
- On the fixture: the groups found match fixturegen's manifest exactly, with no false positives.
- Hard links are never grouped together.
- Live Photo pairs are grouped as one item. A duplicated pair shows as one group of pairs, not two separate groups.
- Property tests confirm no plan ever marks every member of a group for deletion, and no plan ever splits a Live Photo pair.
- Only candidate files are read. Verify that bytes read are no more than the candidate bytes plus partial reads.

---

## Iteration 5: Safe execution engine (L), the first write-capable release
**Goal:** being able to safely delete duplicates, including in bulk.
**Covers:** FR-2 (write mode), FR-33, FR-50..56.

- `Plan`, `Op` and `Fingerprint`, with JSON save and load.
- Dry-run summary and before/after **tree glimpse** (the shared `DirTree` widget is reused in Iteration 6).
- **Review** modal with typed confirmation for large plans.
- Executor:
  - Re-validates each file against its fingerprint before acting.
  - Journals each op before and after (`begin` and `done`).
  - Trash support. Permanent delete only behind `--permanent` plus a typed confirmation phrase.
  - Free-space checks.
  - Resume with `--resume`.
- `yojaka apply` and `yojaka history`, plus the **History** screen.
- Write mode: `--write`, or a toggle in the TUI with a confirmation. The badge turns amber.

**Acceptance:**
- Integration test: scan → dupes → apply → verify. (Undo was dropped 2026-11-12, ADR-0009.)
- Fault-injection test: SIGKILL during apply, then `--resume` completes with nothing lost.
- A file modified between planning and applying is skipped and reported.
- If one half of a Live Photo fails validation, both halves are skipped.
- Manual test on a real folder, with the user's own sign-off.

---

## Iteration 6: Organize, move or copy (L)
**Goal:** product spec S3.
**Covers:** FR-40..43, FR-45, FR-46, FR-49, FR-55.

- Template parser and renderer, sanitization, date fallbacks, and `_undated`.
- Location placeholders, the precision guard (`Near <city>`, segments dropped when empty), and `No Location`.
- Opt-in location inference (same camera, within ±2 h), marked `≈`.
- Template presets: Date, Place, Place+Date, Kind+Date.
- Planner:
  - Resolves conflicts case-insensitively and with NFC normalization.
  - Skips files when the destination already has the same file.
  - Flags cross-volume moves.
- Executor additions:
  - Copy, using `clonefile` on the same volume.
  - Cross-volume move: copy, then verify, then remove the source.
  - `RENAME_EXCL` renames so nothing is ever overwritten.
- **Organize** screen:
  - Template input with live sample preview (showing both photo and video examples, and both geotagged and untagged files).
  - Location coverage, and an infer-location toggle.
  - Copy/move toggle.
  - Proposed tree glimpse, then Review, then Execute.
- `yojaka organize ... --plan-out`.

**Acceptance:**
- Property tests confirm no overwrite, that every source file ends up in exactly one place, and that Live Photo halves always land in the same folder.
- A cross-volume test using an `hdiutil` disk image passes.
- The glimpse for 100k files fits in 40 lines and can be expanded in the TUI.
- A `{country}/{city}/{year}` plan on the fixture puts every geotagged file in the expected folder and every file without location in `No Location/`.
- Inferred files are marked as inferred, and inference never overrides real GPS.

---

## Iteration 7: More duplicate tiers (M)
**Covers:** FR-30 (likely, name and similar tiers).

- **Likely tier:**
  - Photos: SQL grouping on EXIF date-taken, camera and dimensions.
  - Videos: creation date, resolution, and duration within ±1 s.
- **Name tier:** normalization rules and the ±5% size check.
- **Similar tier (photos only):** perceptual hash, a BK-tree, and a configurable threshold. Similar-video detection (keyframe hashing) is moved to after v1.0. This is the only stage that decodes pixels, so it is opt-in (`--similar`).
- UI:
  - Tier filter and badges.
  - Side-by-side metadata comparison.
  - Optional inline thumbnail for terminals that support images (`ratatui-image`), with a text fallback.
- Non-exact tiers **never** allow bulk-apply without per-group review.

**Acceptance:**
- Precision and recall are measured on the fixture's re-encoded and resized variants and recorded in `docs/`.
- No false positives in the exact tier (regression test).

**Status (2026-11-11): done** (ADR-0008, `docs/dupes-accuracy.md`, `docs/demos/iter-7.md`), committed as `99db741` and signed off by the user, like Iterations 5 and 6. Not built: the optional inline thumbnail (a text comparison is shown). The similar tier also compares HEIC, AVIF and raw (added 2026-11-10 through macOS ImageIO).

---

## Iteration 8: Hardening (M)
> Detailed plan: `06-i8-hardening-release-plan.md` (Phases 0-3).

- Check every functional requirement against the code and its tests (`docs/fr-checklist.md`) and close the gaps: filtered export, two more keep rules, saving plans and exports from the app, the `history` time zone.
- Saved preferences (FR-62): remembered templates, copy / move and keep rules, only on request.
- Safety fault-injection suite: real `kill -9` during `apply`, read-only destination, disk full, an unwritable journal, a file swapped for a symlink, Live Photo pairs at every crash point.

**Acceptance:** all functional requirements FR-1..62 are checked off, and the safety fault-injection suite passes.

**Status (2026-11-12): done** (Phases 0-3, 2026-11-11). The fault tests found one bug (undo before resume could split a Live Photo), fixed the same day; undo itself was removed on 2026-11-12 (ADR-0009). Commits `b93a80b`, `0104cbe`, `eb16c83`, plus the Phase 3 commit.

---

## Iteration 9: Features and release v1.0 (L)
> Detailed plan: `09-i9-features-release-plan.md`. Scan speed: `07-scan-speed-plan.md`. Homebrew tap: `08-homebrew-tap-plan.md`.

- **Detailed dashboard** (the user describes it; mockup first).
- Test on real libraries: an iPhone export with Live Photos and 4K videos, an external exFAT or HFS+ drive, an iCloud Drive folder, more than 1M files, and paths with odd Unicode.
- Release engineering: version 1.0.0, minimum macOS 14, a release workflow and GitHub Release (ad-hoc signed; no Developer ID yet), a Homebrew tap, and a user README with GIF demos.
- Performance, **last**: scan speed (plan 07), then every target in tech spec §10, published in `docs/perf.md`. Profile with samply and Instruments, then tune the thread counts, batch sizes and read caps.

**Acceptance:** the dashboard is signed off; real-world runs are recorded; every performance target is met or explained; the FR checklist and the fault-injection suite still hold; `v1.0.0-rc.1` installs with brew on a clean account; the user signs off and `v1.0.0` is tagged.

---

## Iteration 10: LLM assist (L), after v1.0
- `yojaka-llm` crate with the `LlmProvider` trait, an **Ollama** provider first (installed locally), then Claude and OpenAI (opt-in), then Apple Foundation Models through a Swift helper.
- Features, in order:
  1. Folder and event naming suggestions in Organize.
  2. Natural-language filter to a validated SQL `WHERE` clause.
  3. Best-shot pick for near-duplicates.
  4. Tags and captions.
- Privacy gate: local by default; `allow_cloud` plus a confirmation in each session before anything goes to a cloud provider.

## Iteration 11: GUI (later)
- Decide between Tauri and SwiftUI (tech spec §14), then build on `yojaka-core` unchanged.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Splitting a Live Photo pair (photo kept, video deleted, or the other way round) | Pairs treated as one item everywhere; property-test invariant |
| Slow video metadata (`moov` at end of file) | Seek-based box walker; a counting-reader test keeps bytes read under 1 MB |
| Wrong or odd place names (sea, borders, remote areas) | Precision guard (`Near X`, drop city beyond 100 km); geo test set; inferred locations are always marked `≈` |
| Data loss bug | Read-only until I5; trash by default; journal and a no-undo warning; fingerprint re-validation; property and fault-injection tests; never overwrite |
| HEIC/RAW metadata coverage gaps in pure-Rust crates | `--exiftool-fallback`; track coverage per format in tests |
| iCloud dataless files triggering mass downloads | `SF_DATALESS` check before any open; covered by tests |
| Corrupting a Photos library | `.photoslibrary` skipped by default; shown as a warning if the user includes it |
| Full Disk Access (TCC) errors confusing users | Detect `EPERM` and show a one-time guidance message |
| SQLite write contention slowing scans | Single writer thread, batched transactions, WAL |
| Terminal left broken after a crash | Panic hook plus a `Drop` guard that restores the terminal |
| Scope creep (LLM/GUI) | Kept behind v1.0; core API stays UI-free |

## Next action
Answer the open questions in `01-product-spec.md` §6 (defaults are fine to start), then begin **Iteration 0**.
