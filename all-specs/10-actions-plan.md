# 10: Actions tab (Iteration 9, Phase 1A)

Written 2026-11-17, after the user's idea and a review against the architecture, with the decisions below made
with the user the same day. This comes before the detailed dashboard (Phase 1B), because it changes the start
screen and its summary overlaps the dashboard.

**The idea:** a new first tab, **Actions**, with ready-made jobs for people who just want their photos tidied.
Each one runs the same steps: scan the folder → show what is in it → choose a layout → choose where the files
go → review and run. Each action has a short description.

**Acceptance:**
- A never-scanned folder opens on Actions and scans by itself; a folder with saved data opens on the Dashboard.
- All six actions plan and run through the existing Review box and executor, in the TUI and the CLI.
- Identical files are copied or moved once. Running the same action again plans nothing new.
- Copying into the scanned folder (or a folder inside it) is refused; moving there stays allowed.
- Full check clean; the user tries it in the real app (on a copy) and signs it off.

## 1. Decisions (made with the user, 2026-11-17)

| # | Decision |
|---|---|
| A1 | New tab **Actions**, first in the strip: `1` Actions, `2` Dashboard, `3` File browser, `4` Duplicates, `5` Organize, `6` History. |
| A2 | **Start screen.** No saved data for the folder: open on Actions, and the scan starts by itself as it does today. Saved data: open on the Dashboard. **No yes/no question**: landing on Actions *is* the offer, and every other tab is one key away. |
| A3 | **Replaced 2026-11-18 by §10 (a pipeline of steps).** Was: **One page**, not a step-by-step wizard (mockup in §3). The form pattern of the Organize screen: ↑↓ choose a row, Enter acts on it, shaded text box, `‹ choice ›` values. |
| A4 | **One folder**: the folder Yojaka was opened on. To tidy several folders, open their parent. |
| A5 | **Six actions** (table in §2). The "photos & videos" layout is dropped because Merge already splits photos from videos. |
| A6 | **Merge** is `{kind}/{name}`: `Photos/` and `Videos/` side by side, with no folders inside them. A Live Photo's `.MOV` stays next to its photo in `Photos/`, because a pair is never split. |
| A7 | **Identical files are copied or moved once, in every action.** This uses only the exact tier (same bytes), as ADR-0008 allows. The copy that goes is chosen by the keep rules (saved `dupes.keep_rules`, or else the default `oldest-modified, shortest-path`). With move, the other copies stay where they are; nothing is deleted. A Live Photo pair counts as one item. |
| A8 | **Places come from GPS only.** Actions never infers a place; files without GPS go to `No Location`. (The Organize screen keeps its "guess the place" option.) |
| A9 | **Summary** ("What's here"): photo and video counts, total size, first and last year, top countries and the number without GPS. All of it comes from the existing statistics; no new query. |
| A10 | **Confirm** is the existing Review box and executor: no-undo note, `y` for a copy, `move N files` typed for a move, write mode asked for as on Organize. **No new write path.** |
| A11 | **CLI parity (FR-3).** `yojaka organize PATH DEST --preset year\|year-place\|merge\|format` and a new `--skip-identical` flag. Existing presets (`date`, `place`, `place-date`, `kind-date`) keep their names. No new subcommand. |
| A12 | `{format}` gives upper-case folder names (`HEIC`, `JPEG`, `MOV`). APFS ignores case by default, so a destination made earlier with `heic` still gets the same folder. |

## 2. The six actions

| Action | Pattern | CLI `--preset` | Description shown |
|---|---|---|---|
| Merge into one folder | `{kind}/{name}` | `merge` (new) | Every photo in one folder, every video in another. No folders inside. |
| By year | `{year}/{name}` | `year` (new) | One folder per year. Files with no date go to `_undated`. |
| By year and month | `{year}/{month_name}/{name}` | `date` | A folder per year, with a folder per month inside (`2023/Aug`). |
| By year and place | `{year}/{country}/{city}/{name}` | `year-place` (new) | Year first, then country and town from the GPS. Files without GPS go to `No Location`. |
| By place | `{country}/{city}/{name}` | `place` | Country and town from the GPS. Files without GPS go to `No Location`. |
| By file format | `{kind}/{format}/{name}` | `format` (new) | Photos and videos apart, then one folder per format (`Photos/HEIC`, `Videos/MOV`). |

Every description ends with "Identical files are copied once." (or "moved once" when Move is chosen).

The four new patterns are added to `organize::PRESETS`, so the Organize screen's Layout row offers them too.

