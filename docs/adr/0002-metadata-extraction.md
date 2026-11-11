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
- Real-world coverage is limited to what we have tested: 53 real Sony JPEGs plus synthetic iPhone-style MOV, Android-style MP4, `moov`-last and Matroska files. iPhone HEVC, Android and Live Photo **samples are still wanted** (raw: see the addendum) (see `tests/data/README.md`).

## Addendum (2026-11-11): raw photos through ImageIO
Seven real raw files (one per format, CC0 samples from raw.pixls.us, listed in
`docs/dupes-accuracy.md`) showed that `kamadak-exif` over the 8 MiB head is not enough for raw:

| Format | Before | With ImageIO |
|---|---|---|
| Fuji RAF, Canon CR3, Olympus ORF | "malformed metadata: Unknown image format": no size, date, camera or place | all read |
| Nikon NEF | size 160×120 (the thumbnail in IFD0) | 6016×4016 |
| Sony ARW, Canon CR2, DNG | right | same values |

Decision: raw formats (`Format::is_raw`) are read by macOS ImageIO (`CGImageSourceCopyPropertiesAtIndex`,
`meta/raw.rs`, `sys/imageio.rs`): size, orientation, date (taken, else digitized, else the file's),
make, model, lens and GPS. The size is the one Finder shows and the camera's JPEG has (the developed
picture, not the sensor's edges). The EXIF reader stays as the fallback when ImageIO cannot read a
file (for example a camera too new for this macOS).

- ImageIO is given the whole file but reads only what it needs: 0.03 to 0.8 MB of the samples
  (the capped head was 8 MiB). With only the first 8 MiB in memory it found no size for CR2, CR3
  and DNG, so the head cap does not apply to raw.
- The file is opened by Yojaka with `O_NOFOLLOW`, as before, and ImageIO reads it through
  `/dev/fd/N`, which is that open file (`man 4 fd`): a symlink swapped in after the open is never
  followed. Checked with 210 raw files read in parallel (0.34 s): every file got its own values.
- Saved data from before is fixed without a scan from scratch: `meta_kv.raw_meta_version` (now
  `1`); when it differs, raw files are set back to `pending` and read again on the next scan.
