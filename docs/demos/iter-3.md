# Demo: Iteration 3

```sh
export YOJAKA_HOME=$PWD/out/home                     # keep the index and logs out of ~/Library
cargo run -p fixturegen -- out/ --files 5000 --videos 0.1 --live-photos 30
cargo run --release -p yojaka-cli -- out/tree        # first visit: scans by itself, then shows the dashboard
```

In the app:

| Key | What to check |
|-----|---------------|
| `1` | Gauge moved while scanning (bouncing bar, then a percentage for "Reading metadata"); totals, formats, years and cameras fill in. |
| `2` | Browser. `j`/`k` scroll, `Tab` moves to the folder tree, `l` expands a folder and the table follows. |
| `t` | Cycles all, photos, videos. The title shows the count. |
| `/` then `iphone` | Filters by name, camera or place. `Enter` keeps it, `Esc` clears it. |
| `s`, `S` | Next sort column, reverse order. |
| `G` | Jumps to the last row (the table shows `…` for a moment on a big index, then fills in). |
| `r`, then `Esc` | Rescan, then stop it. "Last scan stopped early, partial results kept". |
| `D` | Debug overlay: frame time (should stay under 16 ms) and fetch time. |
| `?` | Help overlay. |
| `q` or `Ctrl-C` | Quit. The terminal is restored; exit code 0. |

Check against `out/manifest.json`:
- `Live Photos` on the dashboard equals `expected.live_pairs`.
- Rows with a `LIVE` badge come in pairs (HEIC/JPEG + MOV with the same name).
- The header always says `[READ-ONLY]`.
- Try `yojaka /System`: the dashboard shows the "protected folder" error and nothing is scanned.

Big-index check (1M rows, frame time under 16 ms):

```sh
YOJAKA_PERF_ROOT=$PWD/out/million cargo nextest run -p yojaka-core --run-ignored only -E 'binary(browse_perf)' --no-capture
cargo run --release -p yojaka-cli -- out/million   # press 2, D, then G / s / S
rm -rf out/million
```