## 3. Mockups

### 80×24, scan finished, plan ready

```
 Yojaka · [READ-ONLY]                                      ✓ 1,155 files scanned
 ╭───────────╮╭───╮╭───╮╭───╮╭───╮╭───╮
 │ 1 Actions ││ 2 ││ 3 ││ 4 ││ 5 ││ 6 │
─┘           └┴───┴┴───┴┴───┴┴───┴┴───┴─────────────────────────────────────────
┌ What's here ─────────────────────────────────────────────────────────────────┐
│ 1,042 photos · 113 videos · 2.6 GB · 2016 – 2024                             │
│ Places: India 812 · France 96 · Japan 40 · 207 without GPS                   │
└──────────────────────────────────────────────────────────────────────────────┘
┌ Choose an action ──────────────┐┌ What it does ──────────────────────────────┐
│ ▶ Merge into one folder        ││ Every photo in one folder, every video in  │
│   By year                      ││ another. No folders inside. Identical      │
│   By year and month            ││ files are copied once.                     │
│   By year and place            ││                                            │
│   By place                     ││ Photos/IMG_0001.heic                       │
│   By file format               ││ Photos/IMG_0001 (1).jpg                    │
│                                ││ Videos/MVI_0042.mov                        │
│                                ││                                            │
│                                ││ 1,088 files into 2 folders · 2.4 GB        │
│                                ││ 67 identical files left out                │
└────────────────────────────────┘└────────────────────────────────────────────┘
 Destination  ░ ~/Pictures/Yojaka Organized                    ░
 Action       ‹ Copy ›  the originals stay
 ▶ Review and run
 ↑↓ choose · Enter select · ? help · q quit
```

### 80×24, first open, scan running

Only the panels change. The rest of the screen is the same, and the actions can be browsed already.

```
┌ What's here ─────────────────────────────────────────────────────────────────┐
│ Scanning… 48,210 files found · reading details 31%                           │
│ The summary and the plan are ready when the scan ends.                       │
└──────────────────────────────────────────────────────────────────────────────┘
 ...
│                                ││ Waiting for the scan to finish…            │
```

### 160×48

The same layout, wider and taller. "What it does" shows the plan's folder glimpse (top folders with file counts,
as on the Organize screen) instead of three example paths, and "What's here" adds the top cameras line. The
tab strip shows every name from about 90 columns; below that it shows the numbers and the current name, as it
does today.

### Behaviour

- ↑↓ moves through all rows: the six actions, Destination, Action, "Review and run". The list marks the chosen
  action with `▶`; moving onto another action chooses it.
- Enter on an action jumps to Destination. Enter on Destination opens the text box (Enter keeps, Esc undoes,
  Ctrl-U clears, `~` is the home folder). Enter on Action takes the next choice (← → also step it). Enter on
  "Review and run", or `a`, opens the Review box.
- The plan follows the settings by itself, like the Organize screen: a change re-plans, and a change during a
  plan stops it and plans again. Planning waits for a scan or a duplicate search to finish.
- The first plan also finds identical files, which reads the files that share a size ("Comparing files…" with
  progress). The hashes are saved, so later plans are quick.
- After a run: the report, then a rescan (as today), because the folder changed.

## 4. Core changes (`yojaka-core`)

1. **Presets.** Add Year, Year + Place, Merge (one folder per kind) and Format to `organize::template::PRESETS`.
   `{format}` renders upper case (A12).
2. **Skip identical files.** `OrganizeOptions.skip_identical: bool`. When set, the planner first runs the exact
   tier (`dupes::find_exact`, cached hashes, changed files left out as today). In each group it keeps one item
   chosen by the keep rules and plans `Skip(Identical)` for the rest. That is a new `SkipReason`, so older plan files still
   load. Live Photo pairs are one item, as in `dupes`. New summary counter `identical` and the bytes saved;
   `Plan::no_undo_note()` and the Review box add "N identical files left out (they stay where they are)".
   Progress is reported as `TaskKind::Hash`, and cancel works.
3. **Already there, also under a suffix.** When `name.jpg` at the destination is a different file, check
   `name (1).jpg`, `name (2).jpg` and so on for the same content before choosing a new suffix. This fixes the open
   item in STATUS: organizing twice re-offered files as `IMG (2).gif`.
4. **Linear suffix search.** Remember the next free suffix per destination name instead of starting from `(0)` for
   every file (`planner.rs:481`). Today `k` files with the same name cost about k²/2 disk checks, and a flat folder
   collects every `IMG_0001.JPG`.
