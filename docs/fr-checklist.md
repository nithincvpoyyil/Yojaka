# Functional requirements checklist (Iteration 8, Phase 1)

Written 2026-11-11. One row per requirement in `all-specs/01-product-spec.md` §4. Proof for the I8 acceptance
line "every functional requirement FR-1..62 is checked off".

**State:** `done` = built and covered by a test or a check named in the row. `changed` = the spec text and the
code differ by a recorded decision. `gap` = not built; the row says what happens next.

**How to read the evidence:** rows marked **†** were built and tested in their own iteration and signed off then;
this audit confirmed the code exists but did not re-run the test one by one. The full test suite (377 tests)
passed at the end of Phase 1. All other rows were checked against the code in this audit.

| FR | Requirement (short) | State | Where | Test / check |
|---|---|---|---|---|
| 1 | `yojaka [PATH]` opens the TUI, default current dir, volumes allowed | done † | `cli.rs`, `main.rs` | CLI + TUI tests |
| 2 | Read-only by default; write mode needs `--write` or `W` plus a confirmation | done † | `app.rs` (`Mode`, `EnableWrite`), `cmd/exec.rs` | `write_mode_is_asked_for_and_can_be_turned_off`, `nothing_can_be_reviewed_while_read_only` |
| 3 | Every TUI feature has a CLI command | done | `cli.rs`: scan, stats, info, places, export, dupes, organize, plan show, apply, undo, history, doctor | `crates/yojaka-cli/tests/cli.rs` |
| 10 | Parallel walk, detect by extension and magic bytes, kind per file | done † | `scan/mod.rs`, `detect.rs` (`SNIFF_LEN`) | `tests/scan.rs`, detect unit tests |
| 11 | Image, RAW and video formats; `--kind` | done † | `model.rs`, `detect.rs`, `meta/*` | `tests/metadata.rs`, `tests/dupe_tiers.rs` (raw) |
| 12 | Persistent SQLite index | done † | `index/`, `migrations/` | `tests/scan.rs` |
| 13 | Incremental rescan | done † | `scan/mod.rs` (`Touch`) | `rescan_is_incremental_and_prunes`; timings in `docs/perf.md` |
| 14 | Safe walking: no symlinks, skip list, dataless never opened, hard links one file, permission errors reported | done † | `scan/mod.rs`, `sys/macos.rs`, `dupes/exact.rs` | `tests/scan.rs`. The default skip list has `Library` (the spec says `Library/`); configurable in `config.toml` |
| 14a | Protected macOS locations, no override, plans re-checked | done † | `protect.rs`, `exec/` | `protect.rs` unit tests (system folders, symlinks into them, `~/Library`, bundles, temp dirs); `bad_plans_are_refused_before_anything_runs` |
| 15 | Live progress and cancel, partial results kept | done † | `scan/mod.rs`, TUI scan gauge | `a_folder_bigger_than_any_buffer_scans_to_completion`, TUI scan tests |
| 20 | Per file: size, format, EXIF date / camera / lens / orientation / GPS flag, GIF and PNG frame count (colour type, bit depth and PNG text chunks dropped 2026-11-11) | changed | `meta/image.rs` | `tests/metadata.rs`. Spec FR-20 edited |
| 20a | Per video: duration, resolution, rotation, fps, codecs, container, date, device, GPS | done † | `meta/isobmff.rs`, `meta/matroska.rs` | `tests/metadata.rs`; counting-reader test (under 1 MB read) |
| 20b | Live Photo pairs are one item everywhere | done † | `livephoto.rs`, `plan/`, `exec/` | `a_live_photo_pair_is_trashed_and_restored_together`, `live_photo_halves_are_moved_together_or_not_at_all` |
| 21 | Statistics: photos and videos, formats, year and month, cameras, places, largest files, undated, longest and largest videos | done | `stats.rs` (`by_year` with `months`, `undated`, `largest_files`, `video.longest`, `video.largest`), `cmd/stats.rs`, dashboard | `tests/metadata.rs` |
| 22 | Export index or a filtered view to CSV or JSON | done (Phase 1) | `export.rs` (`ExportOptions.query`, `.year`), `cmd/export.rs` (`--kind --year --find --folder`), TUI `E` | `export_can_be_limited_by_kind_year_and_text`, `export_can_be_limited_by_kind_text_and_folder`, `e_exports_the_files_the_browser_shows`. The TUI `E` writes a CSV into Yojaka's data folder (`exports/`), not into the photo folders |
| 30 | Duplicate tiers exact / likely / name / similar | done † | `dupes/` | `tests/dupes.rs`, `tests/dupe_tiers.rs`, `docs/dupes-accuracy.md` |
| 31 | Keep rules, chainable: oldest date taken, oldest mtime, shortest path, preferred folder, highest resolution, longest duration / bitrate, **largest file, has EXIF** | done (Phase 1) | `dupes/keep.rs` | `largest_file_and_has_exif_pick_the_right_copy`, `rules_chain_in_order` |
| 32 | Review groups, mark keep / delete, bulk rule, space reclaimed | done † | `tui/dupes.rs`, `screens/duplicates.rs` | TUI duplicates tests |
| 33 | Delete = Trash; permanent needs `--permanent` and a typed phrase | done † | `exec/trash.rs`, `cmd/exec.rs` | `permanent_delete_removes_the_file_and_cannot_be_undone`, `real_trash_round_trip` (ignored; touches the real Trash) |
| 40 | Destination and path template; date from EXIF, container or mtime; `_undated` | done † | `organize/template.rs`, `planner.rs` | `tests/organize.rs`, template proptest |
| 41 | Copy or move (index-only dropped 2026-11-11, decision D3) | changed | spec FR-41 edited | `copy_leaves_the_originals...`, `move_then_undo...` |
| 42 | Never overwrite; `name (1).jpg`; exact duplicate at the destination skipped and reported | done † | `organize/planner.rs`, `exec/transfer.rs` | `tests/organize.rs`, `a_file_changed_after_planning_is_skipped_and_never_overwrites_anything` |
| 43 | Proposed tree before any change, glimpse with collapsed nodes | done † | `plan/tree.rs`, Organize screen | `plan/tree.rs` tests, TUI organize tests |
| 44 | Offline GPS to country / state / district / city / distance | done † | `geo/` (embedded GeoNames) | `geo/tests.rs` |
| 45 | Place placeholders, `Near X` past 25 km, country only past 100 km, `No Location` folder | done † | `geo/mod.rs` (`guarded`), `organize/template.rs` | `geo/tests.rs`. The folder for files without a place is `No Location` (config `no_location_dir`); spec edited to match 2026-11-11 |
| 46 | `--infer-location`: same camera, within ±2 h, marked `≈` | done † | `organize/planner.rs` | `tests/organize.rs`, TUI `≈` assertion |
| 47 | A Live Photo pair uses one place | done † | `organize/planner.rs` | `tests/organize.rs` |
| 48 | Privacy: no coordinates leave the machine; GPS out of exports unless asked | done | `export.rs`, `geo/` | `export_leaves_out_gps_unless_asked` |
| 49 | Date and place can be combined in any order | done † | `organize/template.rs` | template tests |
| 50 | Plans are built first and can be saved (`--plan-out`) and applied later | done (Phase 1) | `plan/mod.rs`, `cmd/dupes.rs`, `cmd/organize.rs`, TUI `P` | CLI pipeline tests, `a_plan_can_be_saved_in_read_only_mode_without_a_review`. TUI `P` saves to `<data>/plans/` (works in read-only mode) |
| 51 | Dry-run: totals, before / after, warnings | changed | `plan/tree.rs`, Review box, `plan show` | Totals, bytes, warnings (cross-volume, look-alike removals, changed files) and one folder tree with counts: removals for dedupe, destination for organize. One tree, not before and after side by side; spec FR-51 edited to say so, 2026-11-11 |
| 52 | Large plans need a typed phrase | changed (D2) | `plan/mod.rs` (`LARGE_PLAN_FILES` 50, `LARGE_COPY_FILES` 1,000) | `a_large_plan_needs_its_phrase_typed`. **Code is right per ADR-0006/0007; the spec text (1,000 for all) and the unused config key `organize.large_plan_threshold` are for the user to tidy later** |
| 53 | Re-validate before each operation | done † | `exec/` | `a_file_changed_after_planning_is_skipped_and_reported`, `an_edit_that_keeps_size_and_mtime_is_still_caught_by_the_hash` |
| 54 | Journal before and after each op; `undo` | done † | `exec/journal.rs`, `exec/undo.rs` | `scan_dupes_apply_undo_gives_back_the_identical_tree`, `undo_never_overwrites...` |
| 55 | Cross-volume: copy, verify hash, then remove source | done † | `exec/transfer.rs` | `cross_volume_move_copies_verifies_removes_and_undo_restores` (needs `hdiutil`, ignored; gets a CI job in Phase 3) |
| 56 | Free-space check before running | done for Organize | `exec/organize_run.rs:125` | Organize copy and move. Trash plans free space, so no check. **No test refuses on a full disk yet: Phase 3 item 4** |
| 60 | Always responsive, heavy work off the UI thread | done † | `tui/worker.rs`, `event_loop.rs` | `rendering_a_full_browser_frame_is_far_below_16_ms`; numbers in `docs/perf.md` |
| 61 | Status bar, `?` help overlay, vim and arrow keys | done † | `keymap.rs` (`HELP` is the one list) | `vim_and_arrow_keys_agree`, help tests |
| 62 | Rich live feedback; remembered choices in `config.toml` | done (Phase 2) | Feedback †. Remembering: `config.rs` (`Config::remember`, `organize.mode`), `dupes/keep.rs` (`configured_rules`), `organize/mod.rs` (`configured_template`, `configured_transfer`), CLI `--remember`, TUI `M` | `remembered_choices_become_the_defaults_and_bad_ones_are_ignored`, `saved_choices::*` (6 TUI tests), `config::tests::remember_*`, `a_bad_saved_chain_falls_back_with_a_warning`. Saves keep rules, folder pattern and copy / move, only on request. Skip folders stay a hand edit of `config.toml` (`scan.skip_dirs`): no screen edits them |

## Also fixed in this phase

- `yojaka history` showed UTC and the History screen showed local time for the same run. Both now use one
  formatter, `yojaka_core::meta::format_run_time` (test `run_time_uses_the_given_offset`).

## Open for the user

1. ~~FR-20: colour type, bit depth, PNG text chunks.~~ **Dropped from the spec on 2026-11-11** (user decision).
2. ~~FR-51~~ Spec reworded to the single folder tree (2026-11-11).
3. FR-52: the spec text and the unused config key (decision D2: yours, later).
4. ~~FR-45~~ `No Location` chosen; spec, tech spec and iteration plan edited (2026-11-11).
