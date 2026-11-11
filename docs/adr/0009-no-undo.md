# ADR-0009: No undo

Status: accepted (2026-11-12, Iteration 9). Supersedes the undo parts of ADR-0006 and ADR-0007.

## Context
Iterations 5 and 6 shipped `yojaka undo` and a History screen `u` key that put files back from the
journal. Keeping undo correct is costly: every new kind of operation needs an exact inverse, the
inverse has to survive crashes, and every fault test had to run twice (apply, then undo). The user
decided that Yojaka should not try: a move or a copy is the user's decision, and Yojaka only warns.

## Decision
- **No undo.** `yojaka undo`, `exec::undo`, `UndoReport`, `TaskKind::Undo`, `Run::Undo`, the
  `Interrupted` error, the History `u` key and the undo box are removed. History is a list of what
  was run (`applied` or `incomplete`); it no longer has `undone` states or counts.
- **A warning instead.** `Plan::no_undo_note()` gives one line per mode ("Yojaka has no undo ...").
  `yojaka apply` prints it before asking for confirmation; the TUI report box, the write-mode
  question, the History detail panel, the help overlay and `yojaka history` say it too.
- **Trash stays the safety net for dedupe.** Files are still moved to the Trash, never deleted, so
  the user can put them back in Finder. Copy and move have no such net.
- **Kept on purpose:** the journal (resume after a crash still needs it), the recorded Trash path,
  and the rollback *inside one run* that puts the first half of a Live Photo pair back when the
  second half fails (`trash::restore`, `transfer::move_back`). That is pair safety, not user undo.

## Consequences
- `apply --resume` is the only way to continue an interrupted run; nothing reverses a finished one.
- Fault tests kill `apply` and check that `--resume` finishes with nothing lost and no half-written
  file. The "kill during undo" tests and the "undo refuses an interrupted run" test are gone.
- Old journals that contain `"run":"undo"` lines are no longer readable (pre-release, so accepted).
