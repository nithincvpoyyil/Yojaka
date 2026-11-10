# ADR-0007: Organize planner and file transfers

Status: accepted (Iteration 6)

## Context
Organize turns a folder pattern into moves or copies. It must never overwrite a file, never split a
Live Photo pair, and be undoable. The tech spec asked for `clonefile` and `renamex_np(RENAME_EXCL)`;
`renamex_np` needs `unsafe`, which the workspace denies.

## Decision
- **Templates** (`organize::template`): parsed once into path segments. `{name}` or `{stem}` is required
  in the last part, so files cannot collide by construction. An empty placeholder drops its segment.
  Date placeholders without a date give one `_undated` folder; place placeholders for a file with no
  place give one "No Location" folder (name from `location.no_location_dir`); a partly known place
  (country but no city) just drops the missing parts. Values are made safe (`/ : \` become `_`, control
  characters, leading dots, NFC, 200 bytes) and a property test checks no rendered path can escape.
- **Planner** (`organize::planner`) reads the index and the destination's names only. A Live Photo pair
  is one unit: folders come from the photo's facts, and both halves get the same ` (n)` suffix. Name
  clashes are judged case-insensitively and in NFC (APFS rules) against both the plan and the disk. A
  destination that already holds the same bytes becomes `Skip(AlreadyThere)`; the same path as the
  source is skipped too. Dates fall back to the modification time (counted in `date_fallback`).
- **Place** uses the existing precision guard (`Place::guarded`): `Near <city>` past `near_km`, no city
  past `far_km`, no country past 300 km. State and district follow the country and city.
- **Inference** (opt-in): a file with a date and no place borrows the nearest geotagged file of the
  same camera within the window (default 2 h); a camera with no geotagged file at all may borrow from
  the same folder. Never over real GPS. Marked `inferred` on the op.
- **No-overwrite without `unsafe`**: every final name is created with `hard_link`, which fails if the
  name is taken. A same-volume move is `hard_link` then `remove_file` (what `rename` does, minus the
  overwrite). A copy goes to `.<name>.yojaka-<op>.tmp`, is cloned (APFS, via `reflink-copy`) or copied,
  synced, given the source's mtime, hash-checked against the source, then linked into place.
- **Cross-volume move** = copy, verify, check the source is unchanged, remove the source. On `EXDEV`
  from the link step the move falls back to this automatically.
- **Validation** before each op: source unchanged (size, mtime), inside the plan folder, destination
  absent, destination folder inside the destination, not protected. A pair is validated together; if the
  second half fails after the first moved, the first is put back.
- **Undo**: a move goes back (never over an existing file); a copy is removed only if it still matches
  the verified hash **and the original still exists** (the last copy is never removed); a folder is
  removed only if Yojaka created it and it is empty. Journal entries now carry the verified hash.
- **Free space**: copies and cross-volume moves need their bytes plus 64 MiB free at the destination.
- **Confirmation**: copy plans type a phrase at 1,000 files; move and trash plans at 50 (`file_ops()`
  counts only files, not folders or skips).

## Consequences
- Plans got fields: `dest`, `inferred`, `pair`, and summary counters (all `serde(default)`).
- `reflink-copy` and `unicode-normalization` are new dependencies.
- The cross-volume test mounts a disk image, so it is `#[ignore]` (`-E 'test(cross_volume)'`).
- Inference is tested on synthetic rows; the fixture has no same-camera, time-adjacent pairs.
- The TUI does not re-plan live while typing: Enter plans (it reads the whole index), and examples
  come from the plan, not from a separate sample query.
