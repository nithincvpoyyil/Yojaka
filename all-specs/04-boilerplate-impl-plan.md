# Yojaka — Boilerplate Implementation Plan (Iteration 0, v0.1, 2026-10-30)

This is the step-by-step build plan for the **project skeleton**. When it is done:

- The repo builds, lints, tests and runs.
- Every crate and module from tech spec §2 exists, with its public types in place.
- Iterations 1–6 only have to **fill in logic**. They never have to restructure the project.

It expands Iteration 0 of `03-iteration-plan.md`.

> **Scope rule:** the boilerplate defines *shapes* (crates, modules, types, traits, error enums, events, config, paths, the CLI surface, the TUI loop, the test harness and CI). It contains **no domain logic** such as scanning, hashing or file operations.
> The one exception is plumbing every later iteration needs. These pieces are fully implemented in I0:
> - config load/save
> - app paths
> - logging
> - the cancel token
> - the event channel
> - the SQLite migration runner
> - terminal setup/restore
> - `yojaka doctor`
> - fixturegen v1

---

## 0. Summary

| Step | What | Est. | Verify |
|------|------|------|--------|
| 1 | Toolchain + repo init | 0.5 h | `cargo --version`, `git status` |
| 2 | Workspace `Cargo.toml`, lints, profiles | 1 h | `cargo metadata` |
| 3 | `yojaka-core` skeleton + plumbing | 4 h | unit tests |
| 4 | `yojaka-tui` skeleton (event loop, Dashboard placeholder) | 3 h | snapshot test, manual run |
| 5 | `yojaka-cli` (clap surface, logging, `doctor`, version) | 2 h | `assert_cmd` tests |
| 6 | `yojaka-llm` placeholder | 0.25 h | builds |
| 7 | `tools/fixturegen` v1 (images, dupes, manifest) | 4 h | generated tree matches manifest |
| 8 | `tools/geodata` placeholder | 0.25 h | builds |
| 9 | Test infrastructure (nextest, insta, `TestEnv`) | 1 h | `cargo nextest run` |
| 10 | Bench skeleton (criterion) | 0.5 h | `cargo bench --no-run` |
| 11 | CI + `cargo-deny` | 1 h | green run (needs a GitHub remote) |
| 12 | Docs: README, CHANGELOG, ADRs, demo script | 1 h | — |
| 13 | Spikes: metadata crates, geocoder → ADRs | 1 day | ADRs merged |
| 14 | Final verification + tag `v0.0.1` | 0.5 h | checklist §15 |

The total is about 3 days. Each step is **one commit** using Conventional Commits (`chore:`, `feat(core):`, `test:`, `ci:`, `docs:`).

---

## 1. Toolchain and repo init

### 1.1 Install (one time, on the dev Mac)
```sh
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustup component add rustfmt clippy llvm-tools-preview

# Cargo tools
cargo install --locked cargo-nextest cargo-deny cargo-insta
brew install hyperfine            # end-to-end timing
# optional: cargo install --locked flamegraph
```

These tools are already present on the dev machine: `git` 2.39, `exiftool`, `ollama`, and `sips` (built into macOS; used by fixturegen to produce HEIC files).
`gh` is **not** installed. Run `brew install gh` if CI and PRs should be driven from the terminal.

### 1.2 Repo init
```sh
cd ~/Development/Yojaka
git init -b main
```

Decide what to do with the empty `Specs` file at the repo root. Proposed: delete it, since `all-specs/` replaces it. **Ask the user before deleting.**

### 1.3 Files

**`rust-toolchain.toml`**: pins the toolchain so CI and local builds match. Use the exact stable version installed in 1.1.
```toml
[toolchain]
channel = "1.xx.0"            # fill with `rustc --version` at install time
components = ["rustfmt", "clippy"]
targets = ["aarch64-apple-darwin"]
profile = "minimal"
```

**`.gitignore`**
```
/target
/out/            # fixturegen output
*.db
*.db-wal
*.db-shm
.DS_Store
/.yojaka/
*.snap.new       # insta pending snapshots
```

**`rustfmt.toml`**
```toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
```

**`clippy.toml`**
```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-print-in-tests = true
```

**`.editorconfig`**: UTF-8, LF line endings, 4 spaces for `.rs`, 2 spaces for `.toml`, `.yml` and `.md`.

**Commit:** `chore: init repo, toolchain and formatting config`

---

## 2. Workspace manifest

