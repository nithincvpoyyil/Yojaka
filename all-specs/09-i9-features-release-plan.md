# 09: Iteration 9 plan (features and release v1.0)

Written 2026-11-12. On that day the user split the old Iteration 8. Phases 0-3 (FR audit, saved preferences,
fault injection) stayed in Iteration 8 (`06-i8-hardening-release-plan.md`, done). The rest moved here, with a
feature phase added in front. LLM assist is now Iteration 10, and GUI is Iteration 11.

Do one phase at a time. Every phase ends with the full check.

**Acceptance:**
- The detailed dashboard is built and signed off by the user.
- Real-world runs R1-R5 are recorded in `docs/real-world-tests.md`.
- Every performance target in tech spec §10 is met or explained in `docs/perf.md`.
- `docs/fr-checklist.md` still holds, and the fault-injection suite still passes.
- `v1.0.0-rc.1` was released through the workflow and installed with brew on a clean account. After the user
  signs off, `v1.0.0` is tagged.

## 0. Ground rules

- `source ~/.cargo/env` before any `cargo` command.
- Full check: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo deny check`.
- Tests and measurements run only on `fixturegen` trees in `out/` (never `/private/tmp`, which is protected),
  with a throw-away `YOJAKA_HOME`. Delete large `out/` trees afterwards.
- **The user runs real libraries (Phase 2).** Claude runs them only with the user's go-ahead for that folder,
  and read-only. Write operations on real data run only on a copy or a scratch folder.
- Before v1.0 the schema can still change by editing the original migrations. **From v1.0 on, any schema change
  needs a real migration.** So every schema change (dashboard queries, Phase 4) must be done before the tag.
- Commit only when the user asks. If the code does not match what this plan says, stop and tell the user.
- The decisions D1-D9 in `06-i8-hardening-release-plan.md` §1 still apply:
  - no tags before `v1.0.0-rc.1`
  - no Apple Developer ID, so ad-hoc signing
  - the repo is public
  - minimum macOS 14
  - a Homebrew tap
  - scan speed is optimised, not just explained

## Phase 1: Detailed dashboard (feature; size once specified)

**Waiting for the user's description.** What exists today and what could be added is listed below, so the user
can say what to keep, add or drop.

- **Shown today:**
  - Scan panel
  - Library: photos and videos (count and size), Live Photos, errors
  - Formats
  - By year
  - Top cameras
  - Top places (cities, plus an "N countries · N without place" line)
- **Computed but not shown** (no new query):
  - months per year
  - total video length; longest and largest videos; codecs
  - largest files overall
  - undated files
  - all countries; files with GPS
  - cloud-only files
  - metadata waiting or failed
  - last scan time and duration
- **Not computed yet** (needs new queries):
  - duplicate wasted space (after a search)
  - lens breakdown
  - video resolution (for example 4K)
  - date range
  - day of the week

Steps once the user has described it:
1. A text mockup at 80×24 and 160×48 for the user to check before any code.
2. The `yojaka-core::stats` additions and their tests. Statistics run on their own thread (4 s at 1M rows), so
   new queries must keep the dashboard responsive.
3. Rendering, snapshots at every size, and key tests for any interaction. If picking a place, year or camera
   should open the File browser filtered to those files, this also covers the parked "place → browser filter"
   item.
4. The user tries it in the real app and signs it off.

## Phase 2: Real-world testing (the user's libraries; 2 days of the user's time)

Each run: release build, a throw-away `YOJAKA_HOME`, read-only first (`scan`, `stats`, `dupes --tier all
--similar`, `organize --plan-out`, `plan show`). Write steps only on a copy or a scratch destination.

| # | Library | What to check |
|---|---|---|
| R1 | iPhone export with Live Photos and 4K HEVC videos | Pairing count matches Photos; HEIC/HEVC metadata; under 1 MB read per video (`--debug` log); similar tier on real HEIC (never measured); Display P3 against sRGB copies |
| R2 | External exFAT drive and an HFS+ drive | Walk; hard links (none on exFAT); mtime precision (exFAT has 2 s / 10 ms); cross-volume organize copy and undo to a scratch folder |
| R3 | iCloud Drive folder with "optimize storage" on | Dataless files listed as cloud-only and **never downloaded** (watch the network in Activity Monitor or with `brctl`; check `SF_DATALESS` before and after) |
| R4 | More than 1M files (a whole volume or a big backup) | Times and peak RSS against §10; TUI stays smooth; cancel and resume |
| R5 | Odd Unicode names | NFD from HFS+ against NFC typed names, emoji, RTL, zero-width, very long paths (over 1024 bytes), names with newlines; each must be shown, searched, exported and organized correctly. Also build a fixturegen tree with these so it becomes an automated test |

Write findings to `docs/real-world-tests.md` with no private paths, names or places (counts and timings only).
Bugs found become tests on synthetic fixtures. If the user agrees, add the missing real samples (iPhone Live
Photo pair, HEVC MOV, Android MP4, GPS photo; under 1 MB each) to `tests/data/` (gitignored) for local tests.

Also decide the open question in STATUS: is a scan root that is itself a symlink refused? (Recommended: resolve
it and scan the target, as the protected-path check already resolves it, and say so in the header.)

## Phase 3: Release engineering (2-3 days; no tag yet)

1. **Version.** Workspace version `1.0.0`; `--version` long form shows the commit; CHANGELOG `[1.0.0]`.
2. **Build.**
   - `MACOSX_DEPLOYMENT_TARGET = "14.0"` (D8) in `.cargo/config.toml`.
   - Check the binary with `otool -l`: `LC_BUILD_VERSION minos 14.0` (it says `11.0` today).
   - Add `macos-14` to CI next to `macos-latest`.
   - The release profile already has LTO, `codegen-units = 1`, `strip` and `panic = "abort"`. Check that the
     panic hook still restores the terminal with `panic = "abort"`: a test that panics in a child TUI process
     under a pty.
3. **Signing: not for v1.0** (D6). The binary stays ad-hoc signed by the linker (`codesign -dv`:
   `adhoc,linker-signed`). The release notes and README say why a downloaded tarball needs
   `xattr -d com.apple.quarantine yojaka`.
4. **Release workflow** `.github/workflows/release.yml`, run on a `v*` tag:
   - build on `macos-14`
   - check `minos` and `--version`
   - make `yojaka-<ver>-aarch64-apple-darwin.tar.gz` and its `.sha256`
   - publish a GitHub Release with the CHANGELOG section as the notes

   The first real run is the `v1.0.0-rc.1` tag in Phase 5. Details: `08-homebrew-tap-plan.md` §4.
5. **Homebrew tap** (D9): plan `08-homebrew-tap-plan.md`. It covers the tap repo
   `nithincvpoyyil/homebrew-yojaka`, a binary formula, a test on a clean account, and a manual bump script for
   v1.0 (automatic later).
6. **README for users.**
   - What it is, and the safety promise
   - Install (brew or tarball) and first run
   - The five screens
   - The main CLI flows: dupes → plan → apply → undo, and organize
   - A Full Disk Access note
   - Privacy: offline, no telemetry, GPS left out of exports
   - License and the GeoNames credit
   - GIF demos recorded with `vhs` (scripts in `docs/demos/*.tape`), **on fixture trees only**, never real photos
7. **Privacy check again just before the tag.** The first check was 06 Phase 0. Repeat the same search over the
   new commits, and check that the GIFs show fixture paths only.
8. **Docs.**
   - Fill the missing demo scripts `docs/demos/iter-4.md` to `iter-6.md`, plus `iter-8.md` and `iter-9.md`.
   - Update the tech spec §10 (final targets) and §15 (distribution as built), and product spec §6 (Q5: macOS
     14, the last 4 majors).
   - Write ADR-0010 for the release and Homebrew setup. ADR-0009 is the scan pipeline (plan 07).

## Phase 4: Performance (last before the release candidate; 3 days plus 4-6 days for scan speed)

The user moved this to the end on 2026-11-12, so features come first. It runs before the release candidate,
because the numbers and any schema change must be final before v1.0.

**Scan speed (D5) has its own plan: `07-scan-speed-plan.md`.** It has stages S0-S6. Its goals: a 1M-entry first
scan from 51.9 s to 15 s or less, and a 104k scan with metadata from 7.5 s to 4 s or less. This phase covers
everything else measured in §10. Do plan 07 first, because it changes the numbers below.

### 4.1 Tools

- `brew install hyperfine samply` (ask the user first). Use samply instead of `cargo flamegraph`, which needs
  `dtrace` as root on macOS. Use Instruments (Time Profiler, Allocations) for memory.
- Build with the existing `profiling` profile (`--profile profiling`, which keeps symbols).

### 4.2 Measure every tech spec §10 target

Fill a new "Targets" table at the top of `docs/perf.md`: target, measured, met or explained, how measured.

| Target | Today | Work |
|---|---|---|
| Walk only, 1M entries < 10 s | Not measured alone (1M first scan 51.9 s) | `07` stage S1 |
| Scan + metadata 100k < 30 s | 7.5 s | `07` (goal ≤ 4 s) |
| Rescan 100k < 5 s | 0.5 s + 0.9 s pairing | `07` stage S4 |
| Exact-dupe pass, 100k / 300 GB, 10% dupes: disk-bound | Only 20k small files measured | `fixturegen` with big files on an external SSD, a few hundred GB if space allows, else scaled down and explained. Compare read MB/s with `dd` on the same disk |
| Peak RSS < 250 MB on a 1M index | 25 MB for the scan; **similar tier peaked at 500 MB** (full-size JPEG decode) | Decode JPEG, PNG and the rest through ImageIO with a size limit (as for HEIC); bump `LOOK_VERSION`; cap decoding by memory. Re-check `dupes_accuracy` |
| TUI frame < 16 ms on 1M rows | 0.2-4.3 ms | Re-measure |

Also measure (not targets): statistics at 1M (4.0 s), text filter at 1M (1.5 s), the similar tier over 100k
pictures, the BK-tree at 1M pictures, Organize planning at 1M.

### 4.3 Tune

Profile the slowest three, then tune:
- thread counts: performance against efficiency cores (`threads = 0` should mean performance cores)
- SQLite batch sizes
- channel bounds
- hash read caps

Record each change with before and after numbers. Any target still missed gets a written reason in
`docs/perf.md`.

### 4.4 CI

Add a 10k-file scan timed in CI with a generous limit, so a large regression fails the build.

## Phase 5: Acceptance and sign-off

- [ ] The detailed dashboard is signed off by the user.
- [ ] `docs/fr-checklist.md`: every FR is done or changed by a recorded decision (FR-52 spec text: the user's,
      D2).
- [ ] `docs/perf.md`: every §10 target is met or explained, with numbers from the v1.0 build; scan speed goals
      from plan 07 met or explained.
- [ ] Fault-injection suite passes, locally and in CI (including the disk-image job).
- [ ] Real-world runs R1-R5 recorded in `docs/real-world-tests.md`.
- [ ] Full check clean; release binary under 100 MB (9.2 MB at I7).
- [ ] `v1.0.0-rc.1` released through the workflow and installed with brew on a clean account; Gatekeeper does
      not block it (`08-homebrew-tap-plan.md` "Done when").
- [ ] `docs/STATUS.md`, `CHANGELOG.md` and the iteration plan status updated. The user signs off, then tag
      `v1.0.0`.

## Order and size

Phase 1 (dashboard) → 2 (real libraries) → 3 (release engineering) → **4 (performance: 07 first)** → 5
(`v1.0.0-rc.1`, sign-off, `v1.0.0`).

Phase 2 needs the user and can start any time. Its findings may add work to Phase 4, or new fault tests.

Rough size: 2½-3 weeks plus the dashboard, so **L**.

## Not in I9

These stay open after v1.0 unless the user moves them in (all are in `TODO.md`):
- inline thumbnails on the Duplicates screen
- more themes
- mouse support
- similar videos
- the place → browser filter, unless the dashboard covers it
