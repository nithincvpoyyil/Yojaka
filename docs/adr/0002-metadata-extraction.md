# ADR-0002: Metadata extraction crates

Status: accepted (Iteration 2)

## Context
Iteration 2 reads EXIF from photos and container metadata from videos. The hard requirement is that video metadata must **seek**: a 4 GB MOV or MP4 may hold `moov` after `mdat`, and reading the file to reach it would turn a scan into a full disk read. The acceptance test is "metadata for a 4 GB video reads less than 1 MB".

Candidates from the tech spec: `nom-exif`, or `kamadak-exif` + `mp4parse`.

## Decision
- **Images:** `kamadak-exif` (JPEG, TIFF and TIFF-based RAW, HEIF/AVIF, PNG, WebP) over a capped head of the file (1 MiB for JPEG/PNG/GIF/WebP/BMP, 8 MiB for TIFF/RAW/HEIC/AVIF), plus `imagesize` for dimensions. APNG, GIF and animated WebP frame counts come from small chunk walkers in `meta/image.rs`.
- **Videos:** our own ISO-BMFF walker (`meta/isobmff.rs`) and a minimal EBML walker (`meta/matroska.rs`). They read an 8–16 byte header per box, seek past everything unneeded, and read only leaf boxes (`mvhd`, `tkhd`, `mdhd`, `hdlr`, 16 bytes of `stsd`, 12 bytes of `stsz`, `keys`/`ilst`, `©xyz`). The frame rate comes from the `stsz` sample count, so sample tables are never read.
- **AVI and MTS:** `exiftool -json` in batches when installed. Optional for other failures (`--exiftool-fallback`).
- Dropped from the workspace: `nom-exif`, `mp4parse`, `kiddo`.

## Why not the crates
- `mp4parse` reads the whole `moov` into memory (including sample tables, which can be tens of MB) and exposes no read budget.
- `nom-exif` would replace `kamadak-exif` and `mp4parse` with one dependency, but we could not bound its reads either, and it would not have parsed the Apple `mdta` keys we need (creation date with timezone, content identifier, ISO 6709 location) without extra work.
- The walkers are about 400 lines in total, are covered by byte-counting tests and by truncation tests (every cut point of a valid file must give an error or valid metadata, never a panic), and add no dependencies.

## Consequences
- Dates are wall-clock times as recorded (no timezone applied). QuickTime `creationdate` wins over `mvhd` (UTC, often unset), and unset or pre-1970 values count as missing.
- Real-world coverage is limited to what we have tested: 53 real Sony JPEGs plus synthetic iPhone-style MOV, Android-style MP4, `moov`-last and Matroska files. iPhone HEVC, Android, RAW/DNG and Live Photo **samples are still wanted** (see `tests/data/README.md`).