**`Cargo.toml` (root):**
```toml
[workspace]
resolver = "3"
members = [
  "crates/yojaka-core",
  "crates/yojaka-tui",
  "crates/yojaka-cli",
  "crates/yojaka-llm",
  "tools/fixturegen",
  "tools/geodata",
]
default-members = ["crates/yojaka-cli"]

[workspace.package]
version = "0.0.1"
edition = "2024"
rust-version = "1.85"           # edition 2024 minimum; raise if a dependency needs it
license = "MIT OR Apache-2.0"   # [DECIDE] license
repository = ""                 # fill when the GitHub repo exists
publish = false

[workspace.dependencies]
# internal
yojaka-core = { path = "crates/yojaka-core" }
yojaka-tui  = { path = "crates/yojaka-tui" }
yojaka-llm  = { path = "crates/yojaka-llm" }

# UI / CLI
ratatui   = "0.30"              # use ratatui::crossterm (re-export) so versions never drift
clap      = { version = "4.6", features = ["derive", "env", "wrap_help"] }
indicatif = "0.18"

# concurrency / fs
rayon             = "1.12"
crossbeam-channel = "0.5"
jwalk             = "0.9"
memmap2           = "0.9"
libc              = "0.2"
trash             = "5.2"

# detection / metadata / hashing (wired in I1–I4; declared now so versions live in one place)
infer        = "0.22"
imagesize    = "0.15"
kamadak-exif = "0.6"
nom-exif     = "3.8"            # spike decides between this and kamadak-exif + mp4parse
mp4parse     = "0.17"
blake3       = { version = "1.8", features = ["rayon", "mmap"] }
unicode-normalization = "0.1"

# storage / serde
rusqlite   = { version = "0.41", features = ["bundled"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
toml       = "1.1"
uuid       = { version = "1.29", features = ["v7", "serde"] }
time       = { version = "0.3", features = ["serde", "formatting", "parsing", "local-offset", "macros"] }
directories = "6"

# geo (I2)
kiddo = "6.3"
zstd  = "0.14"

# errors / logging
thiserror          = "2"
anyhow             = "1"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender   = "0.2"

# fixturegen
image = { version = "0.25", default-features = false, features = ["jpeg", "png", "gif"] }
rand  = "0.10"

# dev
insta      = { version = "1.49", features = ["yaml"] }
proptest   = "1.12"
tempfile   = "3.27"
assert_cmd = "2.2"
predicates = "3.1"
criterion  = { version = "0.8", features = ["html_reports"] }

[workspace.lints.rust]
unsafe_code = "deny"                 # allowed only in yojaka-core::sys::macos (scoped #[allow])
missing_debug_implementations = "warn"
unused_must_use = "deny"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
todo = "deny"                        # no todo!() / unimplemented!() in shipped code
unimplemented = "deny"
dbg_macro = "deny"
unwrap_used = "warn"
print_stdout = "deny"                # TUI must never print; yojaka-cli overrides locally
print_stderr = "deny"

[profile.dev.package."*"]
opt-level = 2                        # blake3, sqlite and image run fast in debug tests

[profile.release]
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"                      # the panic hook restores the terminal before abort

[profile.profiling]                  # cargo build --profile profiling → flamegraph / Instruments
inherits = "release"
debug = true
strip = false
```

Notes:
- Versions are the latest stable releases on crates.io as of 2026-10-30. `Cargo.lock` is committed, so builds are reproducible. If a release is less than 2 weeks old at install time (for example `rusqlite 0.41.1`), let the lockfile pick the previous patch release with `cargo update -p rusqlite --precise 0.41.0`.
- Every member crate sets `[lints] workspace = true` and `version.workspace = true` (likewise `edition`, `license`, `publish`).
- Dependencies that are declared but not used yet stay in `[workspace.dependencies]` only. A crate adds them to its own `[dependencies]` in the iteration that uses them, so that `cargo-deny` / `cargo-machete` doesn't flag them as unused.

**Commit:** `chore: workspace manifest, lints and build profiles`

---

## 3. `yojaka-core` skeleton

### 3.1 `crates/yojaka-core/Cargo.toml`
```toml
[package]
name = "yojaka-core"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true

[dependencies]
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
tracing.workspace = true
crossbeam-channel.workspace = true
rusqlite.workspace = true
directories.workspace = true
uuid.workspace = true
time.workspace = true

[dev-dependencies]
tempfile.workspace = true
insta.workspace = true
proptest.workspace = true
criterion.workspace = true

[[bench]]
name = "baseline"
harness = false

[lints]
workspace = true
```

### 3.2 Module tree
Every file listed exists after I0. The **I0** column shows what the file contains at the end of I0. **"shape"** means it holds types, traits and doc comments only, with no functions that do work yet.

```
crates/yojaka-core/src/
├── lib.rs              I0: module decls, re-exports, `Engine` facade
├── error.rs            I0: full: YojakaError, Result, FileIssue
├── cancel.rs           I0: full: CancelToken
├── events.rs           I0: full: CoreEvent, EventSink
├── config.rs           I0: full: Config (all sections), load/save
├── paths.rs            I0: full: AppPaths (data/index/journal/logs/config)
├── model.rs            I0: shape: FileId, MediaKind, Format, Fingerprint, Place
├── sys/
│   ├── mod.rs          I0: shape
│   └── macos.rs        I0: `is_dataless(&Metadata)` (safe, std MetadataExt::st_flags); later: clonefile, renamex_np
├── index/
│   ├── mod.rs          I0: full: Index::open(path) → WAL + migrations
│   ├── migrate.rs      I0: full: user_version runner
│   └── migrations/
│       └── 0001_init.sql   I0: `meta_kv` table only; I1 adds the file/scan schema as 0002
├── scan/mod.rs         I0: shape: ScanOptions, ScanSummary, ScanProgress            → I1
├── detect.rs           I0: shape: fn signature doc only                              → I1
├── meta/mod.rs         I0: shape: Extractor trait, Meta, MetaError                   → I2
├── livephoto.rs        I0: shape: PairId                                             → I2
├── geo/mod.rs          I0: shape: GeoPoint, Place, LocSrc, Geocoder trait           → I2
├── hash.rs             I0: shape: ContentHash newtype                                → I4
├── dupes/mod.rs        I0: shape: Tier, DupGroup, KeepRule                           → I4
├── plan/mod.rs         I0: shape: Plan, Op, PlanSummary, Warning, SkipReason          → I5
├── exec/mod.rs         I0: shape: JournalEntry, OpState                              → I5
└── organize/mod.rs     I0: shape: Template (parse stub returns Unsupported)           → I6
```