5. **Copies into the scanned folder are refused** (changed while building, 2026-11-18: moves there stay
   allowed, because tidying a folder in place is a normal use), in the planner (plain error) and again in the
   executor's validation (`exec/organize_run.rs`), for plans made earlier or edited by hand. The scanned folder
   itself counts as inside. This applies to the Organize screen and the CLI too (new FR-43a).
6. **`yojaka_core::actions`**: `pub struct Action { id, title, description, template }` and `pub const ACTIONS`.
   The TUI, the CLI help and the later GUI read the list from here. Actions always set `skip_identical` and never
   set `infer_location`.

No schema change: the skip reason and counters live in the plan file, not in the index.

## 5. TUI changes (`yojaka-tui`)

- `Screen::Actions` first in `Screen::ALL`; keys `1`-`6`; `keymap.rs` (help overlay), tab strip.
- **Start screen without a flash:** `yojaka_tui::run` checks whether the folder's saved data exists before the
  first frame and sets `App::screen`. `open_decision` (`app.rs:494`) still starts the scan as today.
- `ActionsState` (`actions.rs`) and `screens/actions.rs`. It reuses the Organize text box and the default
  destination (`<parent>/Yojaka Organized`, never inside the folder); move the shared bits into a helper rather
  than copying them.
- `plan_blocker` and `follow_settings` cover the Actions screen as well as Organize. Only one plan is worked out
  at a time.
- Review box: the identical-files line. Write mode asked for exactly as on Organize.

## 6. CLI changes (`yojaka-cli`)

- `PresetArg`: `Year`, `YearPlace`, `Merge`, `Format`, mapped to the `PRESETS` entries by name, not by index.
- `organize --skip-identical`. The plan summary prints the identical count.
- Help snapshot updated.

## 7. Tests

- Core (`tests/organize.rs`, `tests/organize_exec.rs`):
  - each new preset renders as in §2
  - `skip_identical`: one of each identical set is planned; a Live Photo pair matches only a pair; with move the
    others stay in place; the kept copy follows the keep rules
  - running the same plan settings again after apply plans no transfers (all `AlreadyThere` or `Identical`),
    including names with a ` (n)` suffix
  - 5,000 files with one name plan in linear time (count the disk checks or time it with a generous limit)
  - a copy into (or inside) the scanned folder is refused by the planner, and by apply for a
    hand-edited plan
  - the plan invariant property test still holds with `skip_identical`
- CLI: `organize --preset merge --skip-identical` → `plan show` → `--write apply` → second plan is empty.
- TUI:
  - snapshots at 80×24 and 160×48: scanning, ready, plan failed, read-only Review
  - key tests: start screen with and without saved data, ↑↓ / Enter through the rows, `a` opens the Review
  - renumbered keys `1`-`6`, and the existing screen tests updated
- By hand: the real app on a fixture tree, then the user on a copy of a real folder.

## 8. Steps

1. Specs (this file; product spec FR-1, FR-40, FR-43a, FR-70-74; tech spec §6.4, §7, §8; plans 03 and 09;
   STATUS). **Done 2026-11-17.**
2. Core: §4 items 1-6 with their tests. Write ADR-0010 "Actions and identical files in organize". **Done 2026-11-18.**
   Also: hard links count as identical (found by the tests).
3. CLI: §6 with its tests. **Done 2026-11-18.**
4. TUI: §5, snapshots, key tests; drive it in a pty on a fixture tree. **Done 2026-11-18.**
5. The user tries it and signs it off. Then Phase 1B, the detailed dashboard.

Size: **M**, about 4-5 days.

## 9. Not in scope

- Several folders at once (A4).
- Inferred places in Actions (A8).
- A "skip identical files" row on the Organize screen (the core option and the CLI flag exist; it can be added
  later if wanted).
- Removing source folders that a move left empty.
- Saving the chosen action in `config.toml`.

## 10. Redo as a pipeline (2026-11-18, user feedback)

The user tried the one-page tab and did not like it: "it should be simple, like a pipeline run in CI, not a
dashboard type", "like Jenkins Blue Ocean". Options were shown as mockups and the user chose on 2026-11-18.
This section **replaces A3 and §3/§5** (the one page). A1, A2, A4-A9, A11, A12 and all of §4 (core) stand.

### Decisions

