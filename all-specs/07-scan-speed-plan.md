# 07: Scan speed plan (Iteration 9, Phase 4)

> **Scheduled last in Iteration 9** (user, 2026-11-12): after the dashboard, the real-library tests and the release
> engineering, before the release candidate (`09-i9-features-release-plan.md`, Phase 4). Nothing here has started.

Written 2026-11-11. The user asked for the scan to be made faster (decision D5 in
`06-i8-hardening-release-plan.md`). Measure first, then make one change at a time, each with before/after numbers.

## 1. Where we are

| Measurement (M4 Pro, warm cache, fixturegen trees, `docs/perf.md`) | Time |
|---|---|
| 1M entries (600k media + 400k other), first scan, file pass only (I1, no metadata) | 51.9 s, 25 MB |
| 1M entries, rescan unchanged | 6.5 s |
| 104k media files, file pass | 4.8 s |
| 104k media files, file pass + metadata + places | 7.5 s (4.3 s file pass + 2.9 s metadata and places) |
| 104k rescan unchanged with metadata | 0.5 s file pass + 0.9 s Live Photo pairing |
| Walk alone | about 2% of scan time (ADR-0005), never measured on its own at 1M |

**Not linear:** 5.8× the media files (104k → 600k) took 10.8× the time (4.8 s → 51.9 s). The cost per file
grows with the size of the index, which points at the database rather than the disk.

### What the code does today (`crates/yojaka-core/src/scan/mod.rs`)

1. The main thread pulls entries from `dua_core::walk` and collects 512 file paths (`CHUNK`).
2. **Then it stops walking** and runs `flush` (lines 356-372): `pool.install` sends the 512 paths to the
   worker pool in groups of 64 and **waits** for all of them. Walking and processing take turns instead of
   running at the same time.
3. Each group of 64 (`process`, line 129) **opens a new SQLite connection** and prepares the lookup
   statement. At 1M files that is about 15,000 connection opens.
4. For every file, even one that is not media: `lstat` (line 148), and only **then** the extension check
   (line 160). In the 1M tree, 400k `lstat` calls are for files that are thrown away.
5. For every media file: one lookup by `full_path` (line 171), **even on a first scan or `--force`**, when the
   index is empty. Then `detect` opens the file (`O_NOFOLLOW`) and reads 32 bytes (`detect.rs:138`).
6. One writer thread (`scan/writer.rs`) writes each batch in one transaction: an UPSERT per new file, an
   UPDATE per unchanged file. The `file` table has a unique index on `full_path` plus 5 more indexes
   (`0002_scan_file.sql:33-37`). The connection uses SQLite's default page cache (about 2 MB), while the
   indexes of a 1M-row table are hundreds of MB, and the paths arrive in no particular order.
7. Threads: `available_parallelism() - 2` (10 on the 12-core M4 Pro, which has 8 performance and 4 efficiency
   cores) for the workers **and** for the walker.

## 2. Goals

These are starting goals. Check them after S1 and agree any change with the user.

| Measurement | Today | Goal |
|---|---|---|
| Walk only, 1M entries (tech spec §10) | not measured | < 10 s (probably met already) |
| 1M entries, first scan, file pass | 51.9 s | **≤ 15 s** |
| 104k media files, scan + metadata + places | 7.5 s | **≤ 4 s** |
| 1M entries, rescan unchanged | 6.5 s | **≤ 3 s** |
| 104k rescan unchanged with metadata | 1.4 s | ≤ 0.7 s |
| Peak RSS, 1M entries | 25 MB | stays < 250 MB (G2) and flat with tree size |

## 3. Rules that must not change

- **The dataless check (`SF_DATALESS`) comes before any open** of a file. No change may open a file it has
  not checked.
- `O_NOFOLLOW` on every open. Symlinks are never followed.
- **ADR-0005:** never wait or sleep inside the walker's `descend` callback, and never switch to an ordered
  walk. Waiting in *our own* loop on a full channel is fine: with `Order::Completion` a slow consumer only
  slows the walker down.
