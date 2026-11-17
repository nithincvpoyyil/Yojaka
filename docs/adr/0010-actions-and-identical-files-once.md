# ADR-0010: Actions tab, and identical files once in organize

Status: accepted (Iteration 9, Phase 1A; built 2026-11-18; the screen redone as a pipeline the same day, see
"Amended" below; waiting for the user's sign-off)

## Context
The user wanted ready-made jobs for people who just want their photos tidied: scan, see what is in the
folder, pick a layout (including "merge everything into one folder"), pick a destination, confirm. A review
against the architecture (plan `10-actions-plan.md`) found that most of it already existed (scan, statistics,
the organize planner, the Review box, the executor), but a flat merge copied every identical file again with a
` (n)` name (67 of 118 renamed files in a 1,000-file fixture), organizing twice re-offered files, the ` (n)`
search was quadratic per name, and nothing stopped a copy into the scanned folder.

## Decision
- **Actions is the first tab** (`1` Actions … `6` History). A folder without saved data opens there and is
  scanned by itself; a folder with saved data opens on the Dashboard. Nothing is asked. "Saved data" is
  `Engine::has_index`, checked before the first frame.
- **One page**: "What's here" (existing statistics), six actions with a description and examples, destination,
  copy / move, "Review and run". The list lives in `yojaka_core::actions` so a GUI can show the same jobs.
- **Reuse, not a second form**: the page holds its own `OrganizeState` (destination box, copy / move, plan
  lifecycle). The app tracks whose plan is in flight (`plan_owner`) so the answer goes back to the right page;
  one plan at a time. Confirm is the existing Review box and executor: no new write path.
- **Four presets** added to `PRESETS` (Year, Year + Place, Merge `{kind}/{name}`, Format `{kind}/{format}/{name}`);
  the CLI `--preset` maps by label. `{format}` renders upper case.
- **Identical files once** (`OrganizeOptions.skip_identical`, CLI `--skip-identical`, always on in Actions):
  `Engine::organize` runs the exact tier first (cached hashes), the keep rules pick the file that goes from each
  set, the others become `Skip(Identical)`. Hard links count too: one name goes, unless the file's content
  already stays. A Live Photo pair is one item and only matches a pair (as in `dupes`), so a pair's video can
  still go next to an identical lone `.MOV`; that is the price of never splitting a pair.
- **Already there, under any name**: a file is skipped when its destination folder already holds a file with
  the same bytes that no other file of the plan claimed (the same name first). Folders are listed once and
  indexed by size; hashes are remembered. Organizing twice offers nothing new, also for ` (n)` names.
- **Linear naming**: the next free ` (n)` is remembered per destination name.
- **Copies never go inside the scanned folder** (FR-43a), refused by the planner and again by `apply`. Moving
  inside it stays allowed, because tidying a folder in place is a normal use. (The plan first said "any
  destination inside"; changed while building, because that would have removed tidying in place.)
- **Amended 2026-11-18: a pipeline, not one page** (plan 10 §10, user feedback "simple, like a pipeline run in
  CI, not a dashboard"). The tab lists two jobs, Organize into folders and Remove duplicates, each a column of
  steps like a CI log: Scan → Summary → Layout → Destination → Confirm → Run, or Scan → Summary → Find identical
  → Keep which copy → Confirm → Run. Scan, Summary and Find identical run by themselves; a finished step keeps
  its result on one line. Confirm shows the Review box's lines inline (shared `review_head` /
  `review_folders`) and takes the same answers (`y`, or the typed phrase; write mode asked for first). Run shows
  the executor's progress in the step instead of the Running box. Still no new write path: `Cmd::ApplyPlan`
  and the executor are unchanged. Remove duplicates plans only exact groups, from a copy of the search's groups
  with the chosen keep rule, so the Duplicates tab's own marks (and any look-alike marked by hand) never enter
  it. Another scan while a job waits on a choice drops the job ("start the job again"), because what it found
  is stale. The one-page "What's here" panel and "Review and run" row are gone.
- **Progress**: while a plan compares files, the duplicate search's events are the plan's progress ("Comparing
  files… n of m"), not the Duplicates screen's, and its per-file issues do not count as scan errors.

## Consequences
- `PlanSummary` gained `identical` and `identical_bytes`, `SkipReason` gained `Identical`, `ApplyReport` gained
  `identical` (all `serde(default)`, so older plan files load). `YojakaError::Refused` is new. No schema change.
- The first Actions plan reads the files that share a size (the exact tier); later plans use the saved hashes.
- With move, identical files that stay are left in the source folder; nothing is deleted.
- Tests: `tests/organize.rs` (identical once, pairs, hard links via the exec test, copy into the folder, 1,500
  same names, `has_index`), `tests/organize_exec.rs` (organizing twice, identical once with copy and move, a
  hand-edited plan), CLI `merge_with_identical_files_once...`, `organize_copies_into_the_scanned_folder...`, TUI
  `actions_screen::*` and `actions::tests` (rewritten for the pipeline: every step at 80×24 and 160×48, both
  jobs, Esc back, Ctrl-C during Run, a stopped scan, a stale job). The pipeline was driven in a pty on a 300-file
  fixture: Organize copied 315 files with 35 identical left out; Remove duplicates moved 30 copies to a test
  Trash. Before that, the one page was driven by hand in a pty on a fixture tree: first open, plan, review,
  copy, second open on the Dashboard, the same action again plans nothing, a copy into the folder is refused.