### 3.3 Key code (implement in full in I0)

**`error.rs`**
```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum YojakaError {
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("index error: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("config error in {path}: {message}")]
    Config { path: PathBuf, message: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("{0} is not available yet (planned for {1})")]
    NotYet(&'static str, &'static str),          // e.g. ("dupes", "Iteration 4")
}
pub type Result<T, E = YojakaError> = std::result::Result<T, E>;

/// A per-file problem: collected and reported, never fatal (tech spec §12).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileIssue { pub path: PathBuf, pub kind: IssueKind, pub message: String }

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum IssueKind { PermissionDenied, NotFound, Unreadable, BadMetadata, Changed, Other }
```

**`cancel.rs`**
```rust
#[derive(Debug, Clone, Default)]
pub struct CancelToken(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl CancelToken {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) { self.0.store(true, std::sync::atomic::Ordering::Relaxed) }
    pub fn is_cancelled(&self) -> bool { self.0.load(std::sync::atomic::Ordering::Relaxed) }
    /// `?`-friendly check inside loops.
    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() { Err(crate::YojakaError::Cancelled) } else { Ok(()) }
    }
}
```

**`events.rs`**: the core → UI channel (tech spec §3.1).
```rust
#[derive(Debug, Clone)]
pub enum CoreEvent {
    TaskStarted  { task: TaskKind, label: String },
    Progress     { task: TaskKind, done: u64, total: Option<u64>, bytes: u64, current: Option<std::path::PathBuf> },
    Issue(crate::FileIssue),
    TaskFinished { task: TaskKind, outcome: TaskOutcome },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind { Scan, Metadata, Geocode, Hash, Plan, Execute }  // Undo was removed in I9 (ADR-0009)
#[derive(Debug, Clone)]
pub enum TaskOutcome { Complete, Partial, Cancelled, Failed(String) }

/// Wraps a bounded sender.
/// - `progress()` never blocks; it drops the event if the UI is behind.
/// - `important()` blocks; it is used for start/finish/issue events.
#[derive(Debug, Clone)]
pub struct EventSink(crossbeam_channel::Sender<CoreEvent>);
impl EventSink {
    pub fn channel(cap: usize) -> (Self, crossbeam_channel::Receiver<CoreEvent>) { /* bounded(cap) */ }
    pub fn progress(&self, ev: CoreEvent) { let _ = self.0.try_send(ev); }
    pub fn important(&self, ev: CoreEvent) { let _ = self.0.send(ev); }
    pub fn null() -> Self { /* receiver dropped; used by CLI --quiet and tests */ }
}
```

**`paths.rs`**: all on-disk locations in one place, injected (never global).
```rust
#[derive(Debug, Clone)]
pub struct AppPaths { pub data: PathBuf, pub logs: PathBuf }   // data = ~/Library/Application Support/yojaka
impl AppPaths {
    /// Real locations; respects the YOJAKA_HOME override (read once in main, never via set_var).
    pub fn from_env() -> crate::Result<Self>;
    /// Tests: everything under a temp dir.
    pub fn under(root: &Path) -> Self;
    pub fn config_file(&self) -> PathBuf;              // data/config.toml
    pub fn index_dir(&self) -> PathBuf;                // data/indexes/
    pub fn index_for(&self, scan_root: &Path) -> PathBuf; // indexes/<blake3(canon)[..16]>.db (I1 adds blake3; I0 uses a placeholder std hash)
    pub fn journal_dir(&self) -> PathBuf;              // data/journal/
    pub fn ensure(&self) -> crate::Result<()>;         // mkdir -p all
}
```

Use `directories::BaseDirs`: `data_dir()` is `~/Library/Application Support` and `home_dir()/Library/Logs` is the logs folder. Then join `yojaka`. Do **not** use `ProjectDirs`, which produces reverse-DNS folder names on macOS.

**Rust 2024 note:** `std::env::set_var` is `unsafe` in edition 2024. Tests never set environment variables. They pass `AppPaths::under(tmp)` instead.

