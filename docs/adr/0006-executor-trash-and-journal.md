# ADR-0006: Executor, Trash and journal

Status: accepted (Iteration 5)

## Context
Iteration 5 is the first release that changes the user's files. The tech spec said to use the `trash`
crate and record the Trash location "where possible". On macOS that crate cannot report where a file
went and has no restore, so undo would have had to guess a name in `~/.Trash`.

## Decision
- **Trash** goes through `NSFileManager.trashItemAtURL:resultingItemURL:error:` (the `objc2-foundation`
  crate, safe API, no `unsafe` in Yojaka). It returns the exact path, which the journal stores. The
  `Trasher` trait lets tests use a folder (`DirTrash`) so they never fill the real Trash. The real call
  is covered by an ignored test (`real_trash_round_trip`).
- **Restore and "never overwrite"** use `hard_link` then `remove_file`. `hard_link` fails if the target
  exists, so nothing is replaced and no `unsafe` (`renamex_np`) is needed. It works within one volume,
  which is where the Trash and the file are.
- **Journal** is `journal/<plan-id>.jsonl` (header line, then `begin` / `done` / `failed` / `skipped`
  per op, tagged `apply` or `undo`) plus `<plan-id>.plan.json`, a copy of the plan, so undo and resume
  do not depend on the user keeping the plan file. Every line is flushed (`sync_data`) before the file
  is touched. A last line cut by a crash is ignored when reading.
- **Validation** happens just before each op: still a regular file, not iCloud-only, same size and
  mtime, same full BLAKE3 hash, still inside the plan folder after resolving symlinks, not protected,
  and the copy to keep still exists and matches. Any failure skips the op and reports it.
- **Live Photos** are executed as a unit. Both halves are validated first; one bad half skips both. If
  the second move fails, the first is put back. `DupRef.kept` is now the matching half of the kept pair
  (it was the pair's primary file, which cannot be checked against a video).
- **Resume** skips ops with a `done` entry. An op with `begin` and no `done` whose file is gone is
  recovered by looking for the file in the Trash (name, size, mtime); if that fails the journal says
  undo cannot restore it automatically.
- **Plan checks before anything runs** (`check_plan`): dedupe plans only, paths under the plan root with
  no `..`, nothing protected, no file both removed and kept, `Delete` ops only with `--permanent`.
- **Permanent delete** exists only in the CLI (`apply --permanent`), always asks for a typed phrase
  (`--yes` does not skip it), and is not offered in the TUI. It cannot be undone and undo says so.
- **Typed confirmation** for 50 or more files (`Plan::LARGE_PLAN_FILES`) or any permanent delete.
- **Write mode** is off by default. CLI: `--write` (without it `apply` and `undo` only preview). TUI: `W`
  asks first. Execution is single-threaded: the cost is the disk, and ordering makes the journal simple.

## Consequences
- Undo is exact and repeatable. Running it twice restores once.
- Reading `~/.Trash` for crash recovery may need Full Disk Access; without it that one recovery step
  reports "location unknown" instead of failing the run.
- Free space: the journal volume needs 64 MiB before a run starts (Trash moves on one volume use none).
  Copy and cross-volume move (Iteration 6) will need a real per-destination check.
- The SIGKILL acceptance test is simulated in-process (a trasher that panics after moving a file but
  before the journal entry). A real-process kill test has not been written.
