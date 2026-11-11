# 06: Iteration 8 plan (hardening, performance, release v1.0)

Written 2026-11-11, after I0-I7 were signed off. The goal and acceptance come from `03-iteration-plan.md`
§ Iteration 8. This file breaks them into phases. Do one phase at a time; every phase ends with the full check.

**Acceptance (from the iteration plan):**
- Every functional requirement FR-1..62 is checked off.
- Every performance target is met or explained.
- The safety fault-injection suite passes.

## 0. Ground rules

- `source ~/.cargo/env` before any `cargo` command.
- Full check: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check`.
- Tests and measurements run only on `fixturegen` trees in `out/` (never `/private/tmp`, which is protected),
  with a throw-away `YOJAKA_HOME`. Delete large `out/` trees afterwards.
- **Real libraries (Phase 5) are run by the user, or by Claude only with the user's go-ahead for that folder,
  read-only.** Write operations on real data run only on a copy or a scratch folder.
- Before v1.0 the schema can still change by editing the original migrations (no fix-up migrations). **From
  v1.0 on this stops:** any later schema change needs a real migration. So Phase 2 and Phase 4 must finish
  their schema changes before the tag.
- Commit only when the user asks. If the code does not match what this plan says, stop and tell the user.

## 1. Decisions

All answered by the user on 2026-11-11.

| # | Question | Decision |
|---|---|---|
| D1 | Tag the past iterations `v0.1.0` .. `v0.7.0` on their commits? | **No** (2026-11-11). Only `v1.0.0-rc.N` and `v1.0.0` get tags. |
| D2 | Typed phrase threshold. ADR-0006/0007: trash and move at 50 files, copy at 1,000, permanent delete always. FR-52 still says 1,000 for everything, and `config.organize.large_plan_threshold` is never read. | **Keep the code as it is. The user will tidy up the spec and the config key later.** Not part of I8. |
| D3 | FR-41 "index-only" organize operation. | **Dropped.** Organize is copy or move only. FR-41 in the product spec and STATUS updated 2026-11-11. |
| D4 | FR-62: which choices are remembered, and when? | **As recommended:** template, preset, copy/move, keep rules and extra skip folders, saved only when the user asks (a "remember" key in the TUI, `--remember` in the CLI). |
| D5 | Scan speed: optimise or explain? | **Optimise.** Plan: `07-scan-speed-plan.md`. |
| D6 | Apple Developer ID for codesigning and notarization? | **None for now.** Ship ad-hoc signed (as the linker already does). The tarball needs `xattr -d com.apple.quarantine`; the Homebrew install does not. Signing is a later step. |
| D7 | Is the GitHub repo public? | **Already public.** So the privacy re-check moves to Phase 0. |
| D8 | Minimum macOS version | **The last 4 major versions.** This Mac runs 27.2, so 14, 15, 26 and 27 are supported and **the minimum is macOS 14 (Sonoma)**. Re-check at each release. |
| D9 | Homebrew tap? | **Yes.** Plan: `08-homebrew-tap-plan.md`. |

## Phase 0: Housekeeping (½ day)

1. Commit the open edits to `03-iteration-plan.md` and `docs/STATUS.md` (I7 sign-off) when the user asks.
2. No tags for past iterations (D1). Remove "Tag `v0.1.0` .. `v0.7.0`" from STATUS open items.
3. Add a `### Iteration 8` heading to `CHANGELOG.md` under `[Unreleased]`.
4. Set `repository = "https://github.com/nithincvpoyyil/Yojaka"` in the workspace `Cargo.toml` (`""` today).
5. **Privacy re-check now** (the repo is already public, D7): look through every commit (`git log -p --all`) for
   media, screenshots, databases, keys, tokens, real home paths (`/Users/`), email addresses and private place
   names. The last check was 2026-11-08 (STATUS). Report anything found to the user. Do not rewrite history
   without asking.

## Phase 1: FR audit and gaps (2 days)

### 1.1 Write `docs/fr-checklist.md`

One row per FR-1..62 (plus 14a, 20a, 20b): requirement, where it is built (file), which test covers it, state
(done / gap / changed by decision). This is the acceptance proof for "every FR checked off". Fill it by
reading the code and tests, not from memory. Every "changed by decision" row links the ADR or decision.

### 1.2 Gaps found while writing this plan (verify, then fix)