**`config.rs`**: mirrors tech spec §9 and adds the `[location]` section.
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default)]                      // missing keys → defaults; unknown keys are ignored (forward-compatible)
pub struct Config {
    pub threads: usize,                // 0 = auto
    pub scan: ScanConfig,              // sniff_all, kinds, skip_dirs, skip_hidden, exiftool_fallback
    pub dupes: DupesConfig,            // keep_rules
    pub organize: OrganizeConfig,      // template = "{year}/{month:02}/{name}", large_plan_threshold
    pub location: LocationConfig,      // near_km 25, far_km 100, infer false, infer_window_minutes 120, no_location_dir
    pub llm: LlmConfig,                // provider "none", model, allow_cloud false
}
impl Config {
    pub fn load(path: &Path) -> crate::Result<Self>;   // missing file → Default
    pub fn save(&self, path: &Path) -> crate::Result<()>; // atomic: write <path>.tmp, fsync, rename
}
```

Tests:
- Default → TOML matches an insta snapshot.
- A partial file still loads, with defaults for the missing keys.
- A file with unknown keys still loads.
- Save then load returns the same value.

**`index/mod.rs` + `migrate.rs`**
```rust
pub struct Index { conn: rusqlite::Connection }
impl Index {
    pub fn open(path: &Path) -> crate::Result<Self> {
        // PRAGMA journal_mode=WAL; synchronous=NORMAL; foreign_keys=ON; busy_timeout=5000
        // migrate::run(&conn, MIGRATIONS)
    }
    pub fn open_in_memory() -> crate::Result<Self>;   // tests
    pub fn schema_version(&self) -> crate::Result<u32>;
}
// migrate.rs
pub(crate) const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("migrations/0001_init.sql"))];
// Runs each migration with version > user_version inside its own transaction, then sets PRAGMA user_version.
```

`0001_init.sql`:
```sql
CREATE TABLE meta_kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO meta_kv VALUES ('created_by', 'yojaka');
```

Tests:
- A fresh database ends at version 1.
- Re-opening the database is a no-op.
- A future version (user_version = 99) gives a clear error: "index was created by a newer Yojaka".

**`model.rs` (shape)**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct FileId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind { Image, Video }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Jpeg, Png, Gif, Heic, Webp, Tiff, Bmp, Avif,
    RawDng, RawCr2, RawCr3, RawNef, RawArw, RawRaf, RawOrf,
    Mov, Mp4, M4v, ThreeGp, Avi, Mkv, Webm, Mts,
}
impl Format {
    pub fn kind(self) -> MediaKind;
    pub fn from_extension(ext: &str) -> Option<Self>;  // I0: full (pure table lookup) + unit tests
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint { pub size: u64, pub mtime_ns: i64, pub hash: Option<[u8; 32]> }
```

**`sys/macos.rs`**
```rust
use std::os::macos::fs::MetadataExt;
const SF_DATALESS: u32 = 0x4000_0000;
/// True for iCloud placeholders. Opening one would start a download (FR-14).
pub fn is_dataless(md: &std::fs::Metadata) -> bool { md.st_flags() & SF_DATALESS != 0 }
// I5/I6 add (scoped #[allow(unsafe_code)]): rename_exclusive (renamex_np + RENAME_EXCL), clone_file (clonefile)
```

**`lib.rs`**
```rust
pub mod cancel; pub mod config; pub mod error; pub mod events; pub mod index;
pub mod model; pub mod paths; pub mod sys;
pub mod detect; pub mod dupes; pub mod exec; pub mod geo; pub mod hash;
pub mod livephoto; pub mod meta; pub mod organize; pub mod plan; pub mod scan;

pub use cancel::CancelToken;
pub use config::Config;
pub use error::{FileIssue, IssueKind, Result, YojakaError};
pub use events::{CoreEvent, EventSink, TaskKind, TaskOutcome};
pub use paths::AppPaths;

/// Facade used by every UI (TUI, CLI, future GUI). Methods are added per iteration:
/// scan (I1), stats (I1/I2), dupes (I4), plan/apply (I5), organize (I6).
#[derive(Debug)]
pub struct Engine { pub paths: AppPaths, pub config: Config }
impl Engine {
    pub fn new(paths: AppPaths, config: Config) -> Self { Self { paths, config } }
}
```

**Rule, enforced in review:** `yojaka-core/Cargo.toml` must never list `ratatui`, `crossterm`, `clap` or `indicatif`.

**Commit:** `feat(core): crate skeleton, errors, config, paths, events, index migrations`

---

## 4. `yojaka-tui` skeleton

### 4.1 `Cargo.toml`
Dependencies: `yojaka-core`, `ratatui`, `crossbeam-channel`, `tracing`. Dev-dependencies: `insta`.
There is no direct `crossterm` dependency. Use `ratatui::crossterm`.

### 4.2 Files
```
crates/yojaka-tui/src/
├── lib.rs          pub fn run(engine: Engine, root: PathBuf, opts: TuiOptions) -> anyhow-free Result
├── terminal.rs     TerminalGuard: enter raw/alt-screen; Drop restores. Panic hook installed once.
├── app.rs          App state, Msg, update(), Screen enum, mode (ReadOnly/Write)
├── event_loop.rs   poll input (33 ms) + drain CoreEvent; redraw only when dirty
├── view.rs         top-level layout: header / body / footer
├── theme.rs        colours (badge green/amber), styles
├── keymap.rs       key → Msg table (q, ?, 1–5, Esc, arrows, hjkl)
└── screens/
    ├── mod.rs
    ├── dashboard.rs   I0: placeholder "No scan yet — press s to scan (Iteration 1)"
    └── help.rs        I0: full help overlay generated from keymap.rs
```

