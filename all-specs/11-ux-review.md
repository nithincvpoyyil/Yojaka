# 11: UX review of the TUI (Iteration 9)

Written 2026-11-22 from seven screenshots of the app on a real folder (1,317 photos on the Desktop, read-only
mode, a true-colour terminal) and a read of `theme.rs`, the footer in `view.rs` and the help text in `keymap.rs`.

**Status: decided and built 2026-11-22, waiting for the user's sign-off.** The user's answers to §9 and what
was done for each item are in §10. Each item has an ID (U1, U2, …).

**Severity:** 🔴 can lead to a wrong decision about the user's files, or is a real bug · 🟡 confusing or slows
the user down · 🟢 polish.

## 1. Summary

The base is good: a calm dark theme, numbered tabs, plain English on Actions and Organize, and safety that is
visible all the time (the READ-ONLY badge, "Nothing changes until you confirm", ✓ keep / × delete). The Actions
wizard and the Organize form are the best screens.

The biggest gap is **Duplicates**. It is the riskiest screen, but it gives the least help: there is no cursor in
the list of copies, not enough information to choose one, and a footer of letter keys. That goes against our own
rule for a discoverable TUI (forms with ↑↓ + Enter, no hidden letter keys). The second gap is **consistency**:
the footer, the key for "find" and the names for things change from tab to tab. There is also one real bug in
the 256-colour fallback (U40).

## 2. What works (keep it)

- Safety is always visible: the badge, "Nothing changes until you confirm", "the originals stay where they
  are", and "moves the extra copies to the Trash".
- The wizard shows a live example path next to each layout. The Organize preview shows counts per folder and
  an example (`DSC05083.JPG → 2026/Apr/DSC05083.JPG`).
- Numbered tabs, the same panel frames everywhere, the ▲ sort marker, the `Groups 1/254` counter.
- Keep and delete never depend on colour alone: ✓ and × plus the words. Good for colour-blind users.
- Most of the text is short, plain sentences.

## 3. Suggested order

1. **U40**: fix the 256-colour mapping. Small change, real bug, affects everyone on Terminal.app.
2. **Duplicates**: U25, U26, U27, U28, U30 first; then U29 (the form rework) if the user agrees.
3. **Footer and words**: U1, U2, U3 and the word list in §7. Mostly text in `view.rs` and `keymap.rs`.
4. **Organize**: U32 (month folder order), U33 (what the preview leaves out).
5. **Dashboard**: U17, U18.
6. The rest is polish. U24 (photo preview) is a larger feature, probably after v1.0.

## 4. All screens

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U1 | 🔴 | `W write` is unclear, and it is the key with the biggest consequences. The badge says READ-ONLY but not how to change it. Organize's `a` already asks to turn on write mode during the review ("turns on write mode if you agree"). | Make Duplicates' `a` do the same if it does not already. Then drop `W` from the footers. If a `W` key stays, label it `W allow changes`. |
| U2 | 🟡 | The footer is different on every tab. `q quit` is first on most screens but last in the wizard. "tabs" vs "screens". Duplicates has no tab hint. The main action is buried in the middle. | One layout: this screen's keys on the left (main one first), then `1-6 tabs · ? help · q quit` on the right, the same on every screen. All in `crates/yojaka-tui/src/view.rs:290-385`. |
| U3 | 🟡 | The same word means different things, and the same thing has different keys. `f find` on Duplicates means "look for duplicates"; `/ find` in the File browser filters the list. History uses `j k move`; elsewhere it is `↑↓ choose`. `R scratch` vs `R from scratch`. | `f look for duplicates`; `↑↓ move` everywhere (j k still works, just not shown); `R full rescan` on both screens. |
| U4 | 🟡 | Every key hint has the same weight: `a review` looks the same as `q quit`. | Draw the one main next step as a chip (black on yellow, like `badge_write`), e.g. ` a  Review 77 files `. |
| U5 | 🟡 | Three file counts: 1,317 in the header, "1,355 files seen" and "Photos 1,316" on the Dashboard, with no explanation. | Explain the gap once: `1,355 files seen · 1,316 photos & videos · 39 skipped (not photos or videos)`. |
| U6 | 🟡 | Full `/Users/<name>/…` paths take up width and hide the part that differs. | At display time only, replace the home folder with `~` (the reverse of `expand_home` in `organize.rs:384`). In Duplicates and the Details pane, show paths relative to the opened folder. The folder changes with each run (`yojaka <path>`, or else the current folder: `yojaka-cli/src/main.rs:90-93`), so this is computed, never hard-coded. Real paths are unchanged everywhere else. |
| U7 | 🟢 | Large empty areas on Actions, Dashboard, History and the Duplicates detail pane. | Use them for "what to do next" (U18, U37). |