| FR | Gap | Fix |
|---|---|---|
| FR-22 | `export` writes every file; no "filtered view" (`export.rs:147`, no filter option). | `yojaka export --kind photo\|video --year Y --camera C --place P --format F` (same filters as the browser); TUI: `x` in the File browser exports the current filter to a new file in the folder the user names. Never overwrite. |
| FR-31 | Keep rules `largest-file` and `has-exif` do not exist (`dupes/keep.rs:46-56`). | Add both, with unit tests and the error-message list. |
| FR-52 | Spec text and an unused config key disagree with ADR-0006/0007. | **Not in I8 (D2):** the user will do it later. Mark the checklist row "done in code, spec text pending". |
| FR-41 | Index-only dropped (D3). | Done: spec says copy or move. Checklist row only. |
| FR-51 | Before tree next to after tree. Dedupe plans have "files removed, by folder"; organize has a destination overview. Check whether "before" is shown for organize. | If missing: `plan show --full-tree` prints source folders (before) and destination folders (after). |
| FR-14 | Skip list is configurable, but `Library` is in the default skip list while the spec lists `Library/`. Fine; just record it. | Checklist row only. |
| — | The CLI `history` shows a different hour from the History screen for the same run (STATUS open item). Probably UTC against local time. | Find the cause, use local time in both, add a test with a fixed offset. |
| — | No TUI key saves a plan file (only `yojaka dupes --plan-out`). FR-50 says plans can be saved. | `s` in the Review box saves the plan to a file name the user types (never overwrite). |

Any further gap found in 1.1 goes into this table before it is fixed.

### 1.3 Tests

Unit tests for the new rules and filters; CLI snapshot for `export --help` and a filtered export; TUI
key-sequence test for the plan save and the export key.

## Phase 2: Saved preferences, FR-62 (1-2 days, after D4)

Today `config.dupes.keep_rules` is never read: the CLI and the TUI use `DEFAULT_KEEP`
(`dupes/keep.rs:12`), and the config default is `["oldest", "shortest-path"]`, which does not match it.

1. Make `Config` the single source of defaults: `dupes.keep_rules` defaults to `DEFAULT_KEEP`'s rules,
   `yojaka dupes` uses it when `--keep` is absent, and the TUI's starting rule preset comes from it.
   `organize.template` already feeds the CLI; check the TUI too.
2. Add `organize.mode` (`copy` | `move`) and `organize.preset`.
3. Remembering (per D4): TUI key `M` ("remember these choices") on the Duplicates and Organize screens writes
   the current keep rule / template / mode with `Config::save` (atomic already). CLI `--remember` on `dupes`
   and `organize`. A footer message says what was saved and where.
4. Never write the config without the user asking. A bad saved value (unknown rule, bad template) falls back to
   the default with a warning; it must never stop the app from opening.
5. Tests: round trip of each remembered field; a broken saved template opens with the default and a warning;
   config snapshot updated.

## Phase 3: Safety fault-injection suite (2-3 days)

What exists: in-process crash simulation (`exec.rs:324` `a_crash_mid_apply_resumes...`), resume, cancel,
changed-file skips, proptests for plan invariants (`tests/dupes.rs`, `organize/template.rs`), and an ignored
disk-image cross-volume test (`organize_exec.rs:404`). Missing from tech spec §11:

1. **Real SIGKILL.** A test spawns the real binary (`assert_cmd` path) on a fixture: `yojaka --write apply
   plan.json --yes --trash-dir ...`, waits until the journal has N "done" lines (poll the `.jsonl` file),
   then `kill -9`. Then `apply --resume` must finish, and `undo` must give back a tree whose hash equals the
   original. Run it for trash, copy, move and cross-folder move plans. To make the kill land mid-run without
   sleeps in the product, add a hidden test-only env var (`YOJAKA_TEST_OP_DELAY_MS`) read only in debug
   builds (`cfg(debug_assertions)`), or use enough files that the run takes over a second.
2. **Kill during undo.** Same, killing `undo`; a second `undo` finishes the job.
3. **Read-only destination.** Organize into a folder with mode `0o555`: nothing is half-done; the plan
   reports every file as failed; the exit code is 3; the source is untouched.
4. **Disk full.** A 50 MB APFS image (`hdiutil create -size 50m -fs APFS`), organize-copy 80 MB into it: the
   free-space check refuses before anything runs. Then a plan that fits at check time but not at run time
   (fill the disk between plan and apply): each failed copy is cleaned up, no partial file is left, the source
   is untouched, the journal says what happened.
5. **Journal cannot be written.** Make the journal folder read-only before `apply`: nothing runs (fatal error,
   tech spec §12).
6. **Changed under our feet** (already covered: size, mtime, same-size edit). Add: a file replaced by a
   symlink between plan and apply is skipped.