### 4.3 Key code
```rust
// terminal.rs
pub struct TerminalGuard { pub term: ratatui::DefaultTerminal }
impl TerminalGuard {
    pub fn enter() -> std::io::Result<Self> { Ok(Self { term: ratatui::try_init()? }) } // try_init also installs a panic hook that restores
}
impl Drop for TerminalGuard { fn drop(&mut self) { ratatui::restore(); } }

// app.rs
pub enum Msg { Key(KeyEvent), Resize, Core(CoreEvent), Tick, Quit }
pub enum Screen { Dashboard, Browser, Duplicates, Organize, History } // only Dashboard renders content in I0
pub struct App { pub screen: Screen, pub show_help: bool, pub mode: Mode, pub root: PathBuf, pub should_quit: bool, /* status */ }
impl App { pub fn update(&mut self, msg: Msg) -> bool /* dirty */ { … } }

// event_loop.rs
loop {
    if dirty { guard.term.draw(|f| view::render(&app, f))?; dirty = false; }
    if event::poll(Duration::from_millis(33))? { dirty |= app.update(Msg::from(event::read()?)); }
    while let Ok(ev) = core_rx.try_recv() { dirty |= app.update(Msg::Core(ev)); }
    if app.should_quit { break; }
}
```

**Layout in I0:**
- Header: `Yojaka · <root> · [READ-ONLY]`
- Body: Dashboard placeholder.
- Footer: `q quit · ? help · 1–5 screens`

Screens 2–5 show "Arrives in Iteration N".

### 4.4 Tests
- `TestBackend` 80×24 and 160×48 → `insta::assert_snapshot!` of the buffer for the Dashboard and Help screens.
- `update()` unit tests:
  - `q` sets quit.
  - `?` toggles help.
  - `3` switches to the Duplicates screen.

**Commit:** `feat(tui): terminal guard, event loop, dashboard placeholder, help`

---

## 5. `yojaka-cli` (binary `yojaka`)

### 5.1 `Cargo.toml`
```toml
[package]
name = "yojaka-cli"
# workspace fields…
[[bin]]
name = "yojaka"
path = "src/main.rs"

[dependencies]
yojaka-core.workspace = true
yojaka-tui.workspace = true
clap.workspace = true
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
tracing-appender.workspace = true

[dev-dependencies]
assert_cmd.workspace = true
predicates.workspace = true
insta.workspace = true
tempfile.workspace = true

[lints]
workspace = true
```

Cargo doesn't allow local overrides next to `[lints] workspace = true`. Printing is this crate's job, so `main.rs` starts with `#![allow(clippy::print_stdout, clippy::print_stderr)]`. It still must never print while the TUI is running.

### 5.2 Files
```
crates/yojaka-cli/
├── build.rs            sets YOJAKA_GIT_SHA (git rev-parse --short HEAD, fallback "unknown"), YOJAKA_BUILD_DATE
└── src/
    ├── main.rs         platform gate, parse args, init logging, dispatch, exit code
    ├── cli.rs          clap definitions (the full v1 surface from tech spec §8)
    ├── logging.rs      tracing → ~/Library/Logs/yojaka/yojaka.log (daily rotation), YOJAKA_LOG filter
    ├── exit.rs         ExitCode enum: 0 ok, 1 error, 2 usage, 3 partial
    └── cmd/
        ├── mod.rs
        ├── doctor.rs   I0: full
        └── stub.rs     I0: every other subcommand → "`yojaka dupes` arrives in Iteration 4", exit 1
```

### 5.3 CLI surface (all defined in I0, so `--help` is final from day one)
```rust
#[derive(clap::Parser)]
#[command(name = "yojaka", version = LONG_VERSION, about = "Index, de-duplicate and organize photos & videos")]
pub struct Cli {
    /// Folder or volume to open (default: current dir)
    pub path: Option<PathBuf>,
    /// Allow write operations (still requires dry-run + confirm)
    #[arg(long, global = true)] pub write: bool,
    #[arg(long, global = true)] pub threads: Option<usize>,
    #[arg(long, global = true)] pub local_index: bool,
    #[arg(long, global = true, env = "YOJAKA_HOME", hide = true)] pub home: Option<PathBuf>,
    #[command(subcommand)] pub cmd: Option<Cmd>,
}
pub enum Cmd {
    Scan { path, kind: KindArg, sniff_all: bool },                // I1
    Stats { path, json: bool },                                   // I1/I2
    Info { file },                                                // I2
    Places { path, json: bool },                                  // I2
    Export { path, format: ExportFmt, include_gps: bool },        // I2
    Dupes { path, tier: TierArg, keep: Vec<String>, plan_out: Option<PathBuf>, json: bool }, // I4
    Organize { path, dest, template, copy: bool, r#move: bool, infer_location: bool, plan_out }, // I6
    Plan { #[command(subcommand)] sub: PlanCmd },                 // I5: show FILE [--full-tree]
    Apply { file, yes: bool, resume: bool },                      // I5
    // `Undo` was removed in I9 (ADR-0009)
    History,                                                      // I5
    Doctor,                                                       // I0
}
```

