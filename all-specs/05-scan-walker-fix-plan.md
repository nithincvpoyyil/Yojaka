# 05: Scan walker fix plan (jwalk → dua-core, honest progress)

Written 2026-11-08. This plan is for the next session, which may run on a smaller model. Follow the steps in
order and do one phase at a time. Every phase ends with the full check. Phase 1 has been built and tested
already, and its code is in `all-specs/05-scan-walker-phase1.patch`.

## 0. Ground rules (read first)

- Run `source ~/.cargo/env` before any `cargo` command (Rust lives in `~/.cargo`).
- Full check: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check`.
- **Do not commit** unless the user asks. Before Phase 1, ask the user whether to commit the current
  uncommitted work first (Iteration 4 plus the earlier hang fix). Recommended: yes, so this change is its own commit.
- **Never put waiting or sleeping inside the walker** (no `sleep`, no blocking send in a `descend` callback).
  **Never switch to `Order::ParentFirst`** or any ordered walk. Both together are what froze the scan.
- Manual scans only on this repo or on `out/` fixture trees, always with a throw-away `YOJAKA_HOME`
  (`H=$(mktemp -d)`). Never scan the user's real folders. Do not scan anything under `/private/tmp` (it is protected).
- If the code does not match what this plan says (a file moved, a function was renamed, the patch does
  not apply), **stop and tell the user**. Do not improvise with another crate or design.
- Read-only by default; this work never touches user files.

## 1. Background: what went wrong and what is decided

| Finding | Evidence |
|---|---|
| The TUI scan froze for good, not just slowly. jwalk hands out folders in a fixed order. The old back-pressure loop (`while pending > 50_000 { sleep }`) ran inside jwalk's worker callback. A sleeping worker held the folder the consumer needed next, so nothing could move. | The user's `sample`: scan thread in `swtch_pri` (jwalk's `yield_now` loop), 10 walker threads in `nanosleep`, 10 idle processing threads. |
| The trigger is **one folder with more entries than the cap**. The user scanned the repo root, where `target/debug/deps` has about 59k entries. A photo folder with 50k+ files does the same. | Old build, release CLI, repo root: hung 5 of 5 runs. |
| The uncommitted quick fix (cap removed) stops the hang, but memory now grows with the tree: about 270 MB at 1M entries, while the I1 limit is 150 MB (G2: 250 MB). | `docs/perf.md` ("before back-pressure ... 271 MB"); measured 39 → 52 MB on `out/perf/tree`, 74 MB on the repo root. |
| **jwalk is unmaintained.** The repo was archived on 2026-08-05. 0.9.0 (in `Cargo.lock`) says "Use `dua-core` instead". `cargo deny` does not flag it. | crates.io, github.com/Byron/jwalk |
| **Decision: use `dua-core` (pinned `=6.1.0`).** Same author. Unordered (`Order::Completion`), bounded result queue (workers wait when it is full, which is safe because there is no order), `descend` callback to prune folders, never follows symlinks, cancellable, bulk `getattrlistbulk` on macOS. | Walk-only benchmark with a slow consumer: jwalk 29 → 48 MB as the tree grows; dua-core flat at 4 MB. |
| dua-core has **no public file flags, ctime or birth time**. Yojaka keeps its own `lstat` per file for the iCloud dataless check (`SF_DATALESS`, a safety rule). | `dua-core-6.1.0/src/macos/mod.rs` |
| **Bug found on the way:** jwalk also ran our folder filter on the scan root. A root named like a skip-list entry (`Library`, `node_modules`, `.git`, a hidden name) scanned as empty and reported "complete". | New test `a_root_named_like_a_skipped_folder_is_still_scanned` fails on the old code (0 files), passes on the new. |
| The progress UI added by the quick fix has gaps: a false red "No progress" warning during steps that send no events (Live Photo pairing, exiftool retries, places, saving), a stale ETA shown next to the warning, and "~0s left" on a rescan of a folder that grew. | Code review of `app.rs`, `dashboard.rs`, `meta/pass.rs` |

## Phase 1: Replace jwalk with dua-core (core only)

Already built and tested in a scratch copy on 2026-11-08: 247 tests pass, clippy and `cargo deny` clean.

### 1.1 Apply the patch

```sh
git apply --check all-specs/05-scan-walker-phase1.patch && git apply all-specs/05-scan-walker-phase1.patch
```

If `--check` fails, the files changed since the patch was made. Stop and tell the user (see rules).

### 1.2 What the patch changes (for review)

1. `Cargo.toml`: `jwalk = "0.9"` → `dua-core = "=6.1.0"`. `crates/yojaka-core/Cargo.toml`:
   `jwalk.workspace = true` → `dua-core.workspace = true`. Pin exactly: dua-core is young and changes its API often.
2. `crates/yojaka-core/src/cancel.rs`: new `CancelToken::flag(&self) -> &AtomicBool` for `Walk::next_cancellable`.
3. `crates/yojaka-core/src/scan/mod.rs`:
   - The `descend` closure replaces jwalk's `process_read_dir` filter. Bundle suffixes and Yojaka's own folders
     are always pruned. **At depth 0 (the root) it returns `true`** before the name rules, so the skip list
     and hidden rule apply only inside the chosen folder. (`Protected::check_root` already refuses
     `.app` / `.photoslibrary` roots before the scan starts.) Protected sub-folders are recorded as before.
   - `dua_core::walk(root, threads, Order::Completion, Options::default().skip_metadata(), descend)`.
     Walker metadata is skipped because `process()` does its own `lstat` (needed for `SF_DATALESS`).
   - Hidden files are skipped in the loop (`is_hidden(&e.file_name)`), because dua-core has no `skip_hidden`.
   - Loop: `while let Some(entry) = walker.next_cancellable(cancel.flag())`. Errors keep path and kind
     (`err.path()`, `err.io_error().kind()`).
   - Progress is **time-based** (every 100 ms from the loop, no longer only after a 512-file batch).
   - The estimated total (the last complete scan's `files_seen`) is sent only while `done` is below it
     (`expected.filter(|&e| e > done)`). On a rescan of a grown folder the UI goes back to "N found so far"
     and never shows "~0s left".
   - After the walk, one final `Progress { done, total: Some(done) }` is sent with `sink.important` (it
     cannot be dropped). The UI uses it to show "finishing up" (Phase 2).
   - The removed back-pressure stays removed. The comment points to ADR-0005.
4. `crates/yojaka-core/tests/scan.rs`: two tests.
   - `a_folder_bigger_than_any_buffer_scans_to_completion`: one 60k-file folder next to a tree of small JPEGs,
     a 60 s watchdog. This is a guard, not a reproduction: the old hang needed release-build timing, and
     no debug-build test reproduced it. Phase 1.4 has the real check.
   - `a_root_named_like_a_skipped_folder_is_still_scanned`: root `Library/` with `a.jpg` and
     `node_modules/b.jpg`, expects exactly 1 indexed file.

### 1.3 Build and check

```sh
source ~/.cargo/env
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check
grep -c 'name = "jwalk"' Cargo.lock    # must print 0
```

Expected: 247 tests pass (1 skipped). `cargo deny` may print "multiple versions" warnings; that is allowed.

### 1.4 Manual check on the folder that used to hang (release build)

`target/debug/deps` exists after any debug build, so the repo root still has the 59k-entry folder.
`alarm 90` kills a hung run instead of blocking forever.

```sh
cargo build --release -p yojaka-cli
for root in "$PWD" "$PWD/out/perf/tree"; do
  for i in 1 2 3; do
    H=$(mktemp -d)
    YOJAKA_HOME=$H /usr/bin/time -l perl -e 'alarm 90; exec @ARGV' ./target/release/yojaka scan "$root" --no-metadata 2>&1 \
      | grep -E "files|maximum resident|Alarm"
    rm -rf "$H"
  done