## 5. Screen by screen

### Actions: start screen

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U8 | 🟡 | Nothing says how to work on a different folder. | One line under the intro: `Working in ~/Desktop. To use another folder, quit and run yojaka <folder>.` No folder picker inside the app: one folder per run is decision A4 in `10-actions-plan.md`. |
| U9 | 🟢 | Starting with no path uses the current folder, so it is easy to scan the whole home folder by accident. | A one-line warning on Actions when the opened folder is the home folder or `/`. |
| U10 | 🟢 | Merging everything into one folder is one of the main goals, but it is hidden as a layout inside "Organize into folders". | Name it in the title: "Organize or merge into folders". |
| U11 | 🟢 | The step chains (`Scan → Summary → …`) are bright cyan and compete with the job titles. The title of the unselected job is yellow bold, and yellow means "focus" elsewhere. | Step chains muted; unselected job titles white bold. |

### Actions: wizard

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U12 | 🟡 | Progress is shown twice: the boxes at the top and the checklist below. | Keep the checklist (it shows the answers). Shrink the boxes to one line, or remove them. |
| U13 | 🟡 | Summary says no file has GPS, yet "By year and place" and "By place" are offered as normal choices. Either one puts every file into `No Location`. | Grey them out with "(no files have GPS)". |
| U14 | 🟢 | The examples use made-up names (`IMG_0001.heic`, `India/Panaji`). | Use a real file from the folder, as Organize already does. |
| U15 | 🟢 | `Waiting for you · 0:11` is a running clock on a step where the user is choosing. The `0:00` next to Scan is unclear. | Hide the clock while waiting for the user. Show times only for steps that take a while. |
| U16 | 🟢 | Merge into one folder does not say what happens when two different photos have the same name. | Add it to the description (e.g. "same name, different photo: `DSC05310 (2).JPG`", whatever the rule really is). |

Text: "none of the files has GPS" → "No files have location (GPS) data". "how should the files be arranged?" →
capital H.

### Dashboard

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U17 | 🟡 | The full-width bright green bar at 100% is the loudest thing on the screen and says the least. No time for the last scan. | When idle, one line: `✓ Up to date · scanned 5 min ago · 1,355 files seen`. Show the bar only while a scan runs. |
| U18 | 🟡 | The screen reports facts but suggests nothing to do. | A **Next steps** panel: "254 duplicate groups, 229 MB can be freed → 4", "18 photos have no date", "No GPS, so place layouts won't help". |
| U19 | 🟢 | The Formats and Top places panels hold one or two lines but take half the screen. | Size panels to their content. |
| U20 | 🟢 | Small mismatches: camera counts are cyan, other counts white; `no date 18` is not aligned with the year counts; in the Library line, `Errors` is yellow but `iCloud-only` and `located` are muted. | One style for labels, one for numbers, numbers right-aligned. |

Text: `located 0%` → `with location 0%`.

