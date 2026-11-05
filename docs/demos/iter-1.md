# Demo: Iteration 1

```sh
cargo run -p fixturegen -- out/ --files 5000        # tree + manifest
cargo run --release -p yojaka-cli -- scan out/tree  # progress bar, summary
cargo run --release -p yojaka-cli -- scan out/tree  # incremental: 0 new or changed
cargo run --release -p yojaka-cli -- stats out/tree
cargo run --release -p yojaka-cli -- stats out/tree --json
cargo run --release -p yojaka-cli -- scan /System   # refused: protected location
```

Check against `out/manifest.json`: `expected.indexed_count` equals "media files indexed".
Photos in `stats` are lower by the number of hard links, because hard links count once.
