# Yojaka: project status and hand-off

Last updated: 2026-11-11. Read this first when resuming. The plan is `all-specs/03-iteration-plan.md`;
rules are in `CLAUDE.md`.

**Scan walker (after I4), done 2026-11-09** (`all-specs/05-scan-walker-fix-plan.md`, ADR-0005): `dua-core`
replaces jwalk, so folders over 50k entries no longer freeze the scan and memory is flat (1M entries: 51.9 s,
25 MB; rescan 6.5 s). A root named like a skip-list entry is now scanned. Progress has a "Finishing up" state
with no false "No progress" warning or stale ETA, and live statistics back off when slow. 251 tests pass.
Committed. Iterations 5, 6 and 7 followed (below).

## Where we are

| Iteration | What | State |
|---|---|---|
| 0 | Workspace, config, logging, protected paths | Done |
| 1 | `scan`, per-folder saved data | Done |
| 2 | Metadata, Live Photos, offline places, `info` / `places` / `export` | Done |
| 3 | TUI: dashboard, file browser, help, scan control | Done |
| 4 | Exact duplicates (hashing, `dupes`, Duplicates screen) | Done (`1c698fc`) |
| 5 | Safe executor, History | Done (`79ea0b6`), signed off by the user 2026-11-11 (ADR-0006) |
| 6 | Organize (templates, planner, copy/move, undo, screen, CLI) | Done (`bb1e12e`), signed off 2026-11-11 (ADR-0007) |
| 7 | More duplicate tiers (likely, name, similar) | Done (`99db741`), signed off 2026-11-11 (ADR-0008) |
| 8 | Hardening: FR audit, saved preferences, fault injection (`06`) | Done 2026-11-11 (`b93a80b`, `0104cbe`, `eb16c83`, Phase 3 commit) |
| 9 | Features and release v1.0: detailed dashboard, real libraries, release, performance last (`09`) | **Next** (dashboard: waiting for the user's description) |
| 10 | LLM assist | Not started |
| 11 | GUI | Not started |

The old Iteration 8 was split on 2026-11-12: hardening stayed in 8, the rest became 9; LLM and GUI moved to 10 and 11.

Everything up to `99db741` (Iteration 7) is on `origin/main`. No git tags exist, and none are planned before `v1.0.0-rc.1` (D1).
Checks at the end of the Iteration 7 session: see "Duplicate tiers" below.

Full check: `source ~/.cargo/env && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check`.
New insta snapshots: run with `INSTA_UPDATE=always`, then read the `.snap`. The TUI snapshots under
`crates/yojaka-tui/src/snapshots/` are gitignored; the CLI help snapshot is tracked.

## What the app does today

Open a folder: `yojaka [PATH]` (default: current dir). Read-only. Saved data is per folder and kept
automatically in `~/Library` (no user choice, no jargon).

| Flag | Effect |
|---|---|
| (none) | Never-scanned folder: scans automatically. Already scanned: shown as it is. |
| `--refresh` | Scan on open if the last scan is over 1 hour old. Incremental, deletes nothing. |
| `--force` | Asks y/n at start-up; on `y` deletes the folder's saved data and scans from scratch (slower). `n` carries on as usual. |
| `--about` | MIT license and the GeoNames CC BY 4.0 credit. Creates nothing on disk. |
| `--local-index` | Hidden. For tests and tooling only: keeps the data inside the folder. |

CLI commands: `scan` (`--force` wipes without asking), `stats`, `info`, `places`, `export` (`--kind image|video`, `--year`, `--find TEXT`, `--folder DIR`), `doctor`.
`dupes` (all tiers; see below), `plan show`, `apply`, `undo`, `history` (Iteration 5; see below). `organize` (Iteration 6; see below).

### TUI keys (the `?` overlay is generated from `keymap.rs`)

- Screens: `1`-`5`, `<` `>`, `←` `→`, Shift+`←` `→`. In the file browser's folder pane `←` `→` collapse /
  expand, so use `<` `>` or Shift+arrows there.
- Tab mode: `T` anywhere, or Esc in the File browser when nothing else needs closing. `←` `→` then pick
  the tab; Enter / Esc / ↑ / ↓ go back. During a scan Esc stops the scan first, so use `T`.
- File browser: `Tab` pane, `j k` / arrows move, `PgUp PgDn g G`, `/` find (name, camera, place),
  `t` all / photos / videos, `s` sort column, `S` reverse. **Enter on a file opens it with the OS default
  app** (`/usr/bin/open`); iCloud-only files are refused; a vanished file gives a footer message.
- Scan: `r` rescan (incremental), `R` scan from scratch (asks y/n), `c` or Esc cancel (keeps partial results).
- `M` on Duplicates or Organize (and `--remember` on `yojaka dupes` / `yojaka organize`) saves the keep rule, or the folder pattern and copy / move, in `config.toml` (`dupes.keep_rules`, `organize.template`, `organize.mode`). Only on request. Bad saved values are ignored with a warning. A saved keep chain that is not one of the four presets becomes a fifth step of `p`.
- File browser `E` exports the files shown to a new CSV in `<data>/exports/`; Duplicates and Organize `P` save the plan to `<data>/plans/` (both work in read-only mode: they write only Yojaka's own folder).
- Other: `?` help, `D` debug overlay (frame / fetch times), `q` quit, Ctrl-C stops the scan, then quits.

### Duplicates: exact tier (Iteration 4)

Finding copies never deletes anything: results are marks and a saved plan file. Running a plan is the executor below.

- Core: `yojaka_core::dupes` (`find_exact`, `DupGroup`, keep rules), `hash.rs` (BLAKE3 partial / full),
  `plan/mod.rs` (`Plan::from_dupes`, JSON save / load, `Op::Trash`). `Engine::dupes(root, local, sink, cancel)`.
- Staged: size groups (SQL) -> hard links collapsed -> partial hash (size + first/last 16 KiB) -> full hash.
  Only files sharing a size are opened. Hashes are cached in `file.partial_hash` / `file.full_hash` (a rescan clears
  them when size or mtime changes). Candidates are `stat`ed first; a file changed since the scan is reported
  (`IssueKind::Changed`) and left out. Cloud-only files and empty files are never considered.
- A Live Photo pair is one item. A pair only matches another pair with the same image **and** video; a pair never
  matches a lone image. Toggling, rules and the plan always act on both halves.
- `yojaka dupes PATH [--keep RULES] [--plan-out FILE] [--json]` (tiers: next section). `--keep` chains rules, comma separated: `oldest-taken`, `oldest-modified`, `newest-modified`, `shortest-path`,
  `prefer-path:DIR`, `highest-resolution`, `longest-duration`, `highest-bitrate`, `largest-file`, `has-exif`. Default
  `oldest-modified,shortest-path`; the last tie-break is always the path bytes, then the file id. `--plan-out` never
  overwrites a file (exit 2).
- TUI screen 3: `f` find (`F` with pictures, `v` tier filter: next section), `j k` move, `Tab` groups / copies, `Space` mark, `Enter` keep only this copy (on a group:
  open it), `p` next keep rule (applied to every group), `x` keep every copy in the group, `c` / Esc stop. The last
  unmarked copy of a group cannot be marked. A finished scan clears the results. A scan and a search cannot overlap.
  The search does not start by itself: it reads files, so it waits for `f`.
- Help (`?`) now shows only the global keys plus the current screen's keys (the full list no longer fits 24 rows).
- Measured: 20k-file fixture, 0.7 s, 58 MB read; the second run reads 0 B.

### Duplicate tiers: likely, name, similar (Iteration 7, ADR-0008)

Guesses, not identical files. **No rule marks them and the CLI never plans them**; in the app each group is
marked by hand. Measured precision and recall: `docs/dupes-accuracy.md`.

- `yojaka dupes PATH --tier exact|likely|name|similar|all [--similar] [--similar-distance BITS]`. Default `exact`.
  `all` is exact + likely + name; `--similar` adds the picture comparison (opt-in: it decodes every picture once, then
  remembers; every image format Yojaka knows, raw included). Non-exact groups print `review`; JSON has `tier` and `needs_review`.
- **likely**: photos with the same EXIF date, camera model, width and height; videos with the same date, width and
  height and lengths within 1 s. **name**: same kind and normalized name (NFC, lower case, no extension, no
  ` (n)` / ` copy` / `-n` / `_n` suffix), sizes within 5%. **similar**: 64-bit difference hash within
  `dupes.similar_distance` bits (default 6), same colour signature, same shape. Pictures are turned upright first
  (EXIF orientation / HEIC rotation), so an iPhone portrait HEIC matches its upright JPEG copy.
- HEIC, AVIF and raw (DNG, CR2, CR3, NEF, ARW, RAF, ORF) are decoded in process by macOS ImageIO
  (`sys/imageio.rs`, the one module allowed `unsafe`; crates `objc2-image-io`, `objc2-core-graphics`), opened by
  path and always told the file type (without it ImageIO misreads ARW and NEF as TIFF). The decoder comes from the
  indexed format (`similar::decoder`). Not `sips` (not deprecated, but a process and a temp file per picture) and
  not `libheif` (LGPL C library). Reasons and numbers: ADR-0008 "HEIC, AVIF and raw". A 12 MP HEIC takes 36 ms.
- A raw photo is compared through the camera's preview inside it (12 to 29 ms, a few MB read), so it matches the
  JPEG the camera saved with it; Apple's development of the raw is the fallback (slower, other colours). Measured
  on 7 real CC0 raw files, one per format: all 7 RAW+JPEG pairs grouped (`docs/dupes-accuracy.md`). The saved size
  of a raw is not used in the shape check (metadata bug, see open items).
- The exact tier always runs first; files identical to each other count once in the other tiers. A group that a
  stronger tier already shows is not repeated (order: exact, likely, similar, name). Candidates are `stat`ed; a
  changed file is reported and left out. A Live Photo is one item, listed under its photo.
- Core: `yojaka_core::dupes` (`find`, `DupOptions`, `tiers.rs`, `name.rs`, `similar.rs`); `Engine::dupes(root, local,
  opts, sink, cancel)`. `DupGroup::apply_rules` does nothing to a non-exact group.
- Plans: `DupRef.tier` and `DupRef.kept_expect`. For a non-exact removal the executor checks the kept copy against
  its own size and date (it is a different file). `Plan::from_dupes` removes a file at most once and never removes
  a file another group keeps. `Plan::lookalike_ops()` counts such removals; the Review box and `plan show` say so.
- TUI screen 3: `f` searches exact + likely + name, `F` also pictures, `v` cycles the tier filter. Groups have a tier
  badge; a non-exact group shows `? review` and a comparison table (size, pixels, taken, length, camera). `p` changes
  the rule for exact groups only.
- Index: new column `file.color_signature` (in `0002_scan_file.sql`, no new migration; bit 60 = the upright
  picture is portrait). **Saved data from before 2026-11-10 needs `--force` before the similar tier works**; the
  other tiers work on old data. `meta_kv.picture_look_version` (now `2`): when the code's `LOOK_VERSION` differs,
  every saved picture hash is dropped once and made again (no `--force` needed).
- `fixturegen --variants N` writes the look-alike sets and decoys; the manifest has `variant` and
  `expected.same_content`.
- Tests: `crates/yojaka-core/tests/dupe_tiers.rs` (16, one opt-in raw test), unit tests in `dupes/name.rs`,
  `similar.rs`, `tiers.rs`, `sys/imageio.rs`, CLI `dupes_lists_look_alikes...`, TUI `look_alike_*`,
  `reviewing_a_plan_with_look_alikes_says_so`. End of session (2026-11-11, after HEIC / AVIF / raw and raw
  metadata): 371 tests passed (4 skipped); `cargo fmt`, clippy `-D warnings` and `cargo deny` clean. Release binary 9.2 MB.
- Driven by hand in the real app (pty): find, filter, mark a likely group, review, apply to a scratch Trash, undo.
- Not done: inline thumbnails (optional in the plan); similar videos; a measurement on real photos (real iPhone HEIC
  especially); the BK-tree at 1M pictures. Opt-in raw test: `YOJAKA_RAW_SAMPLES=<dir of X.NEF + X.JPG>
  cargo nextest run -p yojaka-core --run-ignored only -E 'test(raw_photos)'`.
- Also fixed: `organize_exec::an_interrupted_move_resumes...` could wait forever (a race in the test, not in the
  executor).

### Safe executor (Iteration 5, ADR-0006)

The first write-capable release. Off by default: no `--write` means a preview only.

- `yojaka dupes PATH --plan-out plan.json` -> `yojaka plan show plan.json [--full-tree]` -> `yojaka apply plan.json`
  (dry run) -> `yojaka --write apply plan.json` -> `yojaka history` -> `yojaka --write undo --last`
  (or an id, or the start of one). `--write` is a global flag, so it goes before the subcommand or anywhere.
- `apply` re-checks every file (size, mtime, BLAKE3) and the copy that stays, then moves it to the Trash and
  records the exact Trash path in the journal. Changed files are skipped and listed. A Live Photo pair is
  skipped or moved as one. 50 or more files need `trash N files` typed; fewer need `y` (`--yes` skips that).
- `apply --resume` finishes an interrupted plan. Applying a plan twice is refused.
- `apply --permanent --write` deletes for good (typed phrase `delete N files forever`, even with `--yes`);
  `undo` reports those as not restorable. Not available in the TUI.
- Journal and plan copy: `<data>/journal/<plan-id>.jsonl` and `.plan.json`.
- TUI: `W` turns write mode on (asks) or off; on Duplicates `a` opens the Review box (summary, folders
  losing files, typed phrase for large plans); a run shows progress (Ctrl-C stops after the current file),
  then a report, then rescans. Screen 5 History lists runs; `u` puts the selected run back.
- Hidden `--trash-dir DIR` makes tests use a folder as the Trash.
- Core: `yojaka_core::exec` (`apply`, `undo`, `history`, `journal`, `trash`), `plan::tree` (`DirTree`
  glimpse, `Plan::removal_tree`). Tests: `crates/yojaka-core/tests/exec.rs` (12 + 1 ignored real-Trash test),
  CLI pipeline tests in `crates/yojaka-cli/tests/cli.rs`, TUI tests in `snapshots_tests.rs::write_path`.
- Not done: a real-process SIGKILL test (simulated in-process instead), copy / move ops (Iteration 6), a
  before/after tree for non-dedupe plans (only "files removed, by folder" exists), parallel execution.

### Organize (Iteration 6, ADR-0007)

- `yojaka organize PATH DEST [--template T | --preset date|place|place-date|kind-date] [--move] [--infer-location]
  [--plan-out FILE] [--full-tree]` plans only (copy is the default). Run the plan with `yojaka --write apply FILE`;
  `undo` reverses it. Placeholders: `year month day camera make model format ext name stem parent kind country
  country_code state district city`; `{month:02}` pads. See the ADR for the folder-dropping rules.
- TUI screen 4: `e` pattern, `d` folder, `p` next preset, `m` copy/move, `i` infer places, Enter plans (changes
  nothing), `a` reviews and runs (needs `W`). Examples and a folder overview come from the plan.
- Core: `yojaka_core::organize` (`template`, `planner`), `exec::transfer`, `exec::organize_run`.
- Tests: `tests/organize.rs` (planner), `tests/organize_exec.rs` (apply, resume, undo, pairs, refusals; the
  cross-volume test is ignored), template proptest, TUI `organize_screen`, CLI `organize_plans_then_apply...`.
- Not done: live re-planning while typing; a place picker. (`index-only` was dropped from FR-41 on 2026-11-11.)

### Look

- Default theme is Cobalt2 (terminalcolors.com/themes/cobalt2/default): background `#122738`, yellow
  titles / keys / focused borders, blue borders, `#0050a4` selection bar. **All colors live in
  `crates/yojaka-tui/src/theme.rs`**; other themes are planned for the last iteration.
- Terminals without 24-bit color (Terminal.app) get the nearest of the 256 colors.
- Header: one line (`Yojaka · [READ-ONLY]` left; status or the scan loader right; no folder path), then a
  3-row folder-style tab strip, then the screen, then a hint line (keys yellow, actions muted).
- The scan loader (spinner, status, progress bar) shows in the header on every screen.
- Dashboard: scan panel, library totals, Formats, By year, Top cameras, Top places (cities with counts and a
  "N countries · N without place" line). A more detailed dashboard is planned by the user.

## Decisions (made with the user)

- MIT license. Binary-size goal under 100 MB (release binary was 7.4 MB after I3).
- Before v1.0 edit original migrations; do not stack fix-up migrations (a changed schema means rebuilding
  the saved data with `--force`).
- `/private/var/folders` is exempt from the protected paths; `/private/tmp` stays protected, so perf
  fixtures go in the repo's `out/` dir.
- Own ISO-BMFF / EBML walkers and own k-d tree (ADR-0002, ADR-0003). TUI architecture: ADR-0004.
- `--refresh` must never delete; deletion needs `--force` or `R` plus a confirmation.
- Kind filter key is `t`, not the spec's `k` (k is vim "up"). Spec §7 updated.
- Browser screen is named "File browser".
- GeoNames data is public and CC BY 4.0; attribution is in `--about` and the README.

## Privacy check (2026-11-08)

Nothing private is on GitHub: no photos, screenshots, media, databases, keys or tokens in any commit; commits
use the GitHub no-reply email; `tests/data/` (the user's Sony JPEGs), `out/`, `*.db` and TUI snapshots are
gitignored. Re-check before the first public release.

## Open items

- **Fixed 2026-11-11 (found by the Phase 3 fault tests):** `undo` run after a crash but before `apply --resume`
  could split a Live Photo pair. `undo` now refuses a run with a half-done step and prints the resume
  command (ADR-0006 addendum).
- Similar tier: every image format is compared since 2026-11-10 (ImageIO for HEIC, AVIF, raw). Not measured on a
  real photo library yet. Colours are not converted to one colour space, so a Display P3 photo and an sRGB copy can miss in strongly
  coloured pictures.
- Raw metadata **fixed 2026-11-11** (ADR-0002 addendum): read through ImageIO (`meta/raw.rs`), EXIF reader as
  fallback. Before, Fuji RAF, Canon CR3 and Olympus ORF had no size, date, camera or place ("malformed metadata")
  and a Nikon NEF had its thumbnail's size. Old saved data is fixed by the next scan (`meta_kv.raw_meta_version`),
  no `--force`. Checked on the 7 samples and 210 files in parallel (0.34 s). The similar tier still ignores raw sizes.
- The similar tier decodes JPEG / PNG etc. at full size (a 30 MP JPEG is about 90 MB; peak 500 MB seen with 7 at
  once). ImageIO could shrink them while decoding, as for HEIC (would need a `LOOK_VERSION` bump).
- Still missing real samples: iPhone Live Photo pair, iPhone HEVC MOV, Android MP4, GPS-tagged photo. Those
  formats are tested with synthetic `fixturegen` files only. Raw: 7 CC0 files from raw.pixls.us were tried
  (listed in `docs/dupes-accuracy.md`; not kept in the repository).
- Scan CPU optimization deferred by the user (numbers in `docs/perf.md`).
- Open question: should a scan root that is itself a symlink be refused?
- Slow at 1M rows: statistics about 4 s (own thread), text filter with few matches about 1.5 s. No mouse support.
- A rescan with nothing changed shows the last scan's error count (files with bad metadata are retried only
  after `R` / `--force`).
- Detailed dashboard (user will specify), a country bar list, and selecting a place to filter the file browser.
- `--force` must come before a subcommand (`yojaka --force PATH`); `yojaka scan --force` is the older,
  non-interactive command and wipes without asking.
- At 80 columns the Top places summary line is cut with an ellipsis; the narrow-width tab strip needs 44
  columns minimum (full names need 77).
- Another theme is planned for the last iteration.

## Useful commands

- Test data: `cargo run -p fixturegen -- out/ --files 1000` (writes `out/tree`, `out/manifest.json`).
  A 100k tree takes about 35 s. Delete `out/` perf trees afterwards.
- Isolate config / data from the real `~/Library`: `YOJAKA_HOME=<dir>`.
- Drive the real TUI headlessly: a python `pty` + `pyte` script (no tmux on this Mac).
- 1M-row browse perf: `YOJAKA_PERF_ROOT=$PWD/out/million cargo nextest run -p yojaka-core --run-ignored only -E 'binary(browse_perf)' --no-capture`.
- SQLite `file_name` / `full_path` columns are BLOBs: use `CAST(file_name AS TEXT)`.
- `gen` is a reserved word in edition 2024; name fields `generation`.