| # | Decision |
|---|---|
| P1 | **Vertical steps, like a CI run log.** `✔` done, `●` current, `○` waiting, `✖` failed. A finished step shrinks to one line that keeps its result; the current step opens below its line; later steps are greyed. |
| P2 | **Pick a pipeline first.** The tab opens on a short list with a description of each: **Organize into folders** and **Remove duplicates**. Enter starts one. |
| P3 | **Organize:** Scan → Summary → Layout → Destination → Confirm → Run. Layout offers the six actions of §2 (the user's four plus year/month and file format). |
| P4 | **Remove duplicates:** Scan → Summary → Find identical → Keep which copy → Confirm → Run. Identical files only (exact tier, ADR-0008); look-alikes are left for the Duplicates tab. Extra copies go to the Trash. |
| P5 | **The opened folder** is the source (A4 stands). No folder picking inside the tab. |
| P6 | **Scan and Summary run by themselves.** Starting a pipeline scans the folder for changes (quick when saved data exists), then shows the summary, then stops at the first step that needs a choice. |
| P7 | **Confirm is a step, not a pop-up.** It shows what the Review box shows today (counts, size, identical files left out, folders by size, the no-undo note) and takes the same answer: `y` for a copy or a small Trash plan, the typed phrase for a move or 50+ files. Same plan, same executor, **no new write path**. Write mode is asked for here if it is off. |
| P8 | **Run shows a progress bar**: files done / total, the current file, time left. Ctrl-C stops after the current file. When it ends, the line keeps the result and the folder is scanned again (as today). |
| P9 | **Esc goes back one step** (a choice is kept, the plan is dropped). Esc on the first choice goes back to the pipeline list. Esc during the scan stops it. Esc does nothing while Run is going. |

### Mockups (80×24)

The pipeline list, before anything starts:

```
 Yojaka · [READ-ONLY]                                              12,871 files
 ╭───────────╮╭───╮╭───╮╭───╮╭───╮╭───╮
 │ 1 Actions ││ 2 ││ 3 ││ 4 ││ 5 ││ 6 │
─┘           └┴───┴┴───┴┴───┴┴───┴┴───┴─────────────────────────────────────────
 Ready-made jobs for ~/Pictures. Each one scans the folder, shows what is in it,
 asks a few questions, then runs with a progress bar. Nothing changes until you
 confirm.

 ▶ Organize into folders
     Copies or moves every photo and video into new folders: by place, by
     year, or all in one. Identical files are copied once.

   Remove duplicates
     Finds files with exactly the same bytes and moves the extra copies to the
     Trash, keeping one of each. Look-alikes are left for the Duplicates tab.




 ↑↓ choose   Enter start   ? help   q quit
```

Organize, at the Destination step:

```
 Organize into folders · ~/Pictures
 ──────────────────────────────────────────────────────────────────────────────
 ✔ Scan          12,871 files · 1,204 changed since last time             0:06
 ✔ Summary       12,480 photos · 391 videos · 144.4 GB · 2019 – 2023
                 India 6,100 · France 2,400 · Japan 1,300 · 3,071 without GPS
 ✔ Layout        By year and place   2023/India/Kochi/IMG_0042.HEIC
 ● Destination   where should the files go?
   │ Folder   ░ ~/Pictures/../Yojaka Organized                         ░
   │ Action   ‹ Copy ›  the originals stay where they are
 ○ Confirm
 ○ Run

 ↑↓ choose   Enter next   ← → copy / move   Esc back a step
```

Organize, at the Confirm step (copy, write mode on):

```
 ✔ Destination   copy to ~/Yojaka Organized
 ● Confirm       copy 12,480 files (141.0 GB) into 212 folders
   │ 391 identical files stay where they are: one of each is copied.
   │ 2023/India/Kochi        +1,840 files (20.1 GB)
   │ 2022/France/Paris       +1,210 files (13.9 GB)
   │ … 210 more folders (yojaka plan show --full-tree)
   │ Yojaka has no undo: a copy is your decision, and final.
   │ y / Enter  copy        Esc  back
 ○ Run
```

Remove duplicates, at the Keep step:

```
 Remove duplicates · ~/Pictures
 ──────────────────────────────────────────────────────────────────────────────
 ✔ Scan             12,871 files · nothing changed                        0:02
 ✔ Summary          12,480 photos · 391 videos · 144.4 GB · 2019 – 2023
 ✔ Find identical   1,204 groups · 2,810 extra copies · 18.2 GB           0:41
 ● Keep which copy  in each group, one copy stays
   │ ▶ The oldest            the first copy you had (oldest change date)
   │   The shortest path     the one closest to the top folder
   │   The newest            the latest change date
   │   The largest picture   the highest resolution
 ○ Confirm
 ○ Run

 ↑↓ choose   Enter next   Esc back a step
```

Run, while going and after:

```
 ✔ Confirm       copy 12,480 files → ~/Yojaka Organized
 ● Run           Copying ████████████████░░░░░░░░░░░░  6,210 / 12,480
                 IMG_4410.HEIC · 71.2 GB · 2:13 left · Ctrl-C stops after this file

 ✔ Run           12,480 copied · 212 folders made · 0 failed               4:51
                 Done. The folder is being scanned again.
 Enter  back to the list   6  History
```

A failed step shows `✖` and its reason on its line (a scan error, "copying into the scanned folder is refused",
a full disk). Esc goes back to fix the choice. Skipped and failed files are listed under the Run line as the
report does today (the first six, then a count).

At 160×48 the same steps, with more of the folder list in Confirm and the top cameras in Summary. Below 20
rows the description above the list is dropped first, then the result's second line.

### TUI changes

- `actions.rs` becomes a small state machine: `Pipeline { kind, step, results }`. `PipelineKind::{Organize,
  Dupes}`, `Step::{Scan, Summary, Layout, FindIdentical, Keep, Destination, Confirm, Run}`, each with
  `Waiting | Running(progress) | Done(result line) | Failed(reason)`.
- `screens/actions.rs` draws the list and the steps. The one-page layout and its snapshots go.
- Organize reuses `OrganizeState` (destination box, copy / move, the plan, `skip_identical`, no inferred places).
  Remove duplicates reuses the duplicate search (`Cmd::FindDupes`, exact tier) and the keep-rule `PRESETS`
  (`p` on Duplicates), then `Plan::from_dupes`. Both go through `start_apply`, so `Cmd::ApplyPlan` and the
  executor are unchanged.
- The Confirm step renders the review lines the `Review` box uses today. Move the line-building out of
  `screens/modal.rs` into a shared function so both places say the same thing. Typing and `y` work as in the box.
- Progress for Scan, Find identical and Run comes from the events the dashboard and the Running box already
  get. While a pipeline runs, the Running pop-up is not shown on the Actions tab (the Run step is the progress).
- Footer and help overlay (`keymap.rs`) list the keys of each step.

No core change, no CLI change: the CLI already has `organize --preset … --skip-identical` and `dupes --keep … --plan-out`
followed by `apply`.

### Tests

- Key tests: the list, Enter starts a pipeline and a scan, Scan → Summary → first choice by themselves, ↑↓ / Enter
  through each step, Esc back a step and back to the list, Esc stops a scan, Ctrl-C during Run cancels.
- Confirm: read-only asks for write mode; a copy takes `y`; a move needs its phrase; a refused destination shows `✖`.
- Remove duplicates: only exact groups are planned; the keep choice changes which copy stays; nothing to remove
  says so and stops.
- Snapshots at 80×24 and 160×48: the list, Organize at Destination, Confirm, Run going, Run done, a failed step,
  Remove duplicates at Keep.
- Drive the real app in a pty on a fixture tree, both pipelines, copy and move.

### Steps

1. This section; STATUS. **Done 2026-11-18.**
2. Shared review lines; the state machine and its key tests. **Done 2026-11-18.**
3. The screen, the snapshots, the footer and the help. **Done 2026-11-18.**
4. Remove duplicates pipeline. **Done 2026-11-18.**
5. pty run on a fixture tree; full check. **Done 2026-11-18** (441 tests). The user tries it and signs it off.

Built as planned, with these details settled while building:
- The code: `actions.rs` (`Job`, `Step`, `Pipeline`, `ActionsState`), `app/pipeline.rs` (driving the steps),
  `screens/actions.rs` (the list and the steps), `screens/modal.rs` (`review_head`, `review_folders`).
- Confirm works out the plan the moment it opens (the Enter on Destination), not on the next tick.
- On the Destination step, Enter on Folder types; keeping the folder moves to the copy / move row, where Enter
  goes on. ← → switch tabs everywhere on the tab except that row.
- Run blocks every key but Ctrl-C (as the Running box did): no quitting or tab switching mid-run.
- A long Confirm line wraps; on a short terminal the folder list shrinks first, and the answer line always stays.
- The Run line says `315 files copied · 2 folders made · 692.7 KB` (or `30 files moved to the Trash (… freed)`);
  the rest of the report goes under it.

Size: **M**, about 3 days.