### File browser

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U21 | 🟢 | The Length and Place columns are empty for every file and take about 20% of the width. | Hide columns with no values in the current folder. |
| U22 | 🟢 | `Files · all · 1,317 · by name ▲` is cryptic. | `1,317 files · all kinds · sorted by name ▲`. |
| U23 | 🟢 | `Enter` opens the file in the default app, but the footer does not say so. | Add `Enter open` to the footer. |
| U24 | 🟡 | No picture of the photo. For a photo tool this is the feature users will miss most, and it is close to essential for judging look-alike groups. | Later (probably after v1.0): an inline preview in Details with `ratatui-image` (iTerm2, Kitty, Ghostty, WezTerm), falling back to text where not supported. |

Text: `(exif)` → `from camera`; `jpeg` → `JPEG` (also on the Dashboard).

### Duplicates

The riskiest screen, and the one that needs the most work.

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U25 | 🔴 | No visible cursor in the copies pane, so the user cannot tell which file `Space mark` and `Enter keep` act on. The path at the bottom repeats the "keep" path. `Tab` (switch groups / copies) is not in the footer. | A ▶ cursor and selection bar on the focused copy. Add `Tab groups / copies` to the footer. |
| U26 | 🔴 | Not enough information to choose a copy: only path and size. "rule: oldest" does not say oldest by what. | For each copy show date taken, date modified and pixel size, and highlight the value that made the rule pick the kept copy. |
| U27 | 🟡 | The summary is a wall of numbers, and it is not clear that 68 + 186 = 254. | Two lines:<br>`Identical   68 groups · 77 copies marked by rule (oldest, then shortest path) · frees 229.4 MB`<br>`Look-alike 186 groups · need your decision` |
| U28 | 🟡 | The group list has no column headers. `6.2 MB` could be the group's total or the space freed (it is the space freed: 2 × 3.1 MB). The word "exact" is on all 254 rows. | A header row (`#  Match  Copies  Frees`) and section headings (IDENTICAL / LOOK-ALIKE) instead of a label on every row. |
| U29 | 🟡 | Eleven letter keys (`f F v Space Enter p x a P M W`), seven of them in the footer. Against our rule for a discoverable TUI. | The Organize treatment: a small form at the top (`Show ‹Identical›`, `Keep rule ‹Oldest, then shortest path›`) and one main action, `Review 77 files →`. Rarely used keys (`F`, `P`, `M`, `x`) stay in `?` help. |
| U30 | 🟡 | The copy label says `delete`, but the file goes to the Trash. | `→ Trash`. Accurate and less scary. |
| U31 | 🟢 | Long paths that look almost the same. | Dim the shared part and bold the part that differs (`BLUE-MOUNTAINS/` vs `test-folder/new-folder/…`). |

Keys: `Space mark` → `Space trash / keep`; `Enter keep` → `Enter keep only this`; `v tier` → `v show`; `p rule`
→ `p keep rule`.

### Organize

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U32 | 🟡 | **Month folders are not in calendar order.** `{month_name}` folders sort A–Z in Finder (Apr, Aug, Dec, Feb, …). The preview sorts months by file count (Sep, Jul, Jun, Oct, …). | Change the "Date" layout to `{year}/{month:02}-{month_name}/{name}` (gives `2026/04-Apr`; `{month:02}` is already supported). Places: `organize/template.rs:22`, the default config snapshot, and the Actions "By year and month" preset. Sort the preview by date. Saved configs keep their own pattern. |
| U33 | 🟡 | The preview leaves out what the user cares about. (a) **Files with no date:** the Dashboard counts 18, but the preview has no undated folder, and 2026 has 627 files here vs 608 on the Dashboard, so they seem to be filed under some other date. Check this. (b) Whether identical files are copied only once. (c) Free space at the destination. | One line for each: `18 files with no date taken → <where they go>`, `77 identical copies copied once`, `needs 7.6 GB · 120 GB free`. |
| U34 | 🟡 | `Pattern {year}/{month_name}/{name}` is developer syntax, shown at the same level as Layout. | Show it only when Layout is "Custom", or under "Advanced". |
| U35 | 🟡 | The layout names differ from the wizard ("Date" here, "By year and month" there). | One list of names, used in both places. |
| U36 | 🟢 | The `Without GPS` row is bold cyan while its note says "not used". The `‹ ›` arrows suggest ←/→ (which works), but the footer only says `Enter change`. | Grey out the whole row when it does not apply. Footer: `←→ change`. |
| U36b | 🟢 | Large gap between folder names and counts in the preview. | Move the counts closer to the names. |