7. **Live Photo pair invariant** under every fault above: a pair is never split (assert on the final tree).

Disk-image tests stay `#[ignore]` locally but get their own CI job (`cargo nextest run --run-ignored only -E
'test(/disk_image/)'`), since GitHub macOS runners allow `hdiutil`. Group the suite as
`crates/yojaka-core/tests/fault_injection.rs` plus the CLI kill tests in `crates/yojaka-cli/tests/`.

## Phase 4: Performance (3 days, plus 4-6 days for the scan speed work)

**Scan speed (D5) has its own plan: `07-scan-speed-plan.md`** (stages S0-S6, goals: 1M-entry first scan
51.9 s → ≤ 15 s, 104k scan + metadata 7.5 s → ≤ 4 s). This phase covers everything else measured in §10.
Do the 07 plan first, because it changes the numbers below.

### 4.1 Tools

- `brew install hyperfine samply` (samply instead of `cargo flamegraph`: flamegraph needs `dtrace` as root
  on macOS; samply does not). Instruments (Time Profiler, Allocations) for memory.
- Build with the existing `profiling` profile (`--profile profiling`, symbols kept).

### 4.2 Measure every tech spec §10 target

Fill a new "Targets" table at the top of `docs/perf.md`: target, measured, met / explained, how measured.

| Target | Today | Work |
|---|---|---|
| Walk only, 1M entries < 10 s | Not measured alone (1M first scan 51.9 s) | `07` stage S1. |
| Scan + metadata 100k < 30 s | 7.5 s | `07` (goal ≤ 4 s). |
| Rescan 100k < 5 s | 0.5 s + 0.9 s pairing | `07` stage S4. |
| Exact-dupe pass, 100k / 300 GB, 10% dupes: disk-bound | Only 20k small files measured | `fixturegen --big` (or a flag for file size) on an external SSD, a few hundred GB if space allows, else scale down and explain. Compare read MB/s with `dd` on the same disk. |
| Peak RSS < 250 MB on a 1M index | 25 MB scan; **similar tier peaked at 500 MB** (full-size JPEG decode, STATUS open item) | Decode JPEG/PNG/etc. through ImageIO with a max size (like HEIC), or `image`'s thumbnail path; bump `LOOK_VERSION`; cap decode concurrency by memory. Re-check `dupes_accuracy`. |
| TUI frame < 16 ms on 1M rows | 0.2-4.3 ms | Re-measure. |

Also measure, not targets: statistics at 1M (4.0 s), text filter at 1M (1.5 s), similar tier over 100k
pictures, the BK-tree at 1M pictures (STATUS: not measured), Organize planning at 1M.

### 4.3 Tune

Profile the slowest three, then tune thread counts (performance against efficiency cores; `threads = 0`
should mean performance cores), SQLite batch sizes, channel bounds and hash read caps. Record each change
with before/after numbers. Any target still missed gets a written reason in `docs/perf.md` (acceptance says
"met or explained").

### 4.4 CI

The benchmark smoke test exists (`cargo bench --no-run`). Add a 10k-file scan timed in CI with a generous
limit, so a large regression fails the build.

## Phase 5: Real-world testing (the user's libraries; 2 days of the user's time)

Each run: release build, a throw-away `YOJAKA_HOME`, read-only first (`scan`, `stats`, `dupes --tier all
--similar`, `organize --plan-out`, `plan show`). Write steps only on a copy or a scratch destination.

| # | Library | What to check |
|---|---|---|
| R1 | iPhone export with Live Photos and 4K HEVC videos | Pairing count matches Photos; HEIC/HEVC metadata; video read under 1 MB each (`--debug` log); similar tier on real HEIC (STATUS: never measured); Display P3 against sRGB copies |
| R2 | External exFAT drive and an HFS+ drive | Walk, hard links (none on exFAT), mtime precision (exFAT has 2 s / 10 ms), cross-volume organize copy and undo to a scratch folder |
| R3 | iCloud Drive folder with "optimize storage" on | Dataless files listed as cloud-only and **never downloaded** (watch Activity Monitor / `brctl` network; check `SF_DATALESS` before and after) |
| R4 | More than 1M files (a whole volume or a big backup) | Times and peak RSS against §10; TUI stays smooth; cancel and resume |
| R5 | Odd Unicode names | NFD from HFS+ against NFC typed names, emoji, RTL, zero-width, very long paths (> 1024 bytes), names with newlines; shown, searched, exported and organized right. Also build a fixturegen tree with these so it becomes an automated test |