done
```

If `out/perf/tree` is missing: `cargo run --release -p fixturegen -- out/perf --files 100000 --videos 0.1` (about 35 s).

Acceptance (numbers measured 2026-11-08 on the M4 Pro):

| Root | Must | Measured with the patch |
|---|---|---|
| Repo root | every run finishes, no `Alarm` | 5.1 – 5.5 s, 24 – 25 MB, 191,249 files |
| `out/perf/tree` | ≤ 6 s, peak RSS ≤ 40 MB | 4.8 – 4.9 s, 24 MB, 103,923 files |

File counts must match the jwalk build (191,249 and 103,923 on that day; the repo count changes as
`target/` changes). A different count on `out/perf/tree` means a skip rule broke: stop and compare.

## Phase 2: Honest progress (core events + TUI)

Goal: no false "No progress" warnings in healthy runs, no stale ETA, and a clear "finishing up" state.
Rule after this phase: a step whose countable work is done (`done >= total`) is *finishing*, never *stalled*.

### 2.1 Core: send a final count when the countable work ends (`crates/yojaka-core/src/meta/pass.rs`)

1. After the main `for chunk in jobs.chunks(BATCH)` loop, before the exiftool retry block, add:

   ```rust
   if !cancelled && total > 0 {
       // Every file has been read. Exiftool retries, Live Photo pairing and places follow
       // without countable progress; the UI shows "finishing up" from here.
       sink.important(CoreEvent::Progress {
           task: TaskKind::Metadata,
           done: done.load(Ordering::Relaxed),
           total: Some(total),
           bytes: 0,
           current: None,
       });
   }
   ```

2. In `fn geocode`, report progress inside the loop and once at the end:
   - Before the loop: `let todo_len = todo.len() as u64; let mut last_emit = Instant::now();`
   - Change the loop to `for (i, (id, lat, lon)) in todo.into_iter().enumerate() {`, and put this at the
     **top** of the body, right after the cancel check (the body has a `continue`, so the end is not reached):

     ```rust
     if last_emit.elapsed() >= PROGRESS_EVERY {
         last_emit = Instant::now();
         sink.progress(CoreEvent::Progress {
             task: TaskKind::Geocode,
             done: i as u64,
             total: Some(todo_len),
             bytes: 0,
             current: None,
         });
     }
     ```
   - After the loop, before `TaskFinished { task: TaskKind::Geocode, .. }`:
     `if !cancel.is_cancelled() { sink.important(CoreEvent::Progress { task: TaskKind::Geocode, done: todo_len, total: Some(todo_len), bytes: 0, current: None }); }`

### 2.2 TUI state (`crates/yojaka-tui/src/app.rs`, `impl ScanView`)

1. New method:

   ```rust
   /// The countable work of this step is done. What follows (saving, pairing Live Photos,
   /// sharing places) reports no numbers, so silence is expected.
   pub fn finishing(&self) -> bool {
       self.total.is_some_and(|t| t > 0 && self.done >= t)
   }
   ```
2. `stalled_for()`: return `None` when `self.finishing()`.
3. `eta()`: return `None` when `self.finishing() || self.stalled_for().is_some()`.
4. `ratio()`: return `Some(1.0)` when `self.finishing()`, before the 99% cap for the Scan phase.
5. `status_line()` (header), inside `if s.running`, before the `match (s.ratio(), s.eta())`:

   ```rust
   if let Some(d) = s.stalled_for() {
       return format!("{what} · no progress for {}s", d.as_secs());
   }
   if s.finishing() {
       return format!("{what} · finishing up");
   }
   ```

### 2.3 Dashboard (`crates/yojaka-tui/src/screens/dashboard.rs`, `running_line`)

After the `if let Some(stuck) = s.stalled_for() { ... }` block, add:

```rust
if s.finishing() {
    let mut t = "Finishing up".to_string();
    if let Some(e) = s.elapsed() {
        t.push_str(&format!(" · {} elapsed", fmt::clock(e.as_millis() as u64)));
    }
    return Line::styled(fmt::truncate(&t, w), theme::dim());
}
```

### 2.4 Tests (in `app.rs` `mod tests`, next to `silence_is_reported`)

- `finishing_is_not_a_stall`: `begin_phase(TaskKind::Metadata, ..)`, `total = Some(10)`, `done = 10`,
  `last_event` older than `STALL_AFTER`. Expect `finishing()`, `stalled_for() == None`, `eta() == None`, `ratio() == Some(1.0)`.
- `a_stall_hides_the_eta`: Metadata, `total = Some(1000)`, `done = 100`, `rate = Some(100.0)`,
  `started` 10 s ago, `last_event` 6 s ago. Expect `eta() == None` and `stalled_for().is_some()`.
- `unknown_size_has_no_eta`: Scan phase, `total = None`. Expect `ratio() == None` and `eta() == None`.
- Update the existing `finding_files_estimate_never_shows_a_finished_bar`. It sets `done == total` and
  expects 0.99, but `done == total` now means finishing (ratio 1.0). Change it to `total = Some(1000)`,
  `done = 995` and keep `ratio() == Some(0.99)`: the cap still applies while the walk runs. This is the only
  existing assertion this phase may change.
- Existing snapshot tests may change text. Run with `INSTA_UPDATE=always`, then **read every changed
  `.snap` file** and check that the new text is right before keeping it.

### 2.5 Check

Full check. Then, optionally, drive the TUI on `out/perf/tree` with a throw-away `YOJAKA_HOME` (see
`docs/STATUS.md`: a python `pty` + `pyte` script can send keys and print the screen). A healthy run must
never show the red "No progress" line, and must show "Finishing up" between the steps.

## Phase 3: Live statistics refresh that adapts to query cost (`app.rs`)

Today the dashboard asks for statistics every 12 ticks (3 s) during a scan. On a 1M-row index one query
takes seconds, so the stats thread never rests. Make the gap at least 4× the last query time.
Use **ticks** for the gap, not wall-clock time, so unit tests stay deterministic.

1. `DashboardState` (derives `Default`): add
   ```rust
   /// When the running statistics request was sent.
   stats_asked: Option<Instant>,
   /// How long the last statistics request took to answer.
   stats_cost: Duration,
   /// The tick at which the last statistics answer arrived.
   stats_answered_tick: Option<u64>,
   ```
2. `request_stats()`: after `self.dashboard.stats_pending = true;` add `self.dashboard.stats_asked = Some(Instant::now());`
3. `on_response`, `Response::Stats(st)`: add
   ```rust
   if let Some(t) = self.dashboard.stats_asked.take() {
       self.dashboard.stats_cost = t.elapsed();
   }
   self.dashboard.stats_answered_tick = Some(self.ticks);
   ```
4. The live refresh in the tick handler becomes
   `if self.ticks.is_multiple_of(LIVE_STATS_TICKS) && self.live_stats_due() { return self.request_stats(); }` with
   ```rust
   /// Live refreshes wait at least 4× the last query time (and never less than LIVE_STATS_TICKS).
   fn live_stats_due(&self) -> bool {
       let gap = ((self.dashboard.stats_cost.as_millis() as u64 * 4) / 250).max(LIVE_STATS_TICKS);
       self.dashboard
           .stats_answered_tick
           .is_none_or(|t| self.ticks.saturating_sub(t) >= gap)
   }
   ```
   (250 ms is `TICK` in `event_loop.rs`.)
5. Test `slow_statistics_are_asked_for_less_often`: start a scan (`Msg::Rescan`), then set
   `stats_cost = Duration::from_secs(2)` (gap = 32 ticks), `stats_answered_tick = Some(a.ticks)` and
   `stats_pending = false`. Record `t0 = a.ticks`, then send ticks one at a time and note the tick of the
   first `Cmd::Worker(Request::Stats)`. Expect none while `a.ticks - t0 < 32`, and the first one by
   `t0 + 44` (requests only go out on ticks that are multiples of 12). The existing
   `live_statistics_refresh_while_scanning_but_never_pile_up` must still pass unchanged.
6. Full check.

Optional, only if the Phase 4 measurement shows statistics over 1.5 s at 1M rows: drop `full_path` from
`temp.uniq` in `crates/yojaka-core/src/stats.rs` and join back to `file` by `id` in the two queries that
print paths (the longest videos and the largest files).

## Phase 4: Docs and measurement

### 4.1 New ADR `docs/adr/0005-directory-walker.md` (same format as ADR-0004)

```markdown
# ADR-0005: Directory walker

Status: accepted (fix after Iteration 4)

## Context
The scan used `jwalk` with a back-pressure loop that slept inside jwalk's worker callback once 50,000
entries were in flight. jwalk yields folders in a fixed order. A sleeping worker could hold the folder
the consumer needed next, and the scan froze for good. A single folder with more entries than the cap
(a camera dump, `target/debug/deps`) triggered it every time. Removing the loop fixed the hang, but
memory then grew with the tree (about 270 MB at 1M entries, over the 150 MB I1 limit). jwalk itself
is unmaintained: its repo was archived on 2026-08-05, and 0.9.0 says "Use dua-core instead".

## Decision
- Walk with `dua-core`, pinned to an exact version, in `Order::Completion` (unordered).
- Its result queue is bounded and workers wait when it is full. This is safe because the consumer takes
  any entry; nothing waits for one particular folder.
- Folder rules live in the `descend` callback. Bundles and Yojaka's own folders are always pruned. The
  skip list and the hidden rule apply below the root only, never to the folder the user chose.
- Walker metadata is skipped (`Options::skip_metadata`). Yojaka keeps its own `lstat` per file because
  dua-core exposes no file flags, ctime or birth time, and the dataless check (`SF_DATALESS`) is a safety rule.
- Never add waiting inside the walker, and never use an ordered walk with any waiting.

## Consequences
- Memory stays flat: about 24 MB for a 104k-file and a 191k-file first scan (was 52 / 74 MB).
- The scan is about 5% slower on small files (4.8 s versus 4.6 s on 104k files); the walk itself is
  about 2% of scan time.
- Walk order varies from run to run. Nothing may depend on it.
- dua-core is young and changes often: upgrade on purpose and re-run the walker tests.
- dua-core reports APFS clone IDs (`Metadata::clone_id`). The duplicates feature could use them later:
  deleting a clone frees no space.
```

Fill in the 1M numbers from 4.3 under Consequences.

### 4.2 Spec text

- `all-specs/02-tech-spec.md` line 15: `| Directory walk | \`dua-core\` (parallel, unordered, bounded queue) | Bulk \`getattrlistbulk\` on APFS, flat memory, cancellable; see ADR-0005 |`
- Same file, pipeline diagram (around line 86): `jwalk ─►` → `dua-core ─►`.
- Same file, §3.1 (around line 100): "The walker runs on `jwalk`'s own rayon pool." → "The walker runs on
  `dua-core`'s own worker threads; its result queue is bounded (ADR-0005)."
- Same file, §6.1 step 1 (around line 227): `dua_core::walk(root, threads, Order::Completion, Options::default().skip_metadata(), descend)`. `descend` prunes bundles, own folders, skip-list and hidden names (the last two below the root only); symlinks are never followed.
- `all-specs/03-iteration-plan.md` line 73: "Parallel walker (`jwalk`)" → "Parallel walker (`dua-core`, ADR-0005)".
- Leave `all-specs/04-boilerplate-impl-plan.md` unchanged: it is the historical I0 plan.

### 4.3 Measure 1M entries for `docs/perf.md`

This takes about 5 minutes and about 3 GB of disk. **Delete the tree afterwards.**

```sh
cargo build --release -p yojaka-cli -p fixturegen
./target/release/fixturegen out/million --files 550000 --noise 400000
H=$(mktemp -d)
YOJAKA_HOME=$H /usr/bin/time -l ./target/release/yojaka scan out/million/tree --no-metadata 2>&1 | grep -E "files|real|maximum resident"
YOJAKA_HOME=$H /usr/bin/time -l ./target/release/yojaka scan out/million/tree --no-metadata 2>&1 | grep -E "files|real|maximum resident"   # rescan
rm -rf "$H" out/million
```

Update the `docs/perf.md` rows "Scan 600k media + 400k other (1M entries)", "Rescan unchanged, 1M-entry tree",
"Peak RSS, 1M-entry tree" and "Scan 115k media files". Replace the line "Before walker back-pressure was
added, the 1M-entry scan peaked at 271 MB." with: "2026-11: the walker is `dua-core` (ADR-0005). jwalk without
back-pressure peaked at 271 MB on 1M entries; jwalk with back-pressure could deadlock." **Peak RSS must be
under 150 MB**; if not, stop and tell the user.

### 4.4 `CHANGELOG.md`, under `## [Unreleased]`

- `### Fixed`
  - "A scan could freeze for good when one folder held more than 50,000 entries (a camera dump, or
    `target/debug/deps` in a Rust project). The directory walker is now `dua-core`; memory stays flat."
  - "A folder whose own name is on the skip list (`Library`, `node_modules`, `.git`, hidden names) was scanned
    as empty. The skip list now applies only inside the opened folder."