### History

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U37 | 🟡 | Empty state: "Find duplicates (4), mark them, then press a to review." It only mentions duplicates, and "(4)" is unclear. | "Nothing has been changed yet. Every copy, move or trash run from Actions, Duplicates or Organize will be listed here." |
| U38 | 🟢 | While empty, the "Selected run" panel is blank and the footer still offers `d delete history`, `W write` and `j k move`. | Hide the panel and the keys that do nothing. |
| U39 | 🟢 | There is no undo (ADR-0009), so History is the user's only record of what happened. | For each run show where the files went. For trashed files add "restore them from the Trash with Finder → Put Back". |

## 6. Colour and contrast

WCAG contrast ratios, calculated from the values in `crates/yojaka-tui/src/theme.rs`:

| Pair | Ratio | Result |
|---|---|---|
| White on background | 15.3 | ✅ |
| Cyan / yellow / green on background | 12.6 / 9.7 / 8.1 | ✅ |
| Muted text (labels, footer, inactive tabs) | 5.4 | ✅ AA; the weakest text |
| Pink (`delete`) on background | 5.4 | ✅ |
| Blue (borders, bars) on background | 4.3 | ✅ for lines (3:1 needed) |
| White on the selection bar | 7.8 | ✅ |
| Yellow cursor on the selection bar | 5.0 | ✅ |
| **Selection bar vs background** | **1.95** | ⚠️ below 3:1 for UI parts |
| **Text box fill vs background** | **1.54** | ⚠️ text boxes barely look like text boxes |

| ID | Sev | Finding | Recommendation |
|---|---|---|---|
| U40 | 🔴 | **Bug in the 256-colour fallback** (Terminal.app). `c()` at `theme.rs:38` treats the six colour-cube steps as evenly spaced (0, 51, 102, …), but xterm's steps are 0, 95, 135, 175, 215, 255. The background `#122738` becomes teal `#005f5f` (nearest is `#00005f`). Muted text then drops to 3.3:1, pink to 3.3:1 and blue borders to 3.1:1. The text fails AA. | Pick the nearest real step for each channel, and consider the 24-step grey ramp (indices 232–255) when it is closer. Add a test that checks the mapped colours keep text at 4.5:1 or more. |
| U41 | 🟡 | The selected row is marked mainly by a low-contrast fill (1.95:1). Actions and Organize add a ▶ marker; the Duplicates group list and the File browser do not. | ▶ on every selected row. It also works with no colour at all. |
| U42 | 🟡 | The text box fill is too close to the background (1.54:1). | Lighten `INPUT` (`theme.rs:27`), e.g. towards `#2a5a80`, and check the white text on it stays above 4.5:1. |
| U43 | 🟡 | Yellow does too many jobs: panel titles, focused border, active tab, key hints, summary numbers, unselected job titles. Focus is shown only by border colour. | Muted titles on unfocused panels, so focus has two signals. Keep yellow for focus and keys. |
| U44 | 🟢 | Cyan also does too many jobs: values you can change, paths, the selection in an unfocused pane, "186 to review", step chains, "Waiting for you". | Keep cyan for values you can change and for example output. Paths in white or muted. |

## 7. Words

One name for each thing, used on every screen, in `?` help and in the CLI where it applies.