- Every queue stays bounded, so memory stays flat.
- Per-file errors are collected, never fatal. Cancel keeps partial results and marks the scan `partial`.
  A cancelled scan never prunes.
- Same results. **S0 adds an equivalence test before any optimisation**, and every step must pass it.
- One change per commit, each with its numbers in the commit message and in `docs/perf.md`.

## Stage S0: Safety net (½ day)

1. **Equivalence test** (`crates/yojaka-core/tests/scan_equivalence.rs`): a fixturegen tree with
   duplicates, hard links, symlinks, corrupt files, unreadable folders, Live Photo pairs, renamed media, odd
   Unicode names and a folder with 60k entries. Scan it and dump the `file` table in a fixed order, leaving
   out ids and scan ids. Do the same for `scan_error`. Then compare with a dump saved by the current code.
   Repeat for a rescan after changes (add, delete, touch, replace a file with a symlink).
2. Check that the existing scan tests cover cancel in the middle, a 50k+ entry folder (ADR-0005), and a root
   named like a skipped folder. Add any that are missing.

## Stage S1: Measure (1 day)

1. **Timings per step** at `debug` level, ending with one summary line in the `--debug` log:
   - time the main thread waits on the walker
   - time spent in `flush`
   - total `lstat`, lookup and `detect` time, summed across workers
   - writer busy time and the time spent waiting for a free channel slot
   - commit time
   Use cheap `Instant` sums in atomics, not a span per file.
2. **Walk-only number:** a bench (`crates/yojaka-core/benches/walk.rs`, or an ignored test like
   `browse_perf`) that runs `dua_core::walk` with the same `descend` and only counts entries. Fill the §10 row.
3. **Scaling curve:** first scans of 100k, 250k, 500k and 1M media files (`fixturegen`, in `out/`), with
   `--no-metadata` and with metadata. This shows whether the extra cost grows with the index.
4. **Profiles:** `samply record target/profiling/yojaka scan --no-metadata out/million/tree` for the first scan
   and for a rescan. Use Instruments (System Trace) to see thread waits.
5. **Cold cache:** the user runs `sudo purge` before one run of each, because Claude cannot use `sudo`.
6. Write a "Scan breakdown" table in `docs/perf.md`. **Reorder S2-S5 by what the numbers show.** The order
   below is the expected one.

## Stage S2: Cheap fixes (1 day; measure after each)

1. **Check the extension before `lstat`.** The walker already knows the entry is a regular file
   (`e.file_type`). Non-media files then need no system call at all. `seen` keeps counting every regular
   file. With `--sniff-all`, every file is still `lstat`ed.
2. **One read connection per worker thread**, not one per 64 files. Use rayon `map_init` or a per-thread
   slot, with the statement prepared once.
3. **No lookups when the index is empty** (first scan, `--force`). Check the `file` row count once at the start.
4. **Writer settings:** `PRAGMA cache_size = -65536` (64 MB) and `temp_store = MEMORY` on the writer
   connection; sort each batch by `full_path` before inserting; try batches of 2,048 instead of 512. Watch
   peak RSS. A 64 MB cache is fine against the 250 MB limit, and can be smaller if it does not help.
5. **Bulk first scan:** try building the five secondary indexes after the inserts on a first scan, instead
   of keeping them up to date row by row. They stay defined in the migration: drop them and create them again
   in one transaction, and test that a crash in between leaves a database that the next scan repairs. Keep
   this only if it saves more than 20%.
6. **Thread counts:** workers = performance cores (`sysctl hw.perflevel0.physicalcpu`, 8 on M4 Pro). The
   walker gets its own smaller count; try 2, 4 and 6. Update the comment for `threads = 0` in the config:
   it should say "performance cores".

## Stage S3: Walk and process at the same time (1 day)

Today `flush` stops the walk loop (scan/mod.rs:356). Instead:

