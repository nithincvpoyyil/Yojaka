# Demo: Iteration 0

```sh
cargo run -p yojaka-cli -- --version      # version, git SHA, build date, target
cargo run -p yojaka-cli -- doctor         # paths, tools, Full Disk Access, CPU cores
cargo run -p yojaka-cli -- doctor --json
cargo run -p yojaka-cli -- dupes .        # "arrives in Iteration 4", exit 1
cargo run -p yojaka-cli                   # TUI opens with [READ-ONLY]; ? = help, 1-5 = screens, q = quit
cargo run -p fixturegen -- out/ --files 500
```

`out/tree` is the scan root, `out/manifest.json` is the ground truth.
