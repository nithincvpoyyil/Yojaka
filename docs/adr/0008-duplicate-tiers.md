# ADR-0008: Likely, name and similar duplicate tiers

Status: accepted (Iteration 7)

## Context
The exact tier (Iteration 4) finds files with identical bytes. Iteration 7 adds the three tiers of
FR-30 that find *different* files which may be the same picture or clip. These are guesses, so the
iteration plan says they must never be applied in bulk without a person looking at each group.
Measured results are in `docs/dupes-accuracy.md`.

## Decision
- **One search, four tiers** (`dupes::find`, `DupOptions`). The exact tier always runs, because the
  other tiers must know which files are already identical; `DupOptions` decides which tiers are
  *reported*. Order of confidence: exact, likely, similar, name.
- **Likely** (`dupes::tiers`) reads only the index. Photos: same date taken (source `exif` only),
  camera model, width and height. Videos: same recorded date, width and height, and lengths within
  1 s of the shortest in the run. Photos and videos are never mixed.
- **Name**: same kind and same normalized name (`dupes::name`: NFC, lower case, extension removed,
  then ` (n)`, `(n)`, ` copy`, ` copy n`, ` - copy`, `-n`, `_n` with a single digit), and sizes within
  5% of the smallest in the run. A name is never normalized to nothing.
