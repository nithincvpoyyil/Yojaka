# Demo: Iteration 2

```sh
export YOJAKA_HOME=$PWD/out/home                     # keep the index and logs out of ~/Library
cargo run -p fixturegen -- out/ --files 2000 --videos 0.1 --live-photos 20
cargo run --release -p yojaka-cli -- scan out/tree   # also reads metadata, pairs Live Photos, resolves places
cargo run --release -p yojaka-cli -- stats out/tree  # video totals, years, cameras, places, Live Photo pairs
cargo run --release -p yojaka-cli -- places out/tree
cargo run --release -p yojaka-cli -- info out/tree/IMG_0001.jpg    # pick any file; shows GPS and place
cargo run --release -p yojaka-cli -- export out/tree --format csv --out out/export.csv   # no GPS columns
cargo run --release -p yojaka-cli -- export out/tree --format json --include-gps | head
cargo run --release -p yojaka-cli -- scan out/tree --no-metadata   # file index only
```

Check against `out/manifest.json`:
- `expected.live_pairs` equals "Live Photos: N pairs" in `stats`.
- Entries with `gps` resolve to the listed country in `places`.
- The `BROKEN_*.mp4` videos and corrupt images are listed as unreadable metadata, and nothing crashes.
- `grep -c gps out/export.csv` is 0 for the first export, and the JSON export with `--include-gps` has `gps_lat` and `gps_lon`.
