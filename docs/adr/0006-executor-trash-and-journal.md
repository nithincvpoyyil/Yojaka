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
  before the journal entry). ~~A real-process kill test has not been written.~~ Real-process kill tests
  exist since Iteration 8 (`crates/yojaka-cli/tests/fault_kill.rs`), slowed down by a debug-only switch
  (`YOJAKA_TEST_OP_DELAY_MS`) in `Journal::write`.

## Addendum (2026-11-11, Iteration 8): undo refuses an interrupted run

The fault tests found that `undo` run after a crash, **before** `apply --resume`, restored the steps the
journal recorded as done and refused only the step left half done (begun, never finished). The later
resume then finished that one step, so a Live Photo could end with one half restored and the other in the
Trash. Nothing was lost, but the pair was split.

Decision: `undo` checks the journal first. If any `apply` step was begun and never finished, it changes
nothing and returns `YojakaError::Interrupted`, which says how to finish the run
(`yojaka --write apply <plan copy> --resume`, using the copy of the plan kept next to the journal). After
the resume, undo works as before. A run stopped with Ctrl-C is not affected: it stops between files and
leaves no half-done step. The cost: a crashed run must be finished before it can be undone, even when the
user only wants it gone. For trash, copy and move that is safe, and the undo then reverses all of it.

Tests: `exec::an_undo_tried_before_the_resume_is_refused_and_never_splits_a_pair` (both crash points),
`fault_kill::undo_refuses_an_interrupted_run_and_the_printed_resume_command_works` (real kill inside a step,
then the printed command).
