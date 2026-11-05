# Yojaka

A terminal app for indexing, de-duplicating and organizing photo and video files on macOS (Apple Silicon only).

**Status: pre-alpha (v0.0.1, Iteration 0). Read-only skeleton: nothing touches your files yet.**

## Safety promise
Yojaka never writes to your files outside Plan → Dry-run → Confirm → Execute (journaled). Delete means move to the Trash. Files are never overwritten. Location lookup is offline; coordinates never leave your machine.

## Build from source
```sh
rustup toolchain install   # picks up rust-toolchain.toml
cargo build --release -p yojaka-cli
./target/release/yojaka doctor
```

## Develop
```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo nextest run
cargo run -p yojaka-cli -- doctor
cargo run -p fixturegen -- out/ --files 1000   # deterministic test tree + manifest
```
Specs live in `all-specs/`; the iteration plan is `all-specs/03-iteration-plan.md`.

## License
MIT, see `LICENSE`.

## Attribution
Place names (from Iteration 2) will use GeoNames data, licensed CC-BY 4.0: <https://www.geonames.org>.
