# Using Yojaka

Yojaka indexes, de-duplicates and organizes photo and video folders on macOS (Apple Silicon). You can use it
two ways:

- **The app:** `yojaka ~/Pictures` opens a full-screen terminal app with six screens (Actions, Dashboard,
  File browser, Duplicates, Organize, History).
- **The commands:** `yojaka scan`, `yojaka dupes`, `yojaka organize` and the rest, for scripts or when you
  prefer plain output.

Both use the same saved data, so a folder scanned with one is ready in the other.

This page covers version 0.0.1. Every example below was run on a test folder. Paths are shown as
`~/Pictures/...`, and long output is cut where you see `…`.

---

## Contents

1. [How Yojaka keeps your files safe](#1-how-yojaka-keeps-your-files-safe)
2. [Install](#2-install)
3. [Quick start](#3-quick-start)
4. [Command overview and global options](#4-command-overview-and-global-options)
5. [Opening the app](#5-opening-the-app)
6. [Commands](#6-commands)
   [scan](#scan) · [stats](#stats) · [info](#info) · [places](#places) · [export](#export) ·
   [dupes](#dupes) · [organize](#organize) · [plan show](#plan-show) · [apply](#apply) ·
   [history](#history) · [doctor](#doctor)
7. [Folder patterns (templates)](#7-folder-patterns-templates)
8. [Keep rules (which copy stays)](#8-keep-rules-which-copy-stays)
9. [Step-by-step recipes](#9-step-by-step-recipes)
10. [Configuration file](#10-configuration-file)
11. [Files Yojaka keeps](#11-files-yojaka-keeps)
12. [Exit codes](#12-exit-codes)
13. [File formats](#13-file-formats)
14. [Troubleshooting](#14-troubleshooting)

---

## 1. How Yojaka keeps your files safe

- **Read-only by default.** `scan`, `stats`, `info`, `places`, `export`, `dupes`, `organize` and
  `plan show` never change your photos or videos. They only write Yojaka's own saved data.
- **Every change is a plan first.** `dupes` and `organize` write a plan file. `apply` shows it as a dry
  run. Only `apply --write` carries it out, and it asks you first.
- **Delete means the Trash.** Duplicates go to the macOS Trash, and you can put them back from Finder.
  Deleting for good needs `--permanent` and a typed phrase.
- **There is no undo.** A move or a copy is final, so check the dry run before you confirm.
- **Files are never overwritten.** A name clash becomes `IMG_0001 (1).jpg`. A plan or export file that
  already exists is refused.
- **Files are checked again before anything happens.** Before each step, `apply` checks the size, the
  date and the contents of each file. A file that changed since the scan is skipped and listed.
- **Live Photos stay together.** A Live Photo's picture and its video are always moved, copied or
  trashed together.
- **Offline.** Place names come from a database built into Yojaka. No coordinates leave your Mac, and
  exports leave GPS out unless you ask for it.
- **iCloud-only files are never downloaded.** Files that are only in iCloud are listed but never
  opened. Photos libraries (`*.photoslibrary`) are skipped.

## 2. Install

Yojaka is built from source for now. A Homebrew tap and a download come with v1.0.

```sh
git clone https://github.com/nithincvpoyyil/Yojaka.git
cd Yojaka
rustup toolchain install          # uses rust-toolchain.toml
cargo build --release -p yojaka-cli
cp target/release/yojaka /usr/local/bin/   # or anywhere on your PATH
yojaka doctor                     # checks the setup and prints Yojaka's folders
```

To scan folders that macOS protects (Desktop, Documents, external drives), give your terminal app
**Full Disk Access** in System Settings → Privacy & Security → Full Disk Access.

## 3. Quick start

```sh
# Open a folder in the app. A folder Yojaka has not seen yet is scanned first.
yojaka ~/Pictures/Holiday

# Or use plain commands:
yojaka scan ~/Pictures/Holiday                       # 1. index the folder
yojaka stats ~/Pictures/Holiday                      # 2. what is in it
yojaka dupes ~/Pictures/Holiday                      # 3. identical copies (preview only)
yojaka dupes ~/Pictures/Holiday --plan-out dupes.json
yojaka apply dupes.json                              # 4. dry run
yojaka --write apply dupes.json                      # 5. move the extra copies to the Trash

# Copy everything into year / month folders somewhere else:
yojaka organize ~/Pictures/Holiday ~/Pictures/Sorted --preset date --plan-out sort.json
yojaka --write apply sort.json
```

## 4. Command overview and global options

```text
yojaka [OPTIONS] [PATH]               open the app on PATH (default: the current folder)
yojaka [OPTIONS] <COMMAND> [ARGS]     run one command and exit
```

| Command | What it does | Changes files? |
|---|---|---|
| `scan [PATH]` | Index a folder. Only new and changed files are read. | No |
| `stats [PATH]` | Totals, formats, years, cameras, places, largest files | No |
| `info <FILE>` | Everything Yojaka knows about one file | No |
| `places [PATH]` | Countries and cities found | No |
| `export [PATH]` | The file list as CSV or JSON | No (writes only the file you name) |
| `dupes [PATH]` | Find duplicates. Can save a plan. | No |
| `organize <PATH> <DEST>` | Work out a new folder layout. Can save a plan. | No |
| `plan show <FILE>` | Show a saved plan | No |
| `apply <FILE>` | Run a plan: dry run, or with `--write` for real | **Only with `--write`** |
| `history` | Plans that were run, newest first | No |
| `history delete` | Clear the list of finished runs | No (only Yojaka's own records) |
| `doctor` | Check the setup and print Yojaka's folders | No |

**Options that work with any command.** They can go before or after the command name.

| Option | Meaning |
|---|---|
| `--write` | Allow changes. Only `apply` (and the app) use it, and they still show a dry run and ask first. |
| `--debug` | Write a detailed log (threads, steps, timings, errors). The log's file name is printed at start. Without this flag, no log is written. |
| `--threads <N>` | Number of worker threads. The default (`0` in the config) means one per CPU core. |
| `-h`, `--help` | Help. `yojaka <command> --help` gives help for one command. |
| `-V`, `--version` | Version, commit, build date and target |

**Options for opening the app.** Put these before the folder, not after a command.

| Option | Meaning |
|---|---|
| `--refresh` | Scan on open if the last scan is more than an hour old. It never clears anything. |
| `--force` | Ask first, then forget what is saved about the folder and scan it from scratch (slower) |
| `--about` | Print the license and the credit for the place-name data, then exit. Creates nothing on disk. |

## 5. Opening the app

```sh
yojaka                          # the current folder
yojaka ~/Pictures/Holiday       # a folder
yojaka /Volumes/Backup/Photos   # a folder on an external drive
yojaka --refresh ~/Pictures     # scan again if the last scan is over an hour old
yojaka --force ~/Pictures       # asks y/n; on y, forgets the saved data and scans from scratch
yojaka --write ~/Pictures       # start in write mode (the app asks anyway before any change)
```

- A folder with no saved data opens on **Actions** and scans by itself. An already scanned folder opens
  on the **Dashboard** as it was last scanned. Press `r` to look for changes.
- The header shows `[READ-ONLY]` until write mode is on. You don't have to turn it on yourself: a
  review asks "Turn on write mode?" when it needs to.
- Press `?` for the keys of the screen you are on.

**Main keys**

| Where | Keys |
|---|---|
| Everywhere | `1`-`6` or `<` `>` switch screens · `T` pick a screen with `←` `→` · `r` scan for changes · `R` scan from scratch (asks) · `c` cancel a scan · `?` help · `q` quit · `W` write mode on / off · `D` debug overlay |
| Actions | `↑` `↓` choose a job or a choice · `Enter` go on · `Esc` back one step · `y` confirm (a move, or 50 files or more: type the phrase) · `Ctrl-C` stop after the current file |
| File browser | `Tab` folders / files · `Enter` open a file in its default app · `/` filter by name, camera or place · `t` all / photos / videos · `s` `S` sort / reverse · `E` export the files shown to CSV |
| Duplicates | `f` find · `F` also compare how pictures look · `v` show one kind of group · `Space` trash / keep a copy · `Enter` keep only this copy · `p` next keep rule · `x` keep every copy · `a` review and run · `P` save the plan · `M` remember the keep rule |
| Organize | `↑` `↓` choose a setting · `Enter` change it · `←` `→` previous / next choice · `a` review and run · `P` save the plan · `M` remember the pattern and copy / move |
| History | `↑` `↓` move · `d` delete the history of finished runs (asks) |

In text boxes, `Enter` keeps the text, `Esc` undoes the edit, and `Ctrl-U` clears the box. `~` means your
home folder.

## 6. Commands

### scan

Index a folder: file names, sizes and dates, then the details inside each file (date taken, camera, size
in pixels, video length, GPS) and Live Photo pairs. Coordinates are turned into place names offline. A
second scan reads only new and changed files, and forgets files that are gone.

```text
yojaka scan [PATH] [--kind image|video|all] [--sniff-all] [--force] [--no-metadata]
```

| Option | Meaning |
|---|---|
| `PATH` | Folder to scan (default: the current folder) |
| `--kind <KIND>` | `image`, `video` or `all` (default). See the warning below. |
| `--sniff-all` | Look inside files with unknown extensions too, to find photos and videos without one (slower) |
| `--force` | Forget what is saved about this folder and scan it from scratch. Does not ask. |
| `--no-metadata` | Index the files only: skip the details inside files, Live Photo pairing and places (fastest) |

> **Warning about `--kind`:** after `scan --kind video`, Yojaka's saved data holds only videos. Every
> photo is taken out of it, and so are the saved comparison results for photos. Your files are not
> touched. Run `yojaka scan` without `--kind` to bring them back. That rescan reads the photos again.

```console
$ yojaka scan ~/Pictures/Holiday
Scanned /Users/you/Pictures/Holiday in 0.4s
  405 media files indexed (13.7 MB) from 606 files seen
  405 new or changed, 0 unchanged, 0 removed from the index
  metadata: 396 read, 5 Live Photo pairs, 152 places resolved (0.3s)
  9 files had unreadable metadata (listed below and in the log)
  9 issues:
    /Users/you/Pictures/Holiday/a/BROKEN_0009.mp4: malformed metadata: moov box is truncated
    /Users/you/Pictures/Holiday/a/IMG_0227.png: malformed metadata: Broken PNG file
    … and 7 more (see the log file)

$ yojaka scan ~/Pictures/Holiday          # again: nothing changed, nothing read
Scanned /Users/you/Pictures/Holiday in 0.0s
  405 media files indexed (13.7 MB) from 606 files seen
  0 new or changed, 405 unchanged, 0 removed from the index
  metadata: 0 read, 5 Live Photo pairs, 0 places resolved (0.0s)
```

More examples:

```sh
yojaka scan                                   # the current folder
yojaka scan /Volumes/Backup --no-metadata     # a quick count of a big drive
yojaka scan ~/Pictures --force                # scan from scratch (e.g. after "saved data is from an older Yojaka")
yojaka scan ~/Downloads --sniff-all           # also find photos saved without an extension
```

`Ctrl-C` stops a scan. What was found so far is kept, and the exit code is 3. Hidden folders and `.git`,
`node_modules`, `.Trash` and `Library` are skipped (see [the config file](#10-configuration-file)).

### stats

Show what is in a scanned folder.

```text
yojaka stats [PATH] [--json]
```

```console
$ yojaka stats ~/Pictures/Holiday
/Users/you/Pictures/Holiday
  last scan: complete (606 files seen, 0 issues)

  Photos        364    992.9 KB
  Videos         36     12.7 MB
  Total         400     13.7 MB

  By format
    jpeg       image        252    542.6 KB
    png        image         73    287.9 KB
    mov        video         18     96.5 KB
    mp4        video         18     12.6 MB
    …
  Video: 12m 10s in total (30 with known length)
    codecs: hevc 18, h264 12
    longest: …
    largest: …

  Live Photos: 5 pairs

  By year
    2022       36  ██████████████████████████████
    2023       21  ██████████████████
    2024       23  ████████████████████
    undated (no embedded date): 136
    …
  Top cameras
    Apple iPhone 15                   99
    Canon EOS R6                      72
    Google Pixel 8                    72

  Location: 149 with a place, 251 without (149 with GPS)
    India                             32
    Japan                             26
    Cape Town, South Africa           25
    …
  Largest files
        2.1 MB  /Users/you/Pictures/Holiday/a/LATE_0017.mp4
    …
  9 files have unreadable metadata (damaged or truncated)
  Errors: 0
  (hard links to the same file are counted once)
```

`--json` prints the same numbers as JSON, for scripts:

```sh
yojaka stats ~/Pictures --json | jq '.photos, .videos'
```

### info

Show everything Yojaka knows about one file, including its GPS position and place. It reads the file
itself, so the file doesn't need to be scanned first.

```text
yojaka info <FILE> [--json]
```

```console
$ yojaka info ~/Pictures/Holiday/2023/IMG_0085.jpg
File        /Users/you/Pictures/Holiday/2023/IMG_0085.jpg
Format      jpeg (1.6 KB)
Size        64 × 48
Taken       2017-07-07 20:32:06  (from exif)
Camera      Canon EOS R6
GPS         -33.92490, 18.42410
Place       Cape Town, Western Cape, South Africa (0.1 km to Cape Town)
```

`--json` gives every field (`format`, `size_bytes`, `taken_at`, `taken_at_src`, `width`, `height`,
`camera_make`, `camera_model`, `lens`, `orientation`, `duration_ms`, `fps`, `video_codec`, `audio_codec`,
`gps`, `place`, …).

### places

List the countries and cities found in a scanned folder.

```text
yojaka places [PATH] [--json]
```

```console
$ yojaka places ~/Pictures/Holiday
149 of 400 files have a place (37%); 149 have GPS

  Countries
    India (IN)                        32
    Japan (JP)                        26
    South Africa (ZA)                 25
    …
  Cities
    Panjim, Goa, India                            32
    Eifuku, Tokyo, Japan                          26
    Cape Town, Western Cape, South Africa         25
    …
  (place names come from an offline database; nothing leaves this machine)
```

### export

Write the list of scanned files as CSV (default) or JSON, to the screen or to a new file. GPS
coordinates are left out unless you add `--include-gps`.

```text
yojaka export [PATH] [--format csv|json] [--out FILE] [--include-gps]
              [--kind image|video|all] [--year YEAR] [--find TEXT] [--folder DIR]
```

| Option | Meaning |
|---|---|
| `--format <FORMAT>` | `csv` (default) or `json` |
| `--out <FILE>` | Write to this new file instead of the screen. A file that already exists is never overwritten. |
| `--include-gps` | Add the `gps_lat` and `gps_lon` columns |
| `--kind <KIND>` | `image`, `video` or `all` (default) |
| `--year <YEAR>` | Only files taken in that year |
| `--find <TEXT>` | Only files whose name, camera or place contains the text (like `/` in the File browser) |
| `--folder <DIR>` | Only files in this folder inside the scanned one. See the note below. |

Filters combine: each one narrows the list.

```sh
yojaka export ~/Pictures --out ~/Desktop/all-files.csv
yojaka export ~/Pictures --kind video --year 2022
yojaka export ~/Pictures --find "Cape Town" --format json
yojaka export ~/Pictures --find Pixel --year 2020 --out pixel-2020.csv
yojaka export ~/Pictures --folder ~/Pictures/Trips/Japan
yojaka export ~/Pictures --include-gps --out with-gps.csv    # coordinates included: keep the file private
```

```console
$ yojaka export ~/Pictures --out ~/Desktop/all-files.csv
Exported 400 rows to /Users/you/Desktop/all-files.csv
GPS coordinates were left out (use --include-gps to add them).
```

CSV columns: `path, kind, format, size_bytes, width, height, taken_at, taken_at_src, camera_make,
camera_model, lens, orientation, duration_ms, fps, rotation, video_codec, audio_codec, container, frames,
live_photo_pair, country_code, country, state, city, place_dist_km, location_source, metadata_status`, plus
`gps_lat, gps_lon` with `--include-gps`.

> **Note (0.0.1):** give `--folder` an absolute path. `~/...` works too, because the shell expands it. A
> relative path such as `--folder Trips/Japan` matches nothing today and exports 0 rows.

### dupes

Find duplicate files and show what could go to the Trash. **It changes nothing.** With `--plan-out`, it
saves that proposal as a plan for `apply`.

```text
yojaka dupes [PATH] [--tier exact|likely|name|similar|all] [--similar] [--similar-distance BITS]
             [--keep RULES] [--remember] [--plan-out FILE] [--json]
```

| Option | Meaning |
|---|---|
| `--tier <TIER>` | Which kind of match to look for (default `exact`; see below) |
| `--similar` | Also compare how pictures look, to find resized or re-saved copies. The first time, every picture is read once (HEIC, AVIF and raw photos included). After that, the results are saved. |
| `--similar-distance <BITS>` | How different two pictures may look and still match: `0` (the same) to `32` (very loose). The default is `6`. |
| `--keep <RULES>` | Which copy stays, as a comma-separated list of rules in order of preference. See [Keep rules](#8-keep-rules-which-copy-stays). The default is `oldest-modified,shortest-path`. |
| `--remember` | Save the `--keep` rules in the config file, so they are the default next time (needs `--keep`) |
| `--plan-out <FILE>` | Save the proposed removals as a plan file. An existing file is never overwritten. |
| `--json` | Machine-readable output |

**Kinds of match (tiers).** Only identical files are ever marked for the Trash. The other kinds are
guesses: they are listed with `review`, and you decide each group yourself in the app's Duplicates screen.

| Tier | Finds | Marked for the Trash? |
|---|---|---|
| `exact` | Files with the same bytes. Hard links (one file under two names) are counted once and use no extra space. | Yes, by the keep rules |
| `likely` | Photos with the same date taken, camera and size in pixels. Videos with the same date and size whose lengths are within 1 second. | No, you review them |
| `name` | The same name apart from a copy ending (` (1)`, ` copy`, `-1`, `_1`) and nearly the same size (within 5%) | No, you review them |
| `similar` | Pictures that look alike: resized, re-saved or converted (HEIC and JPEG, a raw photo and its JPEG) | No, you review them |
| `all` | `exact` + `likely` + `name`. Add `--similar` to include `similar`. | Only `exact` |

A Live Photo counts as one item. It only matches another Live Photo with the same picture **and** the
same video, never a lone photo.

```console
$ yojaka dupes ~/Pictures/Holiday
Group 1 · 2 copies · 5.4 KB each · frees 5.4 KB
  keep    /Users/you/Pictures/Holiday/IMG_0109.gif
  delete  /Users/you/Pictures/Holiday/misc/family/IMG_0109 copy.gif

Group 2 · 2 copies · 2.9 KB each · frees 2.9 KB
  keep    /Users/you/Pictures/Holiday/2023/IMG_0137.png
  delete  /Users/you/Pictures/Holiday/raw/IMG_0137 (1).png
…
31 groups of identical files; keeping one of each frees 68.2 KB (35 files).
Read 0 B to compare 199 candidate files (267 saved hashes reused).
3 extra names of the same file (hard links) were ignored: they use no extra space.
Preview only: nothing has been deleted. Add --plan-out FILE to save this as a plan.
```

```console
$ yojaka dupes ~/Pictures/Holiday --tier likely
Group 1 · likely · 2 files (same date, camera, pixels)
  review  /Users/you/Pictures/Holiday/PIC_0004.jpg
          16.6 KB · 320×240 · 2022-04-15 09:24:44 · Pixel 8
  review  /Users/you/Pictures/Holiday/PIC_0004_b.jpg
          14.8 KB · 320×240 · 2022-04-15 09:24:44 · Pixel 8
…
```

More examples:

```sh
yojaka dupes ~/Pictures                                   # identical files only
yojaka dupes ~/Pictures --tier all --similar              # every kind of match
yojaka dupes ~/Pictures --tier similar --similar-distance 4   # stricter look-alike match
yojaka dupes ~/Pictures --keep "prefer-path:$HOME/Pictures/Originals,oldest-taken"
yojaka dupes ~/Pictures --keep highest-resolution,largest-file --remember
yojaka dupes ~/Pictures --plan-out ~/Desktop/dupes.json   # save a plan for `apply`
yojaka dupes ~/Pictures --json > dupes.json               # a report for scripts (not a plan)
```

The first search reads files that share a size. Later searches reuse the saved results, so they read
almost nothing. `Ctrl-C` stops a search (exit code 3). A file that changed since the last scan is left out
and reported (exit code 3).

### organize

Work out how the scanned files would be arranged into folders in a destination: copied (the default) or
moved. **It changes nothing.** It prints a summary. With `--plan-out`, it saves the plan for `apply`.

```text
yojaka organize <PATH> <DEST> [--preset NAME | --template PATTERN] [--copy | --move]
                [--skip-identical] [--infer-location] [--remember] [--plan-out FILE] [--full-tree]
```

| Option | Meaning |
|---|---|
| `PATH` | The scanned folder to take files from (`.` for the current folder) |
| `DEST` | Where the files go. It is created if needed. It may not contain `..`. A copy may not go inside `PATH`. |
| `--preset <NAME>` | A ready-made folder pattern (table below) |
| `--template <PATTERN>` | Your own folder pattern, e.g. `{year}/{month_name}/{name}`. See [Folder patterns](#7-folder-patterns-templates). Can't be used with `--preset`. Without either, the pattern from the config file is used (`{year}/{month_name}/{name}` unless you changed it). |
| `--copy` | Copy: the originals stay where they are. This is the default unless the config file says move. |
| `--move` | Move instead of copy. Running it always needs a typed phrase. |
| `--skip-identical` | Copy or move identical files only once. The keep rules pick the one that goes, and the others stay where they are. |
| `--infer-location` | Give a file without GPS the place of a geotagged photo from the same camera taken within two hours. Such places are marked `≈` in the plan. |
| `--remember` | Save the folder pattern and the copy / move choice in the config file as the defaults |
| `--plan-out <FILE>` | Save the plan to this new file. An existing file is never overwritten. |
| `--full-tree` | List every new folder instead of a short overview |

**Presets**

| `--preset` | Pattern | Example result |
|---|---|---|
| `date` | `{year}/{month_name}/{name}` | `2023/April/IMG_0001.HEIC` |
| `year` | `{year}/{name}` | `2023/IMG_0001.HEIC` |
| `place` | `{country}/{city}/{name}` | `Japan/Kyoto/IMG_0001.HEIC` |
| `place-date` | `{country}/{city}/{year}-{month_name}/{name}` | `Japan/Kyoto/2023-April/IMG_0001.HEIC` |
| `year-place` | `{year}/{country}/{city}/{name}` | `2023/Japan/Kyoto/IMG_0001.HEIC` |
| `kind-date` | `{kind}/{year}/{month_name}/{name}` | `Photos/2023/April/IMG_0001.HEIC` |
| `merge` | `{kind}/{name}` | `Photos/IMG_0001.HEIC`, `Videos/IMG_0002.MOV` |
| `format` | `{kind}/{format}/{name}` | `Photos/HEIC/IMG_0001.HEIC` |

```console
$ yojaka organize ~/Pictures/Holiday ~/Pictures/Sorted --preset date --skip-identical --plan-out sort.json
Template: {year}/{month_name}/{name}
Plan 01a202f8-8258-746b-8c45-350d92315821
  from:    /Users/you/Pictures/Holiday
  to:      /Users/you/Pictures/Sorted
  action:  copy (originals stay) 365 files (13.6 MB)
  Live Photos: 5 pairs (both halves go to the same folder)
  identical: 40 files (77.1 KB) stay where they are; a file with the same bytes goes instead
  warning: 129 files have no recorded date; their modification date is used
  files by new folder:
    2017  +20 files (2.1 MB)
      February  +4 files (6.9 KB)
      March  +2 files (3.6 KB)
      …
    2018  +27 files (66.6 KB)
    …

Plan saved to sort.json. Nothing has been changed.
Run it with: yojaka --write apply sort.json
```

> **Note (0.0.1):** the short overview stops after about 40 lines without saying that more folders
> follow. The totals on the `action:` line are always complete. Use `--full-tree` (or `plan show
> --full-tree`) to see every folder.

More examples:

```sh
yojaka organize ~/Pictures ~/Pictures/Sorted --preset date            # preview only
yojaka organize ~/Pictures ~/Pictures/ByPlace --preset place --infer-location --plan-out place.json
yojaka organize ~/Downloads ~/Pictures/Inbox --preset merge --skip-identical --plan-out merge.json
yojaka organize ~/Pictures/Phone ~/Pictures/Library --move --template '{year}/{month:02}/{name}' --plan-out move.json
yojaka organize . /Volumes/Backup/Photos --preset year-place --full-tree
yojaka organize ~/Pictures ~/Pictures/Sorted --template '{camera}/{year}/{name}' --remember
```

Quote a `--template` with `'…'` so the shell leaves the `{}` alone.

### plan show

Show a saved plan: what it does, how many files, and the folders it touches.

```text
yojaka plan show <FILE> [--full-tree]
```

```console
$ yojaka plan show dupes.json
Plan 01a202f9-3719-72a7-997e-ef4a53746548
  from:    /Users/you/Pictures/Holiday
  action:  move to the Trash 35 files (68.2 KB) from 31 duplicate groups
  files removed, by folder:
    a  −7 files (11.8 KB)
    misc  −6 files (18.6 KB)
      family  −3 files (11.2 KB)
    raw  −5 files (8.3 KB)
    …
```

`--full-tree` lists every folder instead of the short overview.

### apply

Run a saved plan. **Without `--write` it is a dry run** and changes nothing. With `--write` it checks every
file again, asks you, then copies, moves or sends files to the Trash. Every step is recorded, so an
interrupted run can be finished later.

```text
yojaka [--write] apply <FILE> [--yes] [--resume] [--permanent]
```

| Option | Meaning |
|---|---|
| `--write` | Really do it. Without this flag, `apply` only shows the dry run. |
| `--yes` | Skip the y/n question of a copy or a small Trash plan. A move, a Trash plan of 50 files or more, and a permanent delete still need the phrase typed. |
| `--resume` | Finish a plan that was started and stopped (by `Ctrl-C`, a crash or a power cut) |
| `--permanent` | Delete for good instead of using the Trash. This can't be undone, and it always asks for a typed phrase, even with `--yes`. Only for duplicate plans. |

**How it asks**

| Plan | You confirm with |
|---|---|
| Copy | `y` |
| Trash, fewer than 50 files | `y` |
| Trash, 50 files or more | typing `trash 120 files` (with your number) |
| Move | typing `move 405 files` |
| Permanent delete | typing `delete 35 files forever` |

```console
$ yojaka apply sort.json
Plan 01a202f8-8258-746b-8c45-350d92315821
  …
Dry run: nothing was changed. Add --write to apply this plan.

$ yojaka --write apply sort.json
Plan 01a202f8-8258-746b-8c45-350d92315821
  …
Yojaka has no undo. Check the plan before you confirm.

Copy 365 files (13.6 MB)? (y/N): y
0 files moved, 365 copied, 116 folders created (13.6 MB).
40 identical files stayed where they are (one of each was copied).
```

```console
$ yojaka --write apply dupes.json
…
Yojaka has no undo. Files can be put back from the Trash in Finder.

Move to the Trash: 35 files (68.2 KB)? (y/N): y
35 files moved to the Trash (68.2 KB freed).
```

```console
$ yojaka --write apply move.json
…
Type "move 405 files" to continue: no
Not confirmed. Nothing was changed.
```

More examples:

```sh
yojaka apply plan.json                          # dry run
yojaka --write apply plan.json                  # run it (asks)
yojaka --write apply plan.json --yes            # a copy or a small Trash plan, without the y/n question
yojaka --write apply plan.json --resume         # finish an interrupted run
yojaka --write apply dupes.json --permanent     # delete for good (asks for "delete N files forever")
```

Rules:

- A plan runs only once. Running it again is refused. Use `--resume` only for a run that was stopped.
- A file that changed since the plan was made, or whose copy that stays has gone, is skipped and listed.
  The run then ends with exit code 3.
- `Ctrl-C` stops after the current file and prints the `--resume` command.

### history

List the plans that were run, newest first.

```text
yojaka history [--json]
yojaka history delete [--yes]
```

```console
$ yojaka history
Yojaka has no undo: a move or a copy is final.

01a202f9-3719-72a7-997e-ef4a53746548  2026-11-22 22:16  applied  35 of 35 files    68.2 KB  /Users/you/Pictures/Holiday
01a202f8-8258-746b-8c45-350d92315821  2026-11-22 22:16  applied  481 of 521 files  13.6 MB  /Users/you/Pictures/Holiday
```

`history delete` clears the records of finished runs. It asks first, and `--yes` skips the question. It
doesn't touch any photo or video, and it needs no `--write`. Unfinished runs are kept so `--resume` still
works, and a plan that was run still can't be run again.

```console
$ yojaka history delete
This deletes the history of 2 finished runs. Only Yojaka's own records go: no photo or video is touched.
A plan that was run still cannot be run again.

Delete the history of 2 finished runs? (y/N): y
Deleted the history of 2 finished runs.
```

### doctor

Check the setup and print Yojaka's folders, the tools it found, Full Disk Access and the default thread
count.

```text
yojaka doctor [--json]
```

```console
$ yojaka doctor
Yojaka 0.0.1 (bf21070) aarch64-apple-darwin
macOS 27.2

Paths
  config_file  /Users/you/Library/Application Support/yojaka/config.toml  [not created (defaults in use)]
  data         /Users/you/Library/Application Support/yojaka  [writable]
  index        /Users/you/Library/Application Support/yojaka/indexes  [writable]
  journal      /Users/you/Library/Application Support/yojaka/journal  [writable]
  logs         /Users/you/Library/Logs/yojaka  [writable]

Tools
  exiftool   13.55
  ffprobe    not found
  …
Full Disk Access: not granted
  To scan protected folders: System Settings → Privacy & Security → Full Disk Access → add your terminal.

CPU: 8 performance + 4 efficiency cores; default threads: 12
```

`yojaka --about` prints the license and the GeoNames credit. `yojaka --version` prints the version,
commit, build date and target.

## 7. Folder patterns (templates)

A pattern is a path with placeholders in `{}`. Each `/` starts a new folder, and the last part is the file
name, usually `{name}`. Text outside `{}` is kept as it is: `Photos {year}/{name}` gives
`Photos 2023/IMG_0001.jpg`.

| Placeholder | Becomes | Example |
|---|---|---|
| `{year}` | Year taken | `2023` |
| `{month}` | Month as a number. `{month:02}` pads it to two digits. | `4`, `04` |
| `{month_name}` | Month name | `April` |
| `{day}` | Day of the month (`{day:02}` pads it) | `7`, `07` |
| `{camera}` | Make and model, without repeating the make | `Apple iPhone 15`, `Canon EOS R6` |
| `{make}` | Camera make | `Apple` |
| `{model}` | Camera model | `iPhone 15` |
| `{format}` | File format, upper case | `HEIC`, `JPEG`, `MOV` |
| `{ext}` | Extension, lower case | `heic` |
| `{name}` | Original file name | `IMG_0001.HEIC` |
| `{stem}` | File name without the extension | `IMG_0001` |
| `{parent}` | Name of the folder the file is in now | `Trip to Kyoto` |
| `{kind}` | `Photos` or `Videos` | `Photos` |
| `{country}` | Country | `Japan` |
| `{country_code}` | Two-letter country code | `JP` |
| `{state}` | State or region | `Kyoto` |
| `{district}` | District | `Higashiyama` |
| `{city}` | City or town | `Kyoto` |

What happens when something is missing:

- **No date taken:** the file's modification date is used, and the summary warns about it. A file with no
  date at all goes to `_undated`.
- **No place:** the place folders become one `No Location` folder (change the name with
  `location.no_location_dir`). With `--infer-location`, a place may be borrowed from a nearby photo
  (marked `≈`).
- **Another empty value** (no camera, for example): that folder level is left out, so there are no empty
  folder names.
- **Names:** `/`, `:` and control characters become `_`, a leading `.` is removed, and very long names are
  shortened (keeping the extension).
- **Name clashes:** a file never overwrites another. The second one becomes `IMG_0001 (1).jpg`. A file whose
  destination folder already holds the same bytes, under any name, is skipped. So organizing the same
  folder twice doesn't make copies of copies.

Examples:

```sh
--template '{year}/{month:02}/{name}'                 # 2023/04/IMG_0001.HEIC
--template '{year}/{month:02}-{month_name}/{name}'    # 2023/04-April/IMG_0001.HEIC
--template '{camera}/{year}/{name}'                   # Apple iPhone 15/2023/IMG_0001.HEIC
--template '{country}/{year}/{stem}.{ext}'            # Japan/2023/IMG_0001.heic
--template '{kind}/{year}/{parent}/{name}'            # Photos/2023/Trip to Kyoto/IMG_0001.HEIC
```

An unknown placeholder is refused, and the error lists the valid ones (exit code 2).

## 8. Keep rules (which copy stays)

When identical files are found, the keep rules decide which copy stays. Rules are tried in order. Each
rule narrows the choice, and the next rule breaks a tie. If several copies are still tied at the end, the
shortest path stays.

| Rule | The copy that stays is the one… |
|---|---|
| `oldest-modified` (or `oldest`) | changed longest ago (**default, first**) |
| `newest-modified` (or `newest`) | changed most recently |
| `oldest-taken` | with the earliest date taken (a file without a date loses) |
| `shortest-path` | with the shortest full path (**default, second**) |
| `prefer-path:DIR` | inside `DIR`, if any copy is |
| `highest-resolution` | with the most pixels |
| `longest-duration` | with the longest video |
| `highest-bitrate` | with the highest video bitrate |
| `largest-file` | that is biggest on disk |
| `has-exif` | that has camera details (a date taken or a camera) |

```sh
yojaka dupes ~/Pictures --keep oldest-taken,shortest-path
yojaka dupes ~/Pictures --keep "prefer-path:$HOME/Pictures/Originals,oldest-modified"
yojaka dupes ~/Pictures --keep prefer-path:Originals          # relative: from the current folder
yojaka dupes ~/Pictures --keep has-exif,largest-file --remember
```

> **Note (0.0.1):** in `prefer-path:`, write `$HOME/...` (in double quotes) or a relative path, **not**
> `~/...`. The shell doesn't expand a `~` after `prefer-path:`, so the rule matches nothing and gives no
> warning.

Keep rules apply only to identical files. Look-alike groups (`likely`, `name`, `similar`) are never marked by
a rule.

## 9. Step-by-step recipes

### Remove identical copies from a folder

```sh
yojaka scan ~/Pictures
yojaka dupes ~/Pictures                                  # look first
yojaka dupes ~/Pictures --keep "prefer-path:$HOME/Pictures/Originals,oldest-modified" \
              --plan-out ~/Desktop/dupes.json
yojaka plan show ~/Desktop/dupes.json                    # which folders lose files
yojaka --write apply ~/Desktop/dupes.json                # to the Trash, asks first
yojaka history
```

Changed your mind? Open the Trash in Finder and choose **Put Back**.

### Copy a messy folder into tidy year / month folders

```sh
yojaka scan ~/Downloads/PhoneDump
yojaka organize ~/Downloads/PhoneDump ~/Pictures/Library --preset date --skip-identical \
                --plan-out ~/Desktop/sort.json
yojaka apply ~/Desktop/sort.json                         # dry run
yojaka --write apply ~/Desktop/sort.json                 # copy (asks y/n)
```

The originals stay where they are. When the copies look right, you can delete the old folder yourself.

### Move files into place-based folders

```sh
yojaka scan ~/Pictures/Travel
yojaka organize ~/Pictures/Travel ~/Pictures/ByPlace --preset place-date --move --infer-location \
                --plan-out ~/Desktop/move.json --full-tree
yojaka --write apply ~/Desktop/move.json                 # asks you to type "move N files"
```

### Merge several folders into one, without repeating identical files

```sh
yojaka scan ~/Pictures/Old
yojaka organize ~/Pictures/Old ~/Pictures/Merged --preset merge --skip-identical --plan-out merge.json
yojaka --write apply merge.json
```

### Finish a run that was stopped

```sh
yojaka history                                    # the run shows as not finished
yojaka --write apply ~/Desktop/sort.json --resume
```

### Make a spreadsheet of one year's videos

```sh
yojaka export ~/Pictures --kind video --year 2024 --out ~/Desktop/videos-2024.csv
```

### Save your usual choices

```sh
yojaka dupes ~/Pictures --keep oldest-taken,shortest-path --remember
yojaka organize ~/Pictures ~/Pictures/Sorted --preset kind-date --copy --remember
# next time, no --keep / --preset needed
```

## 10. Configuration file

Yojaka works without a config file. `--remember` (or `M` in the app) creates
`~/Library/Application Support/yojaka/config.toml`, and you can also edit it yourself. A missing key uses
the default. A bad value is ignored with a warning.

```toml
threads = 0                         # worker threads; 0 = one per CPU core

[scan]
sniff_all = false                   # like --sniff-all on every scan
skip_dirs = [".git", "node_modules", ".Trash", "Library"]   # folder names never scanned
skip_hidden = true                  # skip hidden files and folders
exiftool_fallback = false           # when the built-in reader fails on a file, try exiftool (if installed)

[dupes]
keep_rules = ["oldest-modified", "shortest-path"]   # default for --keep
similar_distance = 6                # default for --similar-distance (0-32)

[organize]
template = "{year}/{month_name}/{name}"   # default pattern when no --preset / --template
mode = "copy"                       # "copy" or "move"

[location]
near_km = 25.0                      # closer than this to a town: the town's name; farther: "Near <town>"
far_km = 100.0                      # farther than this: no town at all (only the country)
infer = false                       # like --infer-location every time
infer_window_minutes = 120          # how close in time a geotagged photo must be to lend its place
no_location_dir = "No Location"     # folder for files without a place
```

`scan.kinds`, `organize.large_plan_threshold` and the `[llm]` section are written to the file but not
used yet. The typed-phrase limit for Trash plans is fixed at 50 files.

## 11. Files Yojaka keeps

Everything Yojaka writes, apart from your plan and export files, is in its own folders:

| What | Where |
|---|---|
| Saved data per scanned folder | `~/Library/Application Support/yojaka/indexes/` |
| Run records (for history and `--resume`) | `~/Library/Application Support/yojaka/journal/` |
| Plans saved from the app (`P`) | `~/Library/Application Support/yojaka/plans/` |
| CSV exports from the app (`E`) | `~/Library/Application Support/yojaka/exports/` |
| Settings | `~/Library/Application Support/yojaka/config.toml` |
| Debug logs (`--debug` only) | `~/Library/Logs/yojaka/` |

`yojaka doctor` prints these paths. To try Yojaka without touching your real settings and saved data,
point it at another folder: `YOJAKA_HOME=/path/to/test-home yojaka …`.

## 12. Exit codes

| Code | Meaning | Examples |
|---|---|---|
| `0` | Success | Also after `--help` and `--version` |
| `1` | Error, or you didn't confirm | Folder not scanned yet, folder doesn't exist, protected system folder, plan already run, copy into the scanned folder, export file already exists, answered `n` |
| `2` | Bad arguments | Unknown option or value, unknown keep rule or placeholder, `--plan-out` file already exists, `..` in a destination, a plan file that doesn't fit the request |
| `3` | Finished, but not completely | Scan or search stopped with `Ctrl-C`; files changed since the scan; a run with skipped or failed files, or one that was stopped |

Errors go to stderr, often with a `hint:` line that says what to do.

## 13. File formats

| Kind | Extensions |
|---|---|
| Photos | `jpg` `jpeg` `jpe` `png` `gif` `heic` `heif` `webp` `tif` `tiff` `bmp` `avif` |
| Raw photos | `dng` `cr2` `cr3` `nef` `arw` `raf` `orf` |
| Videos | `mov` `mp4` `m4v` `3gp` `avi` `mkv` `webm` `mts` `m2ts` |

With `--sniff-all`, files with other extensions or none are checked by their contents too. Video details
are read from the start of the file's index (the `moov` box), never by reading the whole video.

## 14. Troubleshooting

| Message | What to do |
|---|---|
| `… has not been scanned yet` | Run `yojaka scan <folder>` first, or open it in the app. |
| `the saved data for this folder is from an older Yojaka build` | Run `yojaka scan --force <folder>`, or press `R` in the app. Your files aren't touched. |
| `… is a protected location; Yojaka will not operate on it` | Choose a folder of your own, such as `~/Pictures`. System folders are always refused. |
| `Some folders were not readable` | Give your terminal Full Disk Access (System Settings → Privacy & Security). |
| `… already exists; Yojaka never overwrites a file` | Choose a new name for `--plan-out` / `--out`. |
| `copies cannot go inside the scanned folder` | Choose a destination outside the folder, or use `--move`. |
| `plan … has already been applied` | A plan runs once. Make a new plan if needed. `--resume` is only for a run that was stopped. |
| A scan, search or run seems slow | Run it again with `--debug` and look at the log (its path is printed at start). |
