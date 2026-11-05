# Performance

Targets come from tech spec §10. Rows are filled in as iterations land.

| Metric | Target | Measured | Iteration |
|--------|--------|----------|-----------|
| Release binary size (`yojaka`, aarch64) | < 100 MB (G5) | 1.6 MB (I0, 2026-11-05) | 0 |
| Scan 115k media files (135k entries), first scan | < 30 s | 5.0 s, 59 MB peak RSS | 1 |
| Rescan unchanged, same tree | < 5 s and ≥ 5× faster | 0.5 s (10× faster) | 1 |
| Scan 600k media + 400k other (1M entries), first scan | — | 42.7 s, 46 MB peak RSS | 1 |
| Rescan unchanged, 1M-entry tree | — | 5.1 s, 46 MB peak RSS | 1 |
| Peak RSS, 1M-entry tree | < 150 MB (I1), < 250 MB (G2) | 46 MB | 1 |
| Hash throughput (blake3, 1 MB) | baseline | `cargo bench -p yojaka-core` | 0 |

Measured 2026-11-06 on an M4 Pro (12 cores), internal SSD, release build, `/usr/bin/time -l`, fixturegen trees (small files, so walk and open dominate).
Before walker back-pressure was added, the 1M-entry scan peaked at 271 MB.