| Now | Suggested | Why |
|---|---|---|
| exact / identical (both used) | **Identical** | The Actions screen already says it. |
| likely | **Look-alike** | The Actions screen already says "look-alikes". |
| `v tier` | `v show` | "Tier" is our word, not the user's. |
| `Space mark` | `Space trash / keep` | Says what the mark does. |
| `Enter keep` (copies pane) | `Enter keep only this` | It removes the others too. |
| `delete` | `→ Trash` | It is a move to the Trash. |
| `f find` (Duplicates) | `f look for duplicates` | `/ find` filters a list elsewhere. |
| `W write` | `W allow changes` (or drop it: U1) | Says what it allows. |
| `R scratch` / `R from scratch` | `R full rescan` | One label, says what it does. |
| `j k move` | `↑↓ move` | The same as every other screen. |
| `1-6 < > screens` | `1-6 tabs` | The same as every other screen. |
| `a review` / `a review & run` | `a review & run` | One label for one action. |
| `located 0%` | `with location 0%` | Plain English. |
| `(exif)` | `from camera` | Plain English. |
| `Without GPS` | `No location` | Matches the folder name. |
| `none of the files has GPS` | `No files have location (GPS) data` | Reads naturally. |
| `Find duplicates (4)` | `Go to Duplicates (press 4)` | "(4)" alone is unclear. |

## 8. Not changing

- **The folder is set when the app starts** (`yojaka <path>`, or else the current folder), one folder per run
  (decision A4 in `10-actions-plan.md`). No folder picker inside the app; U8 and U9 only add a hint and a
  warning.
- **Paths on screen come from the folder being scanned**, which changes with each run. U6 only changes how they
  are shown.
- **Safety stays as it is:** read-only by default, Plan → Dry-run → Confirm → Execute, no undo (ADR-0009).
  U1 changes where write mode is asked for, not whether.

## 9. Questions for the user

1. **U1:** drop the `W` key and let the review ask for write mode (as Organize does), or keep `W` as a shortcut?
2. **U29:** rework Duplicates into a form like Organize, or keep the keys and only fix the cursor, footer and
   summary (U25–U28)?
3. **U32:** change the default month folders to `04-Apr`? Before v1.0, changing the default is fine; saved
   configs keep their own pattern.
4. **U24:** inline photo preview for v1.0, or after?
5. Where do these go: before the I9 sign-off, or as their own phase after it?

## 10. Decisions and what was built (2026-11-22)

**Answers to §9** (asked before building):

| Question | Answer |
|---|---|
| U1: the `W` key | Drop it from the footers. The review asks to turn on write mode (Duplicates already did, like Organize). `W` still works and is in `?` help. |
| U29: Duplicates | Fixes only (U25–U28, U30, U31), same keys. No form rework. |
| U32: month folders | Whole names: `2026/April`. Not `04-Apr`. Finder sorts them A-Z; the previews show them in calendar order. |
| U24: photo preview | After v1.0. |
| Where it goes | Built now, before the I9 sign-off. |

**Words, as built** (§7 changed in two places): the tiers are `identical`, `likely`, `similar` and
`same name` (the code's `similar` tier already means "pictures look alike", so `likely` did not become
"look-alike"); `Space` is `trash / keep`; the keep rules say `oldest modified` / `newest modified`.

**Status per item.** Done = built and covered by a test or snapshot.

