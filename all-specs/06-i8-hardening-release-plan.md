# 06: Iteration 8 plan (hardening)

Written 2026-11-11, after I0-I7 were signed off. The goal and acceptance come from `03-iteration-plan.md`
§ Iteration 8. This file breaks them into phases. Do one phase at a time; every phase ends with the full check.

> **Split on 2026-11-12:** Iteration 8 is Phases 0-3 below (done). Phases 4-7 (performance, real-world tests,
> release engineering, sign-off) moved to Iteration 9: `09-i9-features-release-plan.md`. The decisions in §1
> still apply there.

**Acceptance (Iteration 8 after the split):**
- Every functional requirement FR-1..62 is checked off (`docs/fr-checklist.md`).
- The safety fault-injection suite passes.
- (Performance targets: moved to Iteration 9.)

## 0. Ground rules

- `source ~/.cargo/env` before any `cargo` command.
- Full check: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check`.
- Tests and measurements run only on `fixturegen` trees in `out/` (never `/private/tmp`, which is protected),
  with a throw-away `YOJAKA_HOME`. Delete large `out/` trees afterwards.
- **Real libraries (now Iteration 9, Phase 2) are run by the user, or by Claude only with the user's go-ahead for that folder,
  read-only.** Write operations on real data run only on a copy or a scratch folder.
- Before v1.0 the schema can still change by editing the original migrations (no fix-up migrations). **From
  v1.0 on this stops:** any later schema change needs a real migration. So every schema change (Phase 2 here,
  Iteration 9) must be done before the tag.
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
   then `kill -9`. Then `apply --resume` must finish with nothing lost. Run it for trash, copy, move and cross-folder move plans. To make the kill land mid-run without
   sleeps in the product, add a hidden test-only env var (`YOJAKA_TEST_OP_DELAY_MS`) read only in debug
   builds (`cfg(debug_assertions)`), or use enough files that the run takes over a second.
2. ~~**Kill during undo.**~~ Dropped with undo (ADR-0009).
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
'test(/disk_image/)'`), since GitHub macOS runners allow `hdiutil`. The suite lives in
`crates/yojaka-cli/tests/fault_kill.rs` (real kills, read-only destination, disk image) and `crates/yojaka-core/tests/exec.rs` /
`organize_exec.rs` (in-process faults). **Built 2026-11-11.** It found one bug (undo before resume could split a Live Photo), fixed the same day; undo was removed on 2026-11-12 (ADR-0009).

## Phases 4-7: moved to Iteration 9

On 2026-11-12 the user split this plan. Iteration 8 is Phases 0-3 above (done). Performance, real-world
testing, release engineering and sign-off moved to **Iteration 9, `09-i9-features-release-plan.md`**, with
a feature phase (the detailed dashboard) in front. LLM assist is now Iteration 10, and GUI is Iteration 11.

## Status

**Done 2026-11-11.**

| Phase | Commit |
|---|---|
| Phase 0 | `b93a80b` |
| Phase 1 | `0104cbe` |
| Phase 2 | `eb16c83` |
| Phase 3 | fault-injection suite (the undo fix is moot since ADR-0009) |

Acceptance for Iteration 8:
- `docs/fr-checklist.md` is complete: every FR is done or changed by a recorded decision; FR-52's spec text
  is left to the user (D2).
- The fault-injection suite passes, including the disk-image tests.
- The performance targets moved to Iteration 9 with the rest.