- **Similar** (`dupes::similar`), photos only, **opt-in** (`--similar`, `F` in the app) because it is
  the only stage that decodes pixels. Each picture gets a 64-bit difference hash (9×8 grey cells) and
  a colour signature (average red, green, blue of each quarter, 5 bits each). Two pictures match when
  at most `dupes.similar_distance` bits differ (default 6, `--similar-distance`), every colour average
  is within one step of 32, and width over height agrees within 3%. Matches are joined into groups
  with a BK-tree and union-find.
  - The colour signature was added after the first measurement: without it, pictures with the same
    light-and-dark layout but other colours matched (precision 0.7% on the fixture; 88% with it).
  - Decoding uses the `image` crate for JPEG, PNG, GIF (first frame), WebP, TIFF and BMP, and macOS
    ImageIO for HEIC, AVIF and raw photos (DNG, CR2, CR3, NEF, ARW, RAF, ORF) (`sys::imageio`,
    added 2026-11-10; see "HEIC, AVIF and raw" below). The decoder is chosen from the indexed
    format (`similar::decoder`, an exhaustive match), never from the file's first bytes: most raw
    files are TIFF inside and would otherwise be read as their small thumbnail.
  - Every picture is turned upright before it is measured (EXIF orientation; the HEIC rotation).
    iPhones store a portrait photo sideways with a rotation flag, and the copies people send on are
    usually upright, so without this they would never match. The shape check compares long side
    over short side from the saved width and height (which are as stored), plus one bit measured
    on the upright picture that says whether it is portrait (bit 60 of `color_signature`). The saved
    size of a raw photo is not used. Raw sizes were wrong before 2026-11-11 (a Nikon NEF got its
    thumbnail's size; Fuji, Canon CR3 and Olympus files none); they are read through ImageIO now
    (ADR-0002 addendum), but the EXIF fallback and older saved data can still hold such sizes.
  - How a picture is measured has a version (`LOOK_VERSION`, kept in `meta_kv` as
    `picture_look_version`). When it changes, every saved hash is dropped once and made again.
  - A hash with fewer than 3 bits set or unset (a blank picture) is not compared.
  - Both numbers are saved in `file.perceptual_hash` and `file.color_signature`; a rescan clears them
    when the size or modification time changes, like the content hashes.
- **Shared rules for the three tiers.** Every candidate is `stat`ed before it is shown; a file changed
  since the scan is reported and left out. Hard links count once. Files that are identical to each
  other (same exact group) count once. A group whose members are all inside a group of a stronger
  tier is not shown again. Cloud-only files are never opened or listed.
- **Live Photos.** A pair is one item, listed under its photo, and only when both halves are
  unchanged. The video half is never matched on its own.
- **No bulk marking.** `DupGroup::apply_rules` and `DupReport::apply_rules` do nothing to a group that
  is not exact. Marks in the other tiers come only from `set_keeper` / `toggle`, which the app calls
  for the group under the cursor. The CLI lists those groups as `review` and never plans them.
- **Plans.** `DupRef` gains `tier` (absent in old plan files, read as `exact`) and `kept_expect`. For a
  non-exact removal the kept copy is a different file, so the executor checks it against its own
  fingerprint (size, modification time, and hash when known) instead of against the removed file. A
  plan without `kept_expect` for such an op is refused per file.
- **Overlapping groups.** With several tiers one file can be in two groups. `Plan::from_dupes` removes
  a file at most once, never removes a file another group keeps, and leaves out a group whose kept
  copy an earlier group removes (with a warning). `check_plan` still refuses any plan where a path is
  both removed and kept.

## Consequences
- An index made before this iteration lacks `color_signature`. The similar tier says so and asks for
  a scan from scratch (`--force`); the other tiers work on an old index. No migration was added
  (pre-release rule: the original schema file was edited).
- The first similar search reads every supported picture in full (about 6 ms per 320×240 fixture
  picture; real photos are larger and slower). Later searches read only new and changed pictures.
- The BK-tree is exact, but its queries slow down as the library grows. It has not been measured at
  1M pictures; that belongs to the performance work of Iteration 9 (planned for 8 when this was written).
- Burst shots (same second, same camera) land in the likely tier, and counter collisions
  (`IMG_0001.jpg` from two cameras) in the name tier. That is expected and is why a person reviews.
- A copy whose pixels were turned (not just flagged), cropped or colour-edited is not found by the
  similar tier.
- Colours are not converted to one colour space, by either decoder. A Display P3 photo and an
  sRGB copy of it can differ by more than one colour step in strongly coloured quarters and then
  do not match.

## HEIC, AVIF and raw (added 2026-11-10)
Options considered for decoding HEIC (the same reasoning covers AVIF and raw):
- **`sips`** (macOS command line). Not deprecated: it ships in `/usr/bin` on macOS 27 and its manual
  has no deprecation notice. But it is a process per picture and writes a converted file to disk,
  and it is itself a front end to ImageIO.
- **`libheif` (`libheif-rs`)**: popular and cross-platform, but a C library under LGPL-3.0 with an
  LGPL HEVC decoder (`libde265`). Linking it into an MIT binary, or asking users to install it with
  Homebrew, is a cost Yojaka (macOS only) does not need.
- **Pure Rust (`heic` crate)**: AGPL or a paid licence. Not usable.
- **ImageIO, in process** (chosen): Apple's own decoder, public since macOS 10.4 and not deprecated,
  the same one Photos and Preview use, with hardware HEVC decoding on Apple Silicon. Bindings:
  `objc2-image-io` and `objc2-core-graphics` (Zlib / Apache-2.0 / MIT), from the same `objc2`
  project as the `objc2-foundation` already used for the Trash.

How it is used: ImageIO opens the file by path, read-only, and reads only what it needs (cloud-only
files are never opened). It is always told the file's type (`kCGImageSourceTypeIdentifierHint`):
without it, ImageIO took a Sony ARW for plain TIFF and failed, and read a Nikon NEF as its 160×120
thumbnail. `CGImageSourceCreateThumbnailAtIndex` applies the rotation and shrinks the picture to at
most 512 pixels on the longer side.
- **HEIC and AVIF**: decoded from the full image, never the small preview stored in the file (it
  can be stale after an edit, or letterboxed).
- **Raw**: the camera's own preview inside the file, when its longer side is at least 256 pixels
  and its proportions match the picture's (within 3%); otherwise the raw is developed. Developing
  is Apple's rendering, not the camera's: on the samples below its colours differed from the
  preview by up to 6 steps of 32 (Adobe RGB or Display P3 out, against sRGB), so a developed raw
  would often miss the JPEG the camera saved with it. It is also 10 times slower (80 to 280 ms).

The decoded picture is drawn into 8-bit RGB memory in its own colour space (sRGB when that is not
RGB), so the values match what the `image` crate gives for other formats. Each picture gets its
own autorelease pool. The bindings are `unsafe`, so `sys/imageio.rs` is the one module that
allows `unsafe` code.

Measured (M-series, release build, synthetic 12 MP picture): 36 ms a HEIC on one thread (68 ms for
the same picture as a JPEG through the `image` crate); about 47 HEIC pictures a second on all cores
(the hardware decoder is shared); memory flat over 600 pictures. A HEIC and the JPEG it was made
from hash 0 to 1 bits apart.

Raw, measured on seven CC0 samples from raw.pixls.us (Sony A7 III ARW, Nikon D750 NEF, Canon 5D IV
CR2, Canon R6 CR3, Fujifilm X-T3 RAF, Olympus E-M10 III ORF, iPhone 12 Pro DNG), each against the
camera's JPEG taken out of the file with exiftool: 12 to 29 ms a file; 0 to 1 bits apart and at
most 1 colour step; all seven pairs grouped end to end. Opening by path reads 1 to 5.6 MB of each
11 to 56 MB raw file (reading it whole had been 5 to 20 times more). AVIF: made from a JPEG with
`sips`, it matches that JPEG like HEIC does.

## Not done
- Inline thumbnails (`ratatui-image`): the plan marks them optional. The app shows a text comparison
  (size, pixels, date, length, camera) instead.
- Similar videos (keyframe hashing): after v1.0, as planned.
- JPEG, PNG and the other `image` crate formats are decoded at full size (a 30 MP JPEG is about
  90 MB in memory, and several are decoded at once). ImageIO could shrink them while decoding, as
  it does for HEIC; that changes their hashes slightly (raise `LOOK_VERSION`).