`yojaka` with no subcommand opens the TUI on `path` or the current directory.

### 5.4 `main.rs` flow
```rust
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
compile_error!("Yojaka v1 supports macOS on Apple Silicon only");

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let paths = AppPaths::from_env_or(cli.home.as_deref())?;   // ensure() dirs
    let _log_guard = logging::init(&paths.logs);               // keep guard alive until exit
    let config = Config::load(&paths.config_file())?;          // CLI flags override config
    let engine = Engine::new(paths, config);
    match cli.cmd {
        None => yojaka_tui::run(engine, root, opts),           // stdout belongs to the TUI from here
        Some(Cmd::Doctor) => cmd::doctor::run(&engine),
        Some(other) => cmd::stub::not_yet(&other),
    }
    // map Result → exit::ExitCode; errors are printed as "error: …" + "hint: …" to stderr
}
```

### 5.5 `yojaka doctor` (fully implemented in I0)
Prints:
- Version, git SHA, target, macOS version.
- Data, index, journal, log and config paths, and whether each one is writable.
- The resolved config file path, and whether it exists.
- Tools found: `exiftool` (with version), `ffprobe`, `ollama`.
- Full Disk Access hint: try `read_dir("~/Library/Mail")`. `EPERM` means it is **not granted**, followed by instructions.
- CPU: performance and efficiency core counts (`sysctl hw.perflevel0.logicalcpu` and `hw.perflevel1.logicalcpu`), and the default thread count that will be used.

`--json` gives machine-readable output, which tests use.

### 5.6 Tests (`tests/cli.rs`, `assert_cmd`)
- `--version` contains `0.0.1`.
- `--help` output → insta snapshot. This locks the CLI surface; any change is reviewed as a snapshot diff.
- `doctor --json` with `YOJAKA_HOME=<tmp>` → valid JSON, and the paths are under the tmp folder.
- `dupes .` → exit 1, and stderr contains "Iteration 4".

**Commit:** `feat(cli): clap surface, logging, doctor, version info`

---

## 6. `yojaka-llm` placeholder
`src/lib.rs` contains only the `LlmProvider` trait and the request and error types from tech spec §13, plus `#![doc = "Planned for Iteration 9"]`. It has no dependencies other than `thiserror`, and **no crate depends on it yet**.

**Commit:** `chore(llm): placeholder crate with provider trait`

---

## 7. `tools/fixturegen` v1

The deterministic test-data generator, used by every integration test and benchmark.

### 7.1 Command line
```
cargo run -p fixturegen -- OUT_DIR
  --files 1000            # unique media files before duplicates
  --seed 42
  --depth 4               # max folder nesting
  --dupes 0.10            # fraction duplicated (exact byte copies, renamed: "(1)", " copy", other folders)
  --corrupt 0.01          # truncated / garbage-after-magic files
  --kinds image           # I0: image only; I2 adds video, live
  --gps 0.6               # fraction with EXIF GPS (from a fixed list of known places, used by geo tests)
  --hardlinks 5 --symlinks 5
  --noise 200             # non-media files (txt, pdf-like) to exercise filtering
```

### 7.2 What v1 generates (I0)
| Item | How |
|------|-----|
| JPEG | `image` crate encodes a small (64×48) gradient image; then an **EXIF APP1** segment is spliced in by our own small TIFF/IFD writer (`exif_writer.rs`: DateTimeOriginal, Make, Model, Orientation, GPS IFD) |
| PNG, GIF | `image` crate (GIF: some with 2+ frames) |
| HEIC | Encode a JPEG, then `sips -s format heic in.jpg --out out.heic` (built into macOS). Results are cached per seed under `~/.cache/yojaka-fixtures/` so repeat runs are fast. Skipped with a warning if `sips` is missing. |
| Duplicates | Byte-identical copies under different names and folders |
| Name variants | `IMG_0001.jpg`, `IMG_0001 (1).jpg`, `IMG_0001 copy.jpg` (some identical, some different bytes, for the name tier) |
| Corrupt | Correct magic bytes followed by a truncated body |
| Links | Hard links (`std::fs::hard_link`) and symlinks, pointing both inside and outside the root |
| Skip-dir bait | A fake `Test.photoslibrary/` and a `.git/` holding images that must **not** be indexed |

Added later (in the iteration that needs them):
- I2: synthetic MOV/MP4 through `isobmff_writer.rs` (`ftyp`/`moov`/`mvhd`/`trak`/`tkhd`/`mdhd`/`hdlr`, Apple `keys`/`ilst`: creationdate, ISO6709, content.identifier, make/model), with `moov` placed both before and after `mdat`; Live Photo pairs.
- I7: re-encoded and resized variants for the likely and similar tiers.

