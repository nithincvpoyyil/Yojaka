# Performance

Targets come from tech spec §10. Rows are filled in as iterations land.

| Metric | Target | Measured | Iteration |
|--------|--------|----------|-----------|
| Release binary size (`yojaka`, aarch64) | < 100 MB (G5) | 1.6 MB (I0), 3.8 MB (I1), 7.3 MB (I2) | 0 |
| Scan 115k media files (135k entries), first scan | < 30 s | 4.8 s, 24 MB peak RSS (`dua-core`, 104k media files) | 1 |
| Rescan unchanged, same tree | < 5 s and ≥ 5× faster | 0.5 s (10× faster) | 1 |
| Scan 600k media + 400k other (1M entries), first scan | — | 51.9 s, 25 MB peak RSS (`dua-core`) | 1 |
| Rescan unchanged, 1M-entry tree | — | 6.5 s, 24 MB peak RSS (`dua-core`) | 1 |
| Peak RSS, 1M-entry tree | < 150 MB (I1), < 250 MB (G2) | 25 MB | 1 |
| Scan + metadata, 104k media files (11% videos, 1% corrupt), first scan | < 30 s | 7.5 s wall, 108 MB peak RSS (4.3 s walk + 2.9 s metadata and places) | 2 |
| Rescan unchanged with metadata, same tree | — | 0.5 s walk + 0.9 s Live Photo pairing | 2 |
| Geocode 100k GPS points | < 1 s | 0.39 s incl. one-time load | 2 |
| Release binary growth | < 6 MB | +3.5 MB (3.78 MB → 7.26 MB) | 2 |
| Metadata of a 4 GB video | < 1 MB read | counting-reader test, `moov` first and last | 2 |
| TUI frame render, 160×48 browser (debug overlay `D`) | < 16 ms | 0.2 – 2.0 ms typical, 4.3 ms worst seen, on a 1M-row index (release) | 3 |
| Browser page fetch, 1M-row index (256 rows) | — | 0.2 – 1.3 ms with an indexed sort or a ready sort, 73 – 92 ms for a jump to the end of a path sort | 3 |
| Sort a column without an index, 1M rows (once per query) | — | 0.77 – 0.90 s; every later page < 1 ms (before the temp table: 4 – 5 s per deep page) | 3 |
| Folder tree, 1M rows / 20k folders | — | 82 ms | 3 |
| Statistics, 1M rows | — | 4.0 s (own thread, never blocks scrolling) | 3 |
| Text filter that matches few rows, 1M rows | — | 1.5 s (full scan, no FTS) | 3 |
| Hash throughput (blake3, 1 MB) | baseline | `cargo bench -p yojaka-core` | 0 |

Measured 2026-11-06 on an M4 Pro (12 cores), internal SSD, release build, `/usr/bin/time -l`, fixturegen trees (small files, so walk and open dominate).
2026-11: the walker is `dua-core` (ADR-0005). jwalk without back-pressure peaked at 271 MB on 1M entries; jwalk with back-pressure could deadlock.