- `### Changed`
  - "Scan progress shows two steps, a percentage and time left when the size is known, 'Finishing up' between
    steps, and a red notice when nothing has moved for 5 s. Live statistics refresh less often when they are slow."

### 4.5 `docs/STATUS.md`

Add a short "Scan walker (after I4)" note: dua-core replaces jwalk (ADR-0005), the root skip-list fix, the
progress states, the new test count and the 1M numbers. Remove any open item these phases closed.

## Final checklist

- [ ] Full check passes (fmt, clippy `-D warnings`, nextest, deny).
- [ ] `grep -c 'name = "jwalk"' Cargo.lock` prints 0; no `jwalk` left in `crates/` (`grep -rn jwalk crates`).
- [ ] Phase 1.4: repo root finishes 3 of 3 runs; `out/perf/tree` peak RSS ≤ 40 MB; file counts unchanged.
- [ ] 1M-entry first scan: peak RSS under 150 MB, numbers in `docs/perf.md`, `out/million` deleted.
- [ ] Healthy TUI run: no red "No progress" line; "Finishing up" shows between steps.
- [ ] ADR-0005, spec lines, CHANGELOG and STATUS updated.
- [ ] Nothing committed unless the user asked.
- [ ] Delete `all-specs/05-scan-walker-phase1.patch` once Phase 1 is in (the git history keeps the change).

## Appendix: measurements from 2026-11-08 (M4 Pro, 12 cores, release, `--no-metadata`)

| Build | Repo root (191k files) | `out/perf/tree` (104k files) |
|---|---|---|
| Old: jwalk + 50k cap | **hung 5 of 5** | 4.5 s, 39 MB |
| Quick fix: jwalk, no cap | 4.7 s, 74 MB | 4.5 – 4.7 s, 52 MB |
| This plan: dua-core | 5.1 – 5.5 s, 24 – 25 MB | 4.8 – 4.9 s, 24 MB (with metadata: 7.8 s, 94 MB) |

Walk only (no per-file work), 10 threads: both walkers take about 0.12 s for 191k files. With a consumer
that sleeps 1 ms per 256 entries: jwalk 29 MB (104k) → 48 MB (191k); dua-core 4 MB on both. A bigger dua-core
queue (`queued_batches_per_worker(16)`) made no difference, so the default (2) stays.