### 7.3 Manifest (`OUT_DIR/manifest.json`): the ground truth tests assert against
```json
{
  "seed": 42, "generator_version": 1,
  "files": [
    { "path": "a/b/IMG_0001.jpg", "kind": "image", "format": "jpeg", "size": 2311,
      "taken_at": "2023-08-14T10:22:01", "camera": "Apple iPhone 15",
      "gps": { "lat": 15.4989, "lon": 73.8278, "expect_city": "Panaji", "expect_cc": "IN" },
      "dup_group": 7, "name_group": 3, "hardlink_of": null, "corrupt": false, "should_index": true }
  ],
  "expected": {
    "indexed_count": 1234, "exact_groups": [["a/x.jpg", "c/x (1).jpg"]],
    "by_format": { "jpeg": 800, "png": 200 }
  }
}
```

### 7.4 Tests
- Same seed gives byte-identical output (compare the hash of the whole tree).
- The manifest counts match a `walkdir` count of the output.
- Every generated JPEG parses with `kamadak-exif` and has the EXIF fields that were written.

**Commit:** `feat(tools): fixturegen v1 with manifest`

---

## 8. `tools/geodata` placeholder
A binary crate that prints "Builds the embedded GeoNames blob — implemented in Iteration 2".
`README.md` in the crate lists the source files and the license:
- `cities1000.zip`, `admin1CodesASCII.txt`, `admin2Codes.txt` and `countryInfo.txt` from download.geonames.org.
- License: CC-BY 4.0.

`crates/yojaka-core/assets/` stays empty for now. The blob lands there in I2.

**Commit:** `chore(tools): geodata placeholder`

---

## 9. Test infrastructure

| File | Purpose |
|------|---------|
| `.config/nextest.toml` | `default` profile; `ci` profile with `retries = 0`, `fail-fast = false`, JUnit output |
| `crates/yojaka-core/tests/common/mod.rs` | `TestEnv { tmp: TempDir, paths: AppPaths }`, `TestEnv::with_fixture(seed, files)` (runs the fixturegen library API directly, not the binary) |
| `tests/data/README.md` | Provenance rules for real sample files: small (< 1 MB), GPS replaced with known coordinates, license or "own photo". **Action for the user (by I2):** provide 1 iPhone Live Photo pair, 1 iPhone HEVC MOV, 1 Android MP4, and 1 camera RAW or DNG. |
| `insta` | Snapshots live next to the tests (`snapshots/`). Review with `cargo insta review`. |

To make `TestEnv::with_fixture` possible, `fixturegen` is a **lib + bin**. Its `lib.rs` exposes `generate(&Spec, out) -> Manifest`.

**Commit:** `test: nextest config, TestEnv harness, sample-data rules`

---

## 10. Bench skeleton
`crates/yojaka-core/benches/baseline.rs` uses criterion. Its group `baseline` benchmarks `Format::from_extension` and a 1 MB `blake3` hash. The hash measures the machine's hashing throughput, which later benchmarks are compared against.
Its main job is to prove the harness works: `cargo bench --no-run` runs in CI.

**Commit:** `test(bench): criterion harness`

---

## 11. CI and supply-chain checks

**`.github/workflows/ci.yml`**
```yaml
name: ci
on: { push: { branches: [main] }, pull_request: {} }
concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }
jobs:
  check:
    runs-on: macos-latest          # arm64 (Apple Silicon) runners
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "<same as rust-toolchain.toml>", components: "rustfmt, clippy" }
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with: { tool: "cargo-nextest,cargo-deny" }
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo nextest run --workspace --profile ci
      - run: cargo test --workspace --doc
      - run: cargo deny check
      - run: cargo bench --workspace --no-run
      - run: cargo build --release -p yojaka-cli && ls -lh target/release/yojaka   # track binary size
```

**`deny.toml`**
- Licenses allowed: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0 (for `mp4parse`), CC0-1.0.
- Advisories: deny.
- Bans: warn on duplicate versions.
- Sources: crates.io only.

**Action for the user:** CI runs only once the repo is pushed to GitHub. There is no remote yet, and `gh` is not installed. Locally, `just ci` (§12) runs the same checks.

**Commit:** `ci: github actions + cargo-deny`

---

## 12. Docs and developer ergonomics

| File | Content |
|------|---------|
| `README.md` | What Yojaka is, its safety promise, install-from-source steps, `yojaka doctor`, status ("pre-alpha, read-only"), GeoNames attribution placeholder |
| `CHANGELOG.md` | Keep-a-Changelog format; `## [0.0.1] — Iteration 0` |
| `docs/adr/0001-workspace-and-core-boundary.md` | Why core, TUI and CLI are split, and the "no UI deps in core" rule |
| `docs/adr/0002-metadata-crates.md` | Written by spike 13.1 |
| `docs/adr/0003-offline-geocoder.md` | Written by spike 13.2 |
| `docs/demos/iter-0.md` | Demo script: `yojaka --version`, `yojaka doctor`, `yojaka` (TUI opens, help, quit), `fixturegen out/ --files 500` |
| `docs/perf.md` | Table skeleton for the tech spec §10 targets |
| `justfile` (optional, `brew install just`) | `just check` = fmt + clippy + nextest; `just ci` = the full CI list; `just fixture N`; `just tui PATH` |