Write findings to `docs/real-world-tests.md` with no private paths, names or places (counts and timings only).
Bugs found become tests on synthetic fixtures. If the user agrees, add the missing real samples (iPhone Live
Photo pair, HEVC MOV, Android MP4, GPS photo; under 1 MB each) to `tests/data/` (gitignored) for local tests.

Also decide the open question in STATUS: is a scan root that is itself a symlink refused? (Recommended: resolve
it and scan the target, as the protected-path check already resolves it, and say so in the header.)

## Phase 6: Release engineering (2-3 days)

1. **Version.** Workspace version `1.0.0`; `--version` long form shows the commit; CHANGELOG `[1.0.0]`.
2. **Build.** `MACOSX_DEPLOYMENT_TARGET = "14.0"` (D8) in `.cargo/config.toml`; check the binary with
   `otool -l` (`LC_BUILD_VERSION minos 14.0`; it says `11.0` today). Add `macos-14` to CI next to
   `macos-latest`. The release profile already has LTO, `codegen-units = 1`, `strip`, `panic = "abort"`.
   Check that the panic hook still restores the terminal with `panic = "abort"` (a test that panics in a
   child TUI process under a pty).
3. **Signing: not for v1.0** (D6). The binary stays ad-hoc signed by the linker (`codesign -dv`:
   `adhoc,linker-signed`). The release notes and README say why a downloaded tarball needs
   `xattr -d com.apple.quarantine yojaka`. Developer ID signing and notarization stay a later step.
4. **Release workflow** `.github/workflows/release.yml`, on a `v*` tag: build on `macos-14`, check `minos`
   and `--version`, `yojaka-<ver>-aarch64-apple-darwin.tar.gz` + `.sha256`, GitHub Release with the CHANGELOG
   section as notes. Dry-run on a `v1.0.0-rc.1` tag first. Details: `08-homebrew-tap-plan.md` §4.
5. **Homebrew tap** (D9): **plan `08-homebrew-tap-plan.md`** (tap repo `nithincvpoyyil/homebrew-yojaka`, a
   binary formula, a test on a clean account, manual bump script for v1.0, automatic later).
6. **README for users.** What it is, the safety promise, install (brew / tarball), first run, the five
   screens, the main CLI flows (dupes → plan → apply → undo; organize), Full Disk Access note, privacy (offline,
   no telemetry, GPS left out of exports), license and GeoNames credit. GIF demos recorded with `vhs`
   (`brew install vhs`, scripts in `docs/demos/*.tape`) **on fixture trees only**, never real photos.
7. **Privacy check again just before the tag** (the first check is in Phase 0): the same search over the
   new commits, and check that the GIFs show fixture paths only.
8. **Docs.** Fill the missing demo scripts `docs/demos/iter-4.md`..`iter-6.md` and `iter-8.md`; update the
   tech spec §10 (final targets), §15 (distribution as built), product spec §6 (Q5: macOS 14, the last 4
   majors); ADR-0010 for the release and Homebrew setup (ADR-0009 is the scan pipeline, plan 07).

## Phase 7: Acceptance and sign-off

- [ ] `docs/fr-checklist.md`: every FR done or changed by a recorded decision.
- [ ] `docs/perf.md`: every §10 target met or explained, with numbers from the v1.0 build.
- [ ] Fault-injection suite passes locally and in CI (including the disk-image job).
- [ ] Real-world runs R1-R5 recorded in `docs/real-world-tests.md`.
- [ ] Full check clean; release binary under 100 MB (9.2 MB at I7).
- [ ] Scan speed goals from `07-scan-speed-plan.md` met or explained.
- [ ] `v1.0.0-rc.1` released through the workflow, installed with brew on a clean account, Gatekeeper does
      not block it (`08-homebrew-tap-plan.md` "Done when").
- [ ] `docs/STATUS.md`, `CHANGELOG.md`, iteration plan status updated. The user signs off, then tag `v1.0.0`.

## Order and size

Phase 0 → 1 → 2 → 3 → 4 (07 first) → 5 → 6 (08) → 7. Phases 3 and 4 can swap. Phase 5 needs the user and
can start any time after Phase 1 (findings may add work to Phases 3-4). Rough size: about 3½ weeks with the
scan speed work, so much larger than the plan's "M"; the extra comes from the FR gaps, the fault-injection
suite and plan 07.

## Not in I8

Inline thumbnails, the detailed dashboard, place-to-browser filter, more themes, mouse support, similar
videos (all in `TODO.md` or later iterations). They stay open after v1.0 unless the user moves them in.
