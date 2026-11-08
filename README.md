# Yojaka

A terminal app for indexing, de-duplicating and organizing photo and video files on macOS (Apple Silicon only).

**Status: pre-alpha. Iterations 0-3 are done (scan, metadata, places, a terminal UI). Everything is read-only: nothing touches your files yet. See `docs/STATUS.md`.**

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
Place names come from [GeoNames](https://www.geonames.org), licensed under
[Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/). The data (populated
places with 1,000 or more people, plus country and region names) was converted into a compact
binary format (`crates/yojaka-core/data/geo.bin.zst`, built by `tools/geodata`) so places can be
looked up offline; the names are used as provided. GeoNames does not endorse Yojaka.
`yojaka --about` prints the same credit.