| ID | Status | What was done |
|---|---|---|
| U1 | Done | `W` gone from every footer; help says "a review asks for it anyway". Badge unchanged. |
| U2 | Done | `view::footer_hints`: one layout, screen keys left, `1-6 tabs · ? help · q quit` right. On a narrow screen `1-6 tabs` goes first, then the screen's last keys, then `? help`. |
| U3 | Done | `f look for duplicates` / `f look again`, `/ filter`, `↑↓ move` on History, `R full rescan`, `1-6 tabs`. |
| U4 | Done | The main next step is a black-on-yellow chip (`theme::key_chip`). |
| U5 | Done | Dashboard: `1,355 files seen: 1,316 photos and videos, 39 other files`. |
| U6 | Done | `fmt::home` (`~`) and `fmt::rel` (inside the opened folder): Actions, Duplicates, Details, History, review box, Organize destination. |
| U7 | Done | Through U18 and U37. |
| U8 | Done | "To work on another folder, quit and run: yojaka <folder>". |
| U9 | Done | A red warning when the folder is the home folder or `/`. |
| U10 | Done | "Organize or merge into folders". |
| U11 | Done | Step chains muted; the unselected job title white. |
| U12 | Not done | The stage boxes and the step list were both chosen by the user (plan 10, §10 and §10.1). Ask before removing either. |
| U13 | Done | Place layouts greyed with "no file has GPS". |
| U14 | Not done | The examples are still the fixed ones from `actions::ACTIONS`; a real file needs its facts before a plan exists. |
| U15 | Done | No clock while waiting for you; step times under a second hidden. |
| U16 | Done | "A second file with the same name gets (1)." Also: "By year" no longer says `_undated` (files without a date taken go by their file date, as the planner does). |
| U17 | Done | One line when up to date: `✓ Up to date · scanned 5 min ago · …`; the bar only while scanning or partial. |
| U18 | Done | Next steps panel: duplicates (or "Look for duplicates") → 4, tidy → 1, undated, no GPS, iCloud only. |
| U19 | Done | Formats and cameras as tall as they need. |
| U20 | Done | Counts plain everywhere; `no date` is a bar like the years; the Library labels all alike. |
| U21 | Done | Length and Kind hidden when the library has no videos; Place when no file has a place. |
| U22 | Done | `350 files · photos and videos · sorted by name ▲`. |
| U23 | Done | `Enter open` in the footer; `(from camera)` / `(from the video)`; formats upper case. |
| U24 | Later | After v1.0 (the user's choice). |
| U25 | Done | ▶ on the current copy (and group); `Tab groups / copies` in the footer. |
| U26 | Done | Identical groups show `modified`, `taken`, `pixels`, `camera` per copy; differences stand out. Modified is local time (`App::tz`; tests use UTC). |
| U27 | Done | Two lines: `Identical … to the Trash · frees … · keep rule: …` and `To decide … left to decide, one by one`. |
| U28 | Done | Header `# match copies frees`; the tier label stays on each row (the list mixes tiers), plain for identical, accent for guesses. |
| U29 | Not done | The user chose fixes only. |
| U30 | Done | `✕ trash`, `? decide`. |
| U31 | Done | Paths relative to the folder; the folder part all copies share is dim. |
| U32 | Done | Whole month names (core `MONTH_NAMES`); previews and the dry-run folder list in calendar order (`plan::tree::calendar_key`, also used by the CLI). |
| U33 | Done | `N dated by file date`, `N identical left out`, `needs X · Y free on that disk` (red "Not enough room" when short; not shown for a move on the same disk). |
| U34 | Partly | The pattern of a ready-made layout is plain dim text with "Enter: write your own" (not hidden: hiding it would leave no way to a custom pattern without a new Layout choice). |
| U35 | Done | `organize::layout_title`: the Actions names (`By year and month`); two more for the presets Actions does not have. |
| U36 | Done | The Without GPS row is grey when unused ("not used by this layout"); footer `Enter ← → change`. |
| U36b | Done | Counts sit near the longest folder name. |
| U37 | Done | "Nothing has been changed yet. Every copy, move or trip to the Trash … is listed here, newest first." |
| U38 | Done | No details panel and no keys while empty. |
| U39 | Done | `Moved to the Trash: …`; "Yojaka has no undo; to get a file back, open the Trash in Finder and choose Put Back." (The destination of a copy or move is not in the run summary, so it is not shown.) |
| U40 | Done | `theme::xterm_index`: nearest real step per channel, the grey ramp only for greys. Background navy `#00005f`; tests check 4.5:1 for every text colour. |
| U41 | Done | ▶ on the Duplicates groups and copies, the File browser table and the History runs. |
| U42 | Done | `INPUT` `#2a5a80` (2.1:1 on the background, white text 7.3:1). |
| U43 | Done | `theme::panel`: focused title yellow, other titles white. |
| U44 | Partly | Cyan left on values you can change, example paths and the unfocused selection; step chains and the tier of identical groups no longer use it. |