```
walker ──(chunks of 512 paths, bounded 4)──► N processing threads ──(items, bounded 8)──► writer
```

- The walk loop sends full chunks to a bounded channel and keeps walking. It waits only when that channel is
  full, which is our loop, not the `descend` callback (rule 3).
- Processing threads (the S2 pool) each keep their own read connection and send `Vec<Item>` to the writer.
- **Cancel:** every stage checks the token. Drop the senders in order (walk → process → writer). A cancel
  while the writer is busy must still end with a `partial` scan and no prune. Add a test that cancels at 10%,
  50% and 99% and checks each run ends within 1 s.
- **Errors:** the writer stopping (disk full, database locked) stops the walk, as `writer_alive` does today.
- Progress stays time-based in the walk loop. `done` comes from the processing threads.

## Stage S4: Rescans (½ day)

1. **Unchanged files:** today each one gets its own `UPDATE ... last_seen_scan_id` (writer.rs:90). Instead,
   put the ids in a temp table (`CREATE TEMP TABLE seen(id INTEGER PRIMARY KEY)`) and run one `UPDATE` at
   the end. Alternatively, one `UPDATE ... WHERE id IN (...)` per batch. Measure both.
2. **One lookup per folder instead of per file:** `SELECT file_name, size_bytes, ... WHERE parent_folder=?`
   (the `file_parent` index exists), kept in a small map while that folder's files come through. Entries
   arrive grouped by folder in `Completion` order, so the cache hit rate should be high. Check it with the
   S1 counters.
3. **Skip what did not change:** when a rescan found no new, changed or removed files, skip Live Photo
   pairing and places (0.9 s of the 1.4 s on 104k). First check what those steps do on an unchanged tree,
   then add a test that a rescan with one new pair still pairs it.

## Stage S5: Metadata pass (1 day; only if S1 shows it matters)

- Split the 2.9 s (104k) by format and by step (open, read, parse, places, write).
- Already planned elsewhere: decode JPEG/PNG through ImageIO with a size limit (similar tier memory, plan 06
  Phase 4). That is the duplicates search, not the scan, so keep it a separate commit.
- Ideas to measure, not to assume:
  - read the JPEG header in one 64 KiB read instead of several small ones
  - check that files whose `metadata_status` is `ok` and are unchanged are never reopened
  - one pool for both passes instead of building a new one
- **Open each file once:** the scan pass opens each media file for 32 bytes, then the metadata pass opens it
  again. Doing both in one open is a bigger change (two progress steps become one). Do it only if S1 shows
  `open` is a large share, and ask the user first.

## Stage S6: Last resort, own bulk walker (2 days; only if the goals are still missed)

`getattrlistbulk` returns name, type, size, times, inode, device **and file flags** (`ATTR_CMN_FLAGS`, which
holds `SF_DATALESS`) for many entries per system call. That would remove the `lstat` per media file.
dua-core uses it inside but does not expose flags or birth time (ADR-0005).

- **Option A (recommended first):** a pull request to dua-core that exposes the flags and birth time. It is
  outward-facing, so ask the user before opening it. Until it is merged, pin a git revision.
- **Option B:** our own walker over `getattrlistbulk` in `sys/`. It needs `unsafe` (only `sys/imageio.rs`
  has it today) and a new ADR that replaces part of ADR-0005, and it must keep every walker test and the
  50k-folder test.

## Done when

- [ ] Goals in §2 are met, or each miss is explained in `docs/perf.md` with the S1 numbers.
- [ ] Equivalence test, all scan tests, full check green. Peak RSS is flat from 100k to 1M.
- [ ] Cancel test at 10/50/99% passes 20 times in a row (`cargo nextest run --retries 0` in a loop).
- [ ] ADR-0009 "Scan pipeline" records the final shape (stages, channel sizes, thread counts, pragmas) and
      the numbers.
- [ ] `docs/STATUS.md` updated. The "scan CPU optimization deferred" open item is closed.
