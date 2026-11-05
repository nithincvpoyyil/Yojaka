check:
    cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace

ci:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace --profile ci
    cargo test --workspace --doc
    cargo deny check
    cargo bench --workspace --no-run
    cargo build --release -p yojaka-cli

fixture N="1000":
    cargo run -p fixturegen -- out/ --files {{N}} --seed 42

tui PATH=".":
    cargo run -p yojaka-cli -- {{PATH}}
