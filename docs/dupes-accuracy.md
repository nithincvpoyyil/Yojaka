# Duplicate tiers: precision and recall

Measured 2026-11-10 (Iteration 7 acceptance). Design: `docs/adr/0008-duplicate-tiers.md`.

## How to reproduce

```sh
cargo nextest run -p yojaka-core -E 'test(tiers_precision_and_recall_on_the_fixture)' --no-capture
```

The test (`crates/yojaka-core/tests/dupe_tiers.rs`) builds a fixture with
`files: 400, variants: 120, videos: 0.1, live_photos: 8` (seed 42), scans it, runs each tier on its
own, and compares the groups with the manifest. It fails if a number drops below the floors at the
end of this page.

## The fixture

805 files are indexed. `variants: 120` adds 120 sets, 20 of each kind:

| Set | Files | A true copy? | Tier meant to find it |
|---|---|---|---|
| re-encoded | `PIC_n.jpg` + the same picture at JPEG quality 60, camera data kept | yes | likely, similar |
| resized | `PIC_n.jpg` + the picture at half size, camera data kept | yes | similar |
| stripped | `PIC_n.jpg` + the picture as PNG without camera data, and an unrelated `PIC_n-1.jpg` | PNG yes, `-1` no | similar |
| renamed | `PIC_n.jpg` + `PIC_n copy.jpg`, re-saved without camera data | yes | name, similar |
| burst | `PIC_n.jpg` + another picture with the same second, camera and pixels | no | (decoy for likely) |
| clip | `CLIP_n.mp4` + a re-export 0.4 s longer, and another clip 5 s longer | re-export yes, longer no | likely |

The pictures of these sets are 320×240 with detail at several scales. The other 400 base files are
64×48 plain colour ramps, which is a hard case for anything that compares pictures.

A pair counts as **correct** when the manifest says both files are the same picture or clip
(`expected.same_content`), or the same bytes. **Recall** is counted over the copies a tier is meant
to find (last column above).

## Result

| Tier | Groups | Pairs reported | Correct | Precision | Meant to find | Found | Recall |
|---|---|---|---|---|---|---|---|
| likely | 62 | 62 | 40 | 0.645 | 40 | 40 | 1.000 |
| similar (6 bits) | 89 | 91 | 80 | 0.879 | 80 | 80 | 1.000 |
| name | 42 | 42 | 20 | 0.476 | 20 | 20 | 1.000 |

The exact tier is checked separately: its groups equal the manifest's byte-identical groups, with
or without the other tiers switched on (`groups_match_the_fixture_manifest_exactly`,
`the_exact_tier_has_no_false_positives_with_every_tier_on`).

## What the wrong pairs are

- **likely, 22 wrong**: the 20 burst decoys, and 2 Live Photo decoys of the base fixture (two
  different pictures written with the same second and camera). Index data alone cannot tell these
  from a re-saved copy.
- **similar, 11 wrong**: all among the 64×48 colour ramps of the base fixture, where two files have
  the same ramp and nearly the same colours. No pair of the detailed pictures was matched wrongly
  (the test asserts this).
- **name, 22 wrong**: the 12 `PIC_n-1.jpg` decoys whose size happens to be within 5% (8 of the 20
  were rejected by the size check), and 10 `IMG_n (2).jpg` decoys of the base fixture.

So on this fixture the likely and name tiers are right about half to two thirds of the time. That is
the reason no rule may mark their groups and the command line never plans them.

## The colour check

The first version of the similar tier compared only the difference hash. On this fixture it
reported 11,197 pairs with 80 correct (precision 0.007): hundreds of ramps share one light-and-dark
layout. Adding the colour signature brought it to the 91 pairs above with the same recall.

## Speed (M-series, 12 cores, release build)

| What | Time |
|---|---|
| likely + name on the 805-file fixture | under 0.05 s |
| similar, first run, 692 small pictures | 0.03 s |
| similar, first run, 17 pictures of 6000×4000 (24 MP JPEG) | 0.31 s wall, 2.3 s CPU (about 140 ms of CPU a picture) |
| similar, 12 MP HEIC (synthetic), ImageIO | 36 ms a picture on one thread; about 47 a second on all cores |
| similar, raw photo (camera preview), ImageIO | 12 to 29 ms a file; 1 to 5.6 MB read of 11 to 56 MB |
| similar, second run | no picture is read |

At that rate a first similar search over 10,000 camera pictures takes roughly 3 minutes on this
machine. The BK-tree grouping has not been measured on large libraries (Iteration 8).

## Limits of this measurement

- The fixture pictures are synthetic. No real HEIC or phone-messenger copies were available.
- The fixture's look-alike sets are JPEG and PNG only. HEIC and AVIF are covered by
  `similar_compares_heic_and_avif_and_turns_sideways_photos_upright` and the unit tests in
  `dupes/similar.rs`.
- Raw photos were measured on seven real files (below), not on the fixture.
- A copy whose pixels were turned (not just flagged with an orientation), cropped or colour-edited
  is not found by the similar tier (none are in the fixture).

## Floors asserted by the test

Recall 0.99 for every tier; precision 0.85 (similar), 0.60 (likely), 0.45 (name); no wrong similar
pair among the detailed pictures.

## Raw photos (2026-11-10)

Seven CC0 samples from [raw.pixls.us](https://raw.pixls.us/data/), one per raw format Yojaka knows.
Each was compared with the camera's JPEG taken out of the file (`exiftool -b -JpgFromRaw`, else
`-PreviewImage`, with the raw's orientation copied over), standing in for the JPEG a camera saves
next to its raw file.

| File (raw.pixls.us path) | Read | vs camera JPEG |
|---|---|---|
| `Sony/ILCE-7M3/_DSC0010.ARW` | 12 ms | 0 bits, colours equal |
| `Nikon/D750/compressed_12_bit.NEF` | 16 ms | 0 bits, colours equal |
| `Fujifilm/X-T3/AFXT2720.RAF` | 16 ms | 0 bits, 1 colour step |
| `Canon/EOS R6/Canon_EOS_R6_CRAW_ISO_100_nocrop_nodual.CR3` | 21 ms | 0 bits, colours equal |
| `Apple/iPhone 12 Pro/IMG_1361.DNG` (portrait) | 21 ms | 1 bit, 1 colour step |
| `Olympus/E-M10 Mark III/PA080192.ORF` | 24 ms | 1 bit, 1 colour step |
| `Canon/EOS 5D Mark IV/B13A0732.CR2` | 29 ms | 1 bit, 1 colour step |

All seven pairs are grouped by `yojaka dupes --tier similar` (0.24 s for the 14 files, cold). To
run this again: put each raw file next to its camera JPEG (`X.NEF` + `X.JPG`) in a folder and run
`YOJAKA_RAW_SAMPLES=<folder> cargo nextest run -p yojaka-core --run-ignored only -E 'test(raw_photos)'`.

Apple's own development of the same files (not used) took 80 to 280 ms and its colours differed
from the camera's by up to 6 steps, so it would have missed most camera JPEGs.

Limits: one file per format and camera; no camera JPEG straight from a card (the stand-in is the
camera's own rendering, but may be smaller or more compressed); raw files from cameras ImageIO does not
support yet (very new models) may count as "could not be read as pictures".
