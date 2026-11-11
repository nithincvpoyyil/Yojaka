# Yojaka — Technical Spec (v0.1, 2026-10-30)

Companion to `01-product-spec.md`. FR-xx references point to that document.

---

## 1. Tech stack

| Concern | Choice | Why |
|---------|--------|-----|
| Language | Rust, edition 2024, stable toolchain | Speed, memory safety, single binary |
| Target | `aarch64-apple-darwin` | M-series only (product spec §5) |
| TUI | `ratatui` + `crossterm` backend | Per the spec; mature; testable with `TestBackend` |
| CLI args | `clap` (derive) | Standard |
| Directory walk | `dua-core` (parallel, unordered, bounded queue) | Bulk `getattrlistbulk` on APFS, flat memory, cancellable; see ADR-0005 |
| Parallelism | `rayon` (CPU work) + `crossbeam-channel` (pipeline) | No async runtime needed for local disk work |
| Format sniffing | `infer` | Magic-byte detection (FR-10) |
| Dimensions | `imagesize` | Reads the header only, never decodes the image |
| EXIF | `kamadak-exif` (`exif` crate); macOS ImageIO for raw | Pure Rust; JPEG, TIFF, HEIF, PNG, WebP. Raw photos (DNG, CR2, CR3, NEF, ARW, RAF, ORF) through ImageIO properties, `kamadak-exif` as the fallback (ADR-0002 addendum) |
| Video metadata | Own ISO-BMFF box walker (MOV/MP4/M4V/3GP) and a minimal EBML walker (MKV/WebM) in `meta/` | Seeks box by box and reads only the small leaf boxes, so `mdat` and sample tables are never read (ADR-0002). `nom-exif` and `mp4parse` were dropped: neither lets us bound the bytes read. |
| Reverse geocoding | Built-in GeoNames data (`cities1000` + `admin1CodesASCII` + `admin2Codes` + `countryInfo`) built by `tools/geodata` into a zstd blob (`crates/yojaka-core/data/geo.bin.zst`, 2.9 MB, committed), embedded with `include_bytes!`, loaded lazily into a static 3-D k-d tree over unit-sphere coordinates | Offline, about 1 µs per lookup, binary grows 3.5 MB in total with all of I2 (ADR-0003). Own k-d tree instead of `kiddo` (about 60 lines, no API risk). **License:** GeoNames is CC-BY 4.0, so attribution is required in `--about` and the README. |
| Video fallback | `exiftool -json` in batches (installed); `ffprobe` used only if present | AVI and MTS/M2TS, plus odd containers the Rust crates miss |
| PNG/GIF details | Chunk and block walkers in `meta/image.rs` over the capped header read | APNG `acTL` and GIF frame count; no extra crates |
| Hashing | `blake3` (with `mmap` + `rayon` features) | Fastest secure hash; uses NEON on Apple Silicon |
| Perceptual hash | Own 64-bit difference hash and colour signature over upright pixels decoded by `image` (JPEG, PNG, GIF, WebP, TIFF, BMP) and macOS ImageIO through `objc2-image-io` (HEIC, AVIF; raw photos through the camera's embedded preview), own BK-tree | Similar tier, photos only, opt-in (ADR-0008). `image_hasher` was not needed. |
| Index storage | `rusqlite` (`bundled` feature), WAL mode | Embedded, queryable, low memory, crash-safe |
| Serialization | `serde`, `serde_json` | Plans, journal, config, export |
| Trash | `trash` crate (macOS backend) | Reversible delete (FR-33) |
| Config | `toml` + `directories` | `~/Library/Application Support/yojaka/config.toml` |
| Errors | `thiserror` (core), `anyhow` (bin) | Typed errors in the library |
| Logging | `tracing` + `tracing-appender` → file | **Never log to stdout while the TUI is running** |
| Testing | `insta` (snapshots), `proptest`, `tempfile`, `assert_cmd`, `criterion` | See §11 |
| LLM (later) | `reqwest` (blocking or tokio) behind a `LlmProvider` trait | Isolated in its own crate |

Pin crate versions to the latest stable releases at Iteration 0 and commit `Cargo.lock`. Use `cargo-deny` to check licenses and advisories.

## 2. Workspace layout

```
yojaka/
├── Cargo.toml                # [workspace]
├── crates/
│   ├── yojaka-core/          # ALL domain logic. No UI and no terminal code. Shared by TUI, CLI and future GUI.
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs
│   │       ├── scan/         # walker, filters, macOS-specific checks (dataless, bundles)
│   │       ├── detect.rs     # extension + magic-byte detection
│   │       ├── meta/         # per-format metadata extractors (image + video)
│   │       ├── livephoto.rs  # Live Photo pairing (photo + companion MOV)
│   │       ├── geo/          # GPS parsing (EXIF + ISO 6709), offline reverse geocoder, location inference
│   │       ├── index/        # SQLite schema, migrations, writer thread, queries
│   │       ├── hash.rs       # staged hashing
│   │       ├── dupes/        # tiers, grouping, keep rules
│   │       ├── organize/     # path templates, planner
│   │       ├── plan/         # Plan model, validation, dry-run summary, tree diff
│   │       ├── exec/         # executor, journal, undo
│   │       ├── events.rs     # progress/event types sent to the UI
│   │       └── cancel.rs     # CancelToken
│   ├── yojaka-tui/           # ratatui app: screens, widgets, key handling
│   ├── yojaka-cli/           # binary `yojaka`: clap, subcommands, launches TUI
│   └── yojaka-llm/           # (later) provider trait + Ollama/Claude/OpenAI/AFM
├── tools/geodata/            # build step: GeoNames dumps → compact zstd k-d tree blob (output is committed)
├── tools/fixturegen/         # generates synthetic photo + video trees for tests and benchmarks
├── benches/
└── all-specs/
```

**Rule:** `yojaka-core` must not depend on `ratatui` or `crossterm`. UIs talk to the core through:
- **Commands**: function calls.
- **Events**: a channel of `CoreEvent` values.
- **Queries**: read-only index access.

## 3. Architecture

```
            ┌────────────── yojaka-cli (binary) ──────────────┐
            │  clap → subcommand runner │ or → TUI app        │
            └──────────────┬──────────────────┬───────────────┘
                           │                  │
                     yojaka-core API     yojaka-tui (main thread: render + input)
                           │                  ▲  CoreEvent (bounded channel)
   ┌───────────────────────┼──────────────────┴───────────────────────────┐
   │  Scan pipeline (worker threads)                                      │
   │                                                                      │
   │  dua-core ─► filter/detect ─► metadata (rayon pool) ─► batcher ─► DB writer (1 thread) │
   │   │            │                 │                                  │   │
   │   └─ progress ─┴─────────────────┴──────────► CoreEvent::Progress   │   │
   │                                                                      ▼   │
   │                                                       SQLite (WAL) index │
   │  Dupes:    query size groups ─► partial hash ─► full hash (rayon) ─► groups │
   │  Organize: index query ─► template render ─► Plan                    │
   │  Exec:     Plan ─► validate ─► journal ─► fs ops ─► journal          │
   └──────────────────────────────────────────────────────────────────────┘
```

### 3.1 Threading model
- **Main thread:** the TUI event loop. It polls `crossterm` input with a ~16–33 ms timeout, drains `CoreEvent`s without blocking, and redraws only when something changed.
- **Pipeline threads:**
  - The walker runs on `dua-core`'s own worker threads; its result queue is bounded (ADR-0005).
  - Detection and metadata extraction run on a rayon pool sized to the **performance cores** (default `num_cpus - 2`; configurable).
- **Single DB writer thread:** receives `Vec<FileRecord>` batches (about 2,000 rows) and writes each batch in one transaction. This avoids SQLite lock contention.
- **Back-pressure:** every channel is bounded (for example capacity 64 batches), so memory stays flat when the disk is faster than the database or the other way round.
- **Cancellation:** a `CancelToken` (`Arc<AtomicBool>`) is checked per directory and per file. On cancel, the current batch is flushed and the scan is marked `partial`.
- No tokio in v1. It may be added only inside `yojaka-llm`.

### 3.2 Memory strategy
- Never hold the full file list in memory. The index on disk is the source of truth.
- The TUI tables use **virtual scrolling**: they fetch only the visible window (`LIMIT/OFFSET` or keyset paging).
- Paths are stored as bytes (`OsStr`), never forced into lossy UTF-8.
- Dedup grouping is done with SQL (`GROUP BY size HAVING count > 1`), so only candidate files are hashed.
- Metadata readers read headers only, through `BufReader` with a capped read (for example the first 256 KB). EXIF in JPEG and HEIC sits near the start of the file. Readers never decode pixels, except for perceptual hashing in a later tier.

## 4. macOS specifics (FR-14)

| Concern | Implementation |
|---------|----------------|
| iCloud dataless files | `lstat` → check `st_flags & SF_DATALESS` (0x40000000). If set, record as `cloud_only` and **do not open the file**. |
| Bundles | Skip directories whose names end in `.photoslibrary`, `.aplibrary`, `.app`, `.bundle`, or `.fcpbundle`. The list is configurable. |
| Hard links | Record `(device_id, inode)`. When grouping duplicates, collapse entries with the same inode. |
| Volumes | Record `st_dev` so the planner can detect cross-volume moves (FR-55). |
| Free space | `statvfs` on the destination before executing (FR-56). |
| Full Disk Access | When TCC denies access (`EPERM` on protected folders), show a one-time hint explaining how to grant Full Disk Access. Do not show it repeatedly. |
| Live Photos | iPhone stores the pair as `IMG_1234.HEIC` + `IMG_1234.MOV`. Both carry the same Apple content identifier: MakerNote `ContentIdentifier` in the photo and the QuickTime key `com.apple.quicktime.content.identifier` in the video. Pair on that ID. If the ID is missing, fall back to the same folder + same stem + creation times within 2 s. See §6.2a. |
| Trash | `trash` crate. Record the trashed path in the journal so it can be restored. |
| Case-insensitive APFS | Detect and handle conflicts case-insensitively when the destination volume is case-insensitive. |
| Unicode normalization | APFS keeps the normalization form it was given. Compare names after NFC normalization (`unicode-normalization` crate) when detecting name-tier duplicates and conflicts. |

## 5. Data model (SQLite)

Location: `~/Library/Application Support/yojaka/indexes/<blake3(root_canonical_path)[:16]>.db`. Optionally `<root>/.yojaka/index.db` with `--local-index`. Schema versions are tracked with `PRAGMA user_version`, and migrations are embedded in the binary.

```sql
CREATE TABLE scan (
  id INTEGER PRIMARY KEY,
  root_path BLOB NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT NOT NULL,              -- running|complete|partial|failed
  files_seen INTEGER,
  error_count INTEGER
);

CREATE TABLE file (
  id INTEGER PRIMARY KEY,
  full_path BLOB NOT NULL UNIQUE,    -- absolute, raw bytes
  parent_folder BLOB NOT NULL,       -- for tree views
  file_name BLOB NOT NULL,
  extension TEXT,
  size_bytes INTEGER NOT NULL,
  modified_at_ns INTEGER NOT NULL,
  status_changed_at_ns INTEGER,
  created_at_ns INTEGER,             -- birth time (macOS)
  device_id INTEGER NOT NULL,
  inode INTEGER NOT NULL,
  media_kind TEXT NOT NULL,          -- image|video
  format TEXT,                       -- jpeg|png|gif|heic|webp|tiff|bmp|avif|raw-*|mov|mp4|m4v|three-gp|avi|mkv|webm|mts
  is_cloud_only INTEGER NOT NULL DEFAULT 0,
  last_seen_scan_id INTEGER NOT NULL REFERENCES scan(id),
  metadata_status TEXT NOT NULL DEFAULT 'pending',  -- pending|ok|error|skipped
  partial_hash BLOB,                 -- blake3 of first+last 16 KiB (+ size)
  full_hash BLOB,                    -- blake3 of whole file
  perceptual_hash INTEGER            -- later: 64-bit perceptual hash
);
CREATE INDEX file_size ON file(size_bytes);
CREATE INDEX file_full_hash ON file(full_hash);
CREATE INDEX file_parent ON file(parent_folder);
CREATE INDEX file_devino ON file(device_id, inode);
CREATE INDEX file_kind ON file(media_kind);

CREATE TABLE meta (
  file_id INTEGER PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
  width INTEGER,
  height INTEGER,
  taken_at INTEGER,                  -- unix seconds, local time as recorded in EXIF
  taken_at_src TEXT,                 -- exif|mtime|none
  camera_make TEXT,
  camera_model TEXT,
  lens TEXT,
  orientation INTEGER,
  has_gps INTEGER,
  gps_lat REAL,
  gps_lon REAL,
  -- reverse-geocoded place (filled by geo pass; cached)
  place_country_code TEXT,           -- ISO 3166-1 alpha-2
  place_country TEXT,
  place_state TEXT,                  -- admin1
  place_district TEXT,               -- admin2
  place_city TEXT,                   -- nearest populated place
  place_dist_km REAL,                -- distance to that place
  loc_src TEXT,                      -- gps|inferred|none
  loc_from_file INTEGER,             -- for inferred: the file it was borrowed from
  frames INTEGER,                    -- gif/webp/png(apng) animation
  -- video-only columns (NULL for images)
  duration_ms INTEGER,
  fps REAL,
  rotation INTEGER,                  -- 0|90|180|270 from track matrix
  video_codec TEXT,                  -- hevc|h264|prores|av1|vp9|mpeg4|...
  audio_codec TEXT,
  bitrate INTEGER,                   -- bits/s (size*8/duration)
  container TEXT,
  -- Live Photo pairing
  content_id TEXT,                   -- Apple content identifier, if present
  pair_id INTEGER,                   -- same value on both halves of a Live Photo
  extra_json TEXT                    -- rarely-used fields
);
CREATE INDEX meta_taken ON meta(taken_at);
CREATE INDEX meta_pair ON meta(pair_id);
CREATE INDEX meta_place ON meta(place_country_code, place_state, place_city);

CREATE TABLE scan_error (
  scan_id INTEGER,
  file_path BLOB,
  error_kind TEXT,
  error_message TEXT
);
```

**Incremental rescan (FR-13):**
- If the path exists and `(size_bytes, modified_at_ns, inode)` is unchanged, only update `last_seen_scan_id`.
- Otherwise, re-read metadata and clear the stored hashes.
- After the scan finishes, rows with `last_seen_scan_id < current` are deleted from the index. Only the index rows are removed; files on disk are never touched.

## 6. Pipelines

### 6.1 Scan
1. `dua_core::walk(root, threads, Order::Completion, Options::default().skip_metadata(), descend)`. `descend` prunes bundles, own folders, skip-list and hidden names (the last two below the root only); symlinks are never followed.
2. For each entry: `lstat` → skip non-regular files → check dataless.
3. Detection:
   - A known image or video extension is a fast accept, confirmed with magic bytes on the first 32 bytes (`ftyp` brand for ISO-BMFF: `qt  `, `isom`, `mp41`, `heic`, …). The brand separates HEIC (image) from MOV/MP4 (video).
   - An unknown or missing extension is checked with magic bytes only if `--sniff-all` is set. Otherwise it is skipped. This keeps scans of huge non-media trees fast.
   - `--kind image|video|all` filters at this step.
4. Metadata reader chosen by format (§6.2).
5. Results are batched to the DB writer. A `Progress { found, processed, bytes, current_dir, rate }` event is sent at most every 100 ms.

### 6.2 Metadata extractors
Trait in `meta/`:
```rust
pub trait Extractor: Send + Sync {
    fn formats(&self) -> &'static [Format];
    fn extract(&self, r: &mut dyn ReadSeek, hint: &FileInfo) -> Result<Meta, MetaError>;
}
```
- One implementation per format family:
  - Images: `jpeg`, `png`, `gif`, `heif`, `webp`, `tiff_raw`, `generic` (dimensions only).
  - Videos: `isobmff` (MOV/MP4/M4V/3GP), `matroska` (MKV/WebM), `other_video` (AVI, MTS: sniff + size only, details via the exiftool fallback).
- **Video reads must seek, not read ahead.** In MOV/MP4, the `moov` box (all the metadata) is often at the **end** of the file, after gigabytes of `mdat`. The extractor walks box headers (8–16 bytes each), seeks past `mdat`, and reads only `moov`. Never read a whole video to get its metadata.
- Video date: QuickTime `com.apple.quicktime.creationdate` (has a timezone) is preferred over `mvhd` creation time (UTC; often 1904-01-01 or 0 when unset; treat those as missing).
- An extraction error is not fatal: set `meta_status = 'error'` and continue.
- Optional `--exiftool-fallback` for formats the Rust crates can't parse (on by default for AVI/MTS). It runs `exiftool -json` in batches. `exiftool` is already on this machine.

### 6.2a Live Photo pairing (FR-20b)
Runs as a SQL pass after metadata extraction:
1. Pair files that share a `content_id` (one image, one video). Give both the same `pair_id`.
2. Fallback for unpaired `.MOV` files: an image in the same folder with the same stem and `|taken_at − video created| ≤ 2 s`.

Everything after this treats a pair as one **item**:
- Duplicate grouping compares items.
- The planner always emits both ops for a pair together. If either half fails validation, both are skipped.
- **Invariant (property test):** no plan trashes or moves exactly one half of a pair.
- The UI shows a `LIVE` badge and the combined size.

### 6.2b Location: GPS → approximate place (FR-44..48)
1. **GPS parsing:**
   - Photos: EXIF `GPSLatitude`/`GPSLongitude`, using the `Ref` tags for N/S and E/W.
   - Videos: QuickTime `com.apple.quicktime.location.ISO6709` or the `©xyz` atom, for example `+15.4989+073.8278+012.000/`.
   - Reject (0,0) and out-of-range values; store them as no GPS.
2. **Reverse geocode:**
   - The embedded k-d tree is loaded **lazily**, only when a template, stats or filter needs place data. Loading takes about 50 ms, after which the data is shared through `OnceLock`.
   - The lookup finds the nearest populated place, uses haversine distance, and returns `(cc, admin1, admin2, city, dist_km)`.
   - The admin1 and admin2 codes are turned into names using the embedded tables.
   - Results are cached in `meta.place_*` and only recomputed when the GPS changes or the geodata version changes (stored in `PRAGMA user_version`-style metadata).
3. **Precision guard:**
   - Up to `near_km` (25): `{city}` = the city name.
   - Up to `far_km` (100): `{city}` = `Near <city>`.
   - Beyond `far_km`: `{city}` and `{district}` are empty and the template drops those path segments. The country is used only if the point falls inside it. Approximation: the nearest place has the same country code and the distance is under 300 km. Otherwise the country is `_unknown_location`.
4. **Inference (opt-in):**
   - A SQL pass sorts the files of each camera model by `taken_at`.
   - A file without GPS takes the place of the nearest geotagged file from the same camera, if it is within `infer_window` (2 h). Set `loc_src='inferred'` and `loc_from_file`.
   - Also allowed across devices: when a camera has no geotagged files at all, use any geotagged file in the same folder within the window.
   - Never overwrite real GPS.
5. **Live Photo pairs** share one place (FR-47).
6. **Upgrade path (not v1):** finer place names (neighbourhood or landmark) through opt-in online lookups (Apple `CLGeocoder` via a Swift helper, or Nominatim with rate limiting), plus clustering of trips and events that the LLM can name (§13).

### 6.3 Duplicate detection (FR-30)
Staged, so the minimum number of bytes is read:
1. **Size groups:** a SQL query over all non-cloud files groups them by `size_bytes`; only sizes with count > 1 go on. Hard links (same `device_id, inode`) are collapsed first.
2. **Partial hash:** BLAKE3 of `size || first 16 KiB || last 16 KiB`, computed in parallel. Only partial hashes that match go on.
3. **Full hash:** BLAKE3 of the whole file, streamed or memory-mapped (`update_mmap_rayon` for files larger than 64 MB). Equal full hash means the files are **exact** duplicates.
4. **Likely tier:**
   - Images: group by `(taken_at, camera_model, width, height)` where `taken_at_src='exif'`.
   - Videos: group by `(taken_at, width, height)` with `duration_ms` within ±1000 ms. This catches re-exported or messenger-shared copies.
   - Files that are already exact duplicates of each other count once.
   - Images and videos are never grouped together.
   - A Live Photo is one item, listed under its photo; its video half is never matched on its own.
5. **Name tier:** normalize the name:
   - Apply NFC normalization and lowercase.
   - Strip the extension.
   - Strip ` (n)`, `(n)`, ` copy`, ` copy n`, ` - copy`, and `-n` / `_n` with one digit (so the counter in `IMG_1234` stays).

   Then group by kind and normalized name and keep runs whose sizes are within 5% of the smallest.
6. **Similar tier (images only, opt-in with `--similar`):** a 64-bit difference hash (9×8 grey cells) plus a colour signature (average colour of each quarter). Two pictures match when the hashes differ in at most `dupes.similar_distance` bits (default 6), the colours agree, and width over height agrees within 3%. Grouped with a BK-tree and union-find. Both values are cached in the index (`perceptual_hash`, `color_signature`).

The likely, name and similar tiers are guesses. Keep rules never mark their groups, the CLI never plans them, and in the TUI each group is marked by hand (ADR-0008). A group that a stronger tier already shows is not repeated. Measured precision and recall: `docs/dupes-accuracy.md`.

Hashes are cached in the index and stay valid while `(size, mtime)` is unchanged. Videos make up most of the bytes in a typical library. Size grouping plus the partial hash means only true candidates are fully read, and those reads use `update_mmap_rayon`.

Group model:
```rust
struct DupGroup {
    id: u64,
    tier: Tier,
    files: Vec<FileId>,
    keep: Option<FileId>,
    reclaim_bytes: u64,
}
```

Keep rules form a `Vec<KeepRule>` applied in order. Each rule narrows the candidates until one is left. If a tie remains, the file with the lowest id wins, so the result is deterministic.

### 6.4 Organize planner (FR-40..43)
- **Template mini-language:**
  - Placeholders: `{year} {month:02} {day:02} {camera} {make} {model} {format} {ext} {name} {stem} {parent} {kind}` (`{kind}` renders as `Photos` or `Videos`; the labels can be configured).
  - Location placeholders: `{country} {country_code} {state} {district} {city}` (see §6.2b). If a placeholder resolves to empty, its path segment is dropped, so there are no `//` or empty folders. A file with no location at all goes to `_no_location/`.
  - Default template: `{year}/{month:02}/{name}` for both kinds. A Live Photo's two halves always land in the same folder.
  - Literal text is kept as written.
  - Values are sanitized for the filesystem (`/` and `:` become `_`, length is trimmed).
  - A template is parsed once into tokens and rendered for each file.
- **Fallbacks:**
  - If there is no EXIF date, use mtime, and mark it as fallback in the plan.
  - If there is no date at all, use `_undated/`.
- **Conflict resolution:** a `HashMap<NormalizedDestPath, ...>` over the planned destinations, plus existence checks on disk.
  - If the destination already has a file with the same hash, plan `Skip(AlreadyThere)`.
  - Otherwise add a suffix: ` (1)`, ` (2)`, and so on.

### 6.5 Plan (FR-50)
```rust
pub struct Plan {
    pub id: Uuid,
    pub created_at: i64,
    pub root: PathBuf,
    pub kind: PlanKind,          // Dedupe | Organize
    pub mode: OpMode,            // Copy | Move | Trash | DeletePermanent
    pub ops: Vec<Op>,            // may be streamed to/from JSONL for huge plans
    pub summary: PlanSummary,    // counts, bytes, reclaim, cross-volume count
    pub warnings: Vec<Warning>,
}
pub enum Op {
    Move  { id: u64, src: PathBuf, dst: PathBuf, expect: Fingerprint },
    Copy  { id: u64, src: PathBuf, dst: PathBuf, expect: Fingerprint },
    Trash { id: u64, path: PathBuf, expect: Fingerprint, reason: DupRef },
    Delete{ id: u64, path: PathBuf, expect: Fingerprint, reason: DupRef },
    MkDir { id: u64, path: PathBuf },
    Skip  { id: u64, path: PathBuf, why: SkipReason },
}
pub struct Fingerprint { size: u64, mtime_ns: i64, hash: Option<[u8; 32]> }
```

**Invariant**, tested with proptest: every file the plan touches ends up in exactly one place (its destination, the Trash, or unchanged) and no op targets an existing path.
**Safety rule:** a dedupe plan must never trash every member of a group.

### 6.6 Tree preview and "glimpse" (FR-43, FR-51)
- Build a `DirTree` from the planned destinations (before view: source parents; after view: destination parents). Each node aggregates `(files, bytes)`.
- **Glimpse algorithm:**
  - Render breadth-first up to `max_lines` (default 40).
  - For each node, show its children sorted by size and stop after `k` children (default 8). Add `… N more folders (X files, Y GB)`.
  - Collapse single-child folder chains into one line (`2023/08/`).
- The TUI widget expands lazily. The CLI prints the glimpse, and `--full-tree` prints everything.
- **Before/after view:** two panes, or a unified diff with `+` for new folders and `−` for emptied ones.

### 6.7 Executor, journal, undo (FR-52..56)
- **Journal:** `~/Library/Application Support/yojaka/journal/<plan-id>.jsonl`. Each line is `{op_id, state: "begin"|"done"|"failed"|"skipped", detail, ts}`, and the file is fsynced every N entries plus at the end.
- **Per-op sequence:**
  1. Re-stat the source and compare it with `expect`. On mismatch, record `skipped(changed)`.
  2. Write `begin` to the journal.
  3. Perform the operation:
     - **Same-volume move:** `rename(2)`, refusing to overwrite (check existence first and use `renamex_np` with `RENAME_EXCL`).
     - **Cross-volume move:** copy to `dst.yojaka-tmp`, fsync, verify BLAKE3, rename into place, then remove the source.
     - **Copy:** same as cross-volume, without removing the source. APFS `clonefile(2)` may be used when on the same volume, because clones are instant.
     - **Trash:** `trash::delete`, recording the resulting Trash location where possible.
  4. Write `done` to the journal.
- **Parallelism:** independent ops run on a small pool (4–8 threads; I/O-bound). `MkDir` ops run first in order.
- **Undo:** read the journal in reverse and invert each `done` op:
  - Move becomes the reverse move.
  - Copy becomes deleting the copy, only if its hash still matches.
  - Trash becomes restoring from the Trash.

  Undo is itself journaled. A `begin` entry without `done` (for example after a crash) is resolved by checking which path currently exists.
- **Resume:** `yojaka apply --resume <plan-id>` skips ops that are already `done`.

## 7. TUI design (`yojaka-tui`)

**Pattern:** Elm-like. `App { state, screen_stack }`, then `update(Msg) -> Vec<Cmd>`, then `view(&state, frame)`. Messages come from keys, `CoreEvent`s, and timer ticks.

**Screens:**

| Key | Screen | Content |
|-----|--------|---------|
| `1` | **Dashboard** | Scan progress gauge, totals (photos and videos shown separately, plus total video duration), format distribution (bar chart, most used first), year histogram (sparkline or bars), top cameras, error count |
| `2` | **Browser** | Folder tree (left) and virtualized file table (right). Columns: name, kind, format, dimensions, duration, size, taken, camera, place (`≈` when inferred). `t` cycles the kind filter (all, photos, videos); `k` stays "up" for the vim keys. Sort and filter (`/`). Detail pane shows full metadata. |
| `3` | **Duplicates** | Groups list (tier badge, count, reclaim), with group members on the right. `f` searches (exact, likely, name); `F` also compares how pictures look. `v` filters by tier. `space` toggles keep/delete, `p` applies the next keep rule to every exact group. Groups of the other tiers show a side-by-side comparison (size, pixels, date, length, camera) and are marked one at a time. Total reclaim in the summary. |
| `4` | **Organize** | Destination picker, template presets (Date / Place / Place+Date / Kind+Date), template input with a live sample preview (5 example files, including a geotagged one and one without location), an infer-location toggle, coverage ("82% of files have a location"), copy/move toggle, and the proposed tree glimpse. |
| — | **Review / Dry-run** (modal) | Summary counts, warnings, before/after trees, op list sample, and a confirmation (typed phrase for large or permanent plans) |
| — | **Execute** | Progress, current op, error list, `Esc` to stop safely after the current op |
| `5` | **History** | Past plans from the journal, status, and `u` to undo |

**Global UI:**
- Header shows the root, the mode badge (**READ-ONLY** green or **WRITE** amber), and the index status.
- Footer shows key hints. `?` opens help and `q` quits.
- Mouse support is optional.
- Resizing must work down to 80×24.

## 8. CLI surface (`yojaka-cli`)

```
yojaka [PATH]                         # open TUI (read-only)
yojaka [PATH] --write                 # open TUI with write mode available
yojaka scan [PATH] [--kind image|video|all] [--sniff-all] [--force] [--threads N] [--local-index]
yojaka stats [PATH] [--json]
yojaka dupes [PATH] [--tier exact|likely|name|similar|all] [--similar] [--similar-distance BITS]
             [--keep RULE,...] [--plan-out FILE] [--json]   # only exact groups are marked or planned
yojaka organize [PATH] --dest DIR --template "{year}/{month:02}" [--copy|--move] [--infer-location] [--plan-out FILE]
                                     # e.g. --template "{country}/{city}/{year}-{month:02}/{name}"
yojaka plan show FILE [--full-tree]   # dry-run view of a saved plan
yojaka apply FILE [--yes] [--resume]  # executes; requires --write semantics
yojaka undo [PLAN_ID|--last]
yojaka history
yojaka export [PATH] --format csv|json [--filter ...] [--include-gps]
yojaka places [PATH] [--json]        # countries/cities with counts, location coverage
```

Exit codes:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error |
| 2 | Usage error |
| 3 | Partial success (some ops skipped or failed) |

Without `--yes`, `apply` prints the dry-run and prompts for confirmation on the terminal.

## 9. Config (`config.toml`)

```toml
threads = 0                    # 0 = auto (perf cores)
sniff_all = false
kinds = ["image", "video"]
skip_dirs = [".git", "node_modules", ".Trash", "*.photoslibrary"]
skip_hidden = true
keep_rules = ["prefer-path:~/Pictures", "highest-resolution", "oldest-taken", "shortest-path"]
organize_template = "{year}/{month:02}/{name}"   # applies to photos and videos
[location]
near_km = 25                   # beyond this, {city} becomes "Near <city>"
far_km = 100                   # beyond this, drop city/district
infer = false                  # --infer-location
infer_window_minutes = 120
no_location_dir = "_no_location"
language = "en"               # place-name language (ASCII names in v1)
large_plan_threshold = 1000
exiftool_fallback = false
[llm]                          # later
provider = "ollama"            # ollama | apple-fm | claude | openai | none
model = "..."
allow_cloud = false
```

User choices made in the TUI (template, keep rules) are written back to this file when the user opts in (FR-62).

## 10. Performance targets and benchmarking

Reference machine: M-series MacBook with an internal SSD (the development machine is an M4 Pro with 12 cores and 24 GB RAM). These are **initial targets**; Iteration 8 checks and adjusts them.

| Benchmark | Target |
|-----------|--------|
| Walk only, 1M entries (mixed files) | < 10 s |
| Scan + metadata, 100k media files (≈ 90% photos, 10% videos; warm cache) | < 30 s. Video metadata must cost about the same as photo metadata, because only `moov` is read. |
| Rescan unchanged 100k media files | < 5 s |
| Exact-dupe pass, 100k media files / 300 GB, 10% dupes | Limited by disk read speed for candidate files only |
| Peak RSS, 1M-file index | < 250 MB |
| TUI frame time with 1M-row index | < 16 ms (fetch only the visible page) |

**Tools:**
- `criterion` microbenchmarks: hashing, extractors, template rendering.
- `tools/fixturegen` generates reproducible trees: N files, a chosen duplicate ratio, nested depth, mixed formats, some corrupted files. It also writes **synthetic videos**: valid ISO-BMFF `ftyp` + `moov` (`mvhd`, `tkhd`, `mdhd`, and Apple metadata keys) with a padded `mdat`, with `moov` placed both before and after `mdat`. It also writes **Live Photo pairs** that share a content identifier. No ffmpeg is needed.
- `hyperfine` for end-to-end CLI timing.
- `/usr/bin/time -l` for peak RSS.

## 11. Testing strategy

| Level | What |
|-------|------|
| Geo | Known coordinates → expected place (Goa, Paris, Tokyo, a mid-ocean point, the poles, near the antimeridian, a border town); ISO 6709 parsing; precision-guard thresholds; inference window edges |
| Unit | Detection, each extractor (against fixture images **and a few real iPhone, Android and camera clips** in `tests/data/`, kept small, under 1 MB each), Live Photo pairing, name normalization, keep rules, template parser, glimpse algorithm |
| Property (`proptest`) | Plan invariants (§6.5): no overwrite, no full-group deletion, never split a Live Photo pair, undo(apply(plan)) restores the original tree |
| Integration | `tempfile` trees: scan → dupes → plan → apply → verify → undo → verify the tree hash matches the original |
| CLI | `assert_cmd` + `insta` snapshots of `stats`, `dupes`, and `plan show` output |
| TUI | `ratatui::backend::TestBackend` + `insta` buffer snapshots for each screen, plus key-sequence tests driving `update()` |
| Fault injection | Kill mid-apply (spawn child, SIGKILL), then `--resume` and `undo` must both succeed; read-only destination; disk full simulated with a small APFS disk image (`hdiutil create -size 50m`) |
| Manual | Real-world libraries: phone export, external HFS+/exFAT drive, iCloud Drive folder with dataless files |

CI runs on GitHub Actions `macos-14` (arm64) or later: `fmt`, `clippy -D warnings`, `test`, `cargo-deny`, and a benchmark smoke test.

## 12. Error handling and logging
- The core returns `Result<T, YojakaError>` (`thiserror`). Per-file errors are **collected, never fatal**. They go to `scan_error` and the UI error list.
- Fatal errors are: the index can't be opened, the root is missing, or the journal can't be written. The journal case blocks execution.
- `tracing` writes to `~/Library/Logs/yojaka/yojaka.log` with daily rotation. Set the level with `YOJAKA_LOG=debug`.
- A panic hook restores the terminal (leaves the alternate screen and disables raw mode) before printing the panic.

## 13. LLM integration (design only; Iteration 9+)

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn is_local(&self) -> bool;
    fn complete(&self, req: CompletionRequest) -> Result<String, LlmError>;
    fn describe_image(&self, img: &Path, prompt: &str) -> Result<String, LlmError>; // vision models
}
```

**Providers:**
- **Ollama:** HTTP `localhost:11434`. Ollama is already installed on this machine.
- **Apple Foundation Models:** on-device. Needs a small Swift helper binary or an FFI bridge, because the framework is Swift-only.
- **Claude** and **OpenAI:** HTTPS. Opt-in, with the API key read from the Keychain or an environment variable.

**Use cases, in priority order:**
1. Event and folder naming suggestions for organize clusters. Only text metadata is sent: dates, place counts, camera.
2. Natural-language filters translated into SQL `WHERE` clauses over the index. The query is validated against an allow-list of columns.
3. Best-shot pick among near-duplicates (vision).
4. Tagging and captions (vision, local only by default).

Privacy rule: no image bytes or paths leave the machine unless `allow_cloud = true` **and** the user confirms for that session.

## 14. GUI path (later)
`yojaka-core` is already free of UI code, so a GUI can call it directly. Options:
- **Tauri:** Rust backend with a web front end. Small binary, quickest to build.
- **Native SwiftUI:** through `swift-bridge` or `uniffi`. Best Mac feel.

The decision is deferred. The core must expose plain, serializable types (all `serde`) to keep both options open.

## 15. Distribution
- `cargo build --release` with `lto = "fat"`, `codegen-units = 1`, `strip = true`, and `panic = "abort"` (after the panic hook runs).
- GitHub Releases provide the `aarch64-apple-darwin` tarball, codesigned and notarized (needs an Apple Developer ID).
- Homebrew tap later: `brew install <tap>/yojaka`.
