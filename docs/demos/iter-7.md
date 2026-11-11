# Demo: Iteration 7 (more duplicate tiers)

Nothing here deletes a file unless you turn write mode on and confirm.

```sh
export YOJAKA_HOME=$PWD/out/home                     # keep saved data and logs out of ~/Library
cargo run -p fixturegen -- out/ --files 400 --variants 120 --videos 0.1 --live-photos 8
cargo build --release -p yojaka-cli
Y=./target/release/yojaka
$Y scan out/tree
```

`--variants 120` adds 120 sets of look-alikes: a detailed picture (or a clip) plus a re-encoded,
resized, stripped, renamed or re-exported copy, and decoys that only seem to match (a burst shot,
a name collision, a longer clip). `out/manifest.json` lists them (`variant`, `expected.same_content`).

## Command line

```sh
$Y dupes out/tree                          # as before: identical files only
$Y dupes out/tree --tier likely            # same date, camera and pixels; listed as "review"
$Y dupes out/tree --tier name              # same name apart from a copy suffix, sizes within 5%
$Y dupes out/tree --tier all               # exact + likely + name
$Y dupes out/tree --tier all --similar     # also compares how pictures look (reads each picture once)
$Y dupes out/tree --tier similar           # the second time: "0 read now, the rest remembered"
$Y dupes out/tree --tier similar --similar-distance 0     # stricter: fewer groups
$Y dupes out/tree --tier all --similar --plan-out out/plan.json
$Y plan show out/plan.json                 # only identical copies are in the plan
```

Check:
- Groups of the likely, name and similar tiers say `review` on every file, never `keep` / `delete`.
- The last lines say how many groups need a look and that nothing in them is marked.
- `PIC_0000.jpg` and `PIC_0000_edit.jpg` are a likely group; `PIC_0001.jpg` and `PIC_0001_small.jpg`
  only show up with `--similar`; `PIC_0003.jpg` and `PIC_0003 copy.jpg` are a name group.
- `PIC_0004.jpg` and `PIC_0004_b.jpg` (a burst) are a likely group but not a similar one.

## In the app

```sh
$Y out/tree
```

| Key | What to check |
|-----|---------------|
| `3`, then `f` | The title reads `Duplicates · N exact · N likely · N name`. The summary says how many groups are still to review. |
| `F` | Searches again and also compares pictures: a `similar` count appears. |
| `v` | Shows exact, then likely, then similar, then name groups only, then all. |
| on a likely group | Rows say `? review`. Under the list: size, pixels, date and camera of each file, with differences highlighted. |
| `p` | Changes the rule for identical copies. Likely / similar / name groups stay unmarked. |
| `Tab`, `Enter` | Keeps the selected file of this one group and marks the other. `x` clears the marks again. |
| `W`, `y`, then `a` | The Review box says how many removals "are not identical to the copy that stays". `Esc` cancels. |
| `a`, `y` | Runs it. `5` (History), `u` puts everything back. |

## Numbers

```sh
cargo nextest run -p yojaka-core -E 'test(tiers_precision_and_recall_on_the_fixture)' --no-capture
```

prints the table recorded in `docs/dupes-accuracy.md`.

Clean up: `rm -rf out/`.