`CLAUDE.md` gets a link to this file and the final command list.

**Commit:** `docs: readme, changelog, ADR-0001, demo script`

---

## 13. Spikes (time-boxed; results recorded in ADRs; spike code deleted afterwards)

### 13.1 Metadata crates (½ day)
- **Where:** `crates/yojaka-core/examples/spike_meta.rs` (examples are not shipped).
- **Test set:** fixturegen JPEG, PNG and HEIC files, plus any user-provided iPhone and Android samples.
- **Candidates:**
  - **A:** `nom-exif` only.
  - **B:** `kamadak-exif` + `mp4parse`.
- **Measure:**
  - Fields that come back correctly: date, make/model, GPS, dimensions, video duration, codec, ISO 6709, Apple content identifier.
  - Bytes read on a large MOV, using a counting reader.
  - Time per file.
  - Binary size added.
- **Decision rule:** choose the option with the most correct fields, with no full-file reads. If they tie, choose fewer dependencies.

### 13.2 Offline geocoder (½ day)
- **Where:** `tools/geodata/examples/spike_geo.rs`.
- **Candidates:**
  - **A:** the `reverse_geocoder` crate (last released 2024-03). Check its dataset and admin-name support.
  - **B:** our own GeoNames → `kiddo` + `zstd` blob.
- **Measure** on 20 known coordinates (Panaji, Paris, Tokyo, mid-Atlantic, the North Pole, Fiji near the antimeridian, a border town):
  - Whether it is correct.
  - Lookup time.
  - Blob size.
  - Whether admin1/admin2 come back as names.
- **Decision rule:** choose correct names and a binary increase under 6 MB. **Expected outcome: B**, because A has gone 2.5 years without releases and its admin-name support is unverified.

**Commit:** `docs(adr): metadata crates and geocoder decisions`

---

## 14. Final repo layout after Iteration 0
```
Yojaka/
├── .config/nextest.toml
├── .github/workflows/ci.yml
├── .editorconfig  .gitignore  clippy.toml  rustfmt.toml  rust-toolchain.toml  deny.toml  justfile
├── Cargo.toml  Cargo.lock
├── CLAUDE.md  README.md  CHANGELOG.md
├── all-specs/   (01–04 specs, what-is-it.md)
├── docs/  adr/  demos/iter-0.md  perf.md
├── crates/
│   ├── yojaka-core/  src/{lib,error,cancel,events,config,paths,model,detect,hash,livephoto}.rs
│   │                 src/{sys,index,scan,meta,geo,dupes,plan,exec,organize}/
│   │                 assets/  benches/baseline.rs  tests/common/mod.rs
│   ├── yojaka-tui/   src/{lib,terminal,app,event_loop,view,theme,keymap}.rs  src/screens/
│   ├── yojaka-cli/   build.rs  src/{main,cli,logging,exit}.rs  src/cmd/{doctor,stub}.rs  tests/cli.rs
│   └── yojaka-llm/   src/lib.rs
├── tools/
│   ├── fixturegen/   src/{lib,main,exif_writer,tree}.rs
│   └── geodata/      src/main.rs  README.md
└── tests/data/README.md
```

---

## 15. Done checklist (Iteration 0 acceptance)

- [ ] `cargo build --workspace` passes with zero warnings.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [ ] `cargo nextest run --workspace` passes. That includes the config, migration, cancel, `Format`, TUI snapshot, CLI and fixturegen-determinism tests.
- [ ] `cargo deny check` passes.
- [ ] `yojaka --version` prints the version, git SHA and target.
- [ ] `yojaka doctor` lists correct paths and detects `exiftool` and `ollama`.
- [ ] `yojaka` opens the TUI with the READ-ONLY badge. `?` shows help, `q` quits, and the terminal is restored. After a forced panic (debug-only hidden key `Ctrl-\`), the terminal is also restored.
- [ ] No output goes to stdout or stderr while the TUI is running (checked by clippy lints plus a manual check). The log file receives entries.
- [ ] `cargo run -p fixturegen -- out/ --files 1000 --seed 42` produces the tree and the manifest. Running it twice gives identical output.
- [ ] `yojaka-core` has no UI dependencies (`cargo tree -p yojaka-core | grep -E "ratatui|crossterm|clap"` prints nothing).
- [ ] Release binary size is recorded in `docs/perf.md`.
- [ ] ADR-0002 and ADR-0003 are written, and tech spec §1 is updated with the decisions.
- [ ] Tag `v0.0.1`, and fill in the CHANGELOG.

## 16. Decisions needed from the user (none block starting)
1. **License:** MIT OR Apache-2.0 (proposed), or proprietary?
2. **GitHub repo:** name and visibility (needed for CI). Install `gh`?
3. **Empty `Specs` file:** OK to delete?
4. **Sample media for `tests/data`** (needed by I2): see §9.
5. **G5 binary-size target** in the product spec (that line is currently incomplete).
