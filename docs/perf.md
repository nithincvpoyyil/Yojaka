# Performance

Targets come from tech spec §10. Rows are filled in as iterations land.

| Metric | Target | Measured | Iteration |
|--------|--------|----------|-----------|
| Release binary size (`yojaka`, aarch64) | < 100 MB (G5) | 1.6 MB (I0, 2026-11-05) | 0 |
| Scan 100k files | see tech spec §10 | — | 1 |
| Hash throughput (blake3, 1 MB) | baseline | `cargo bench -p yojaka-core` | 0 |
