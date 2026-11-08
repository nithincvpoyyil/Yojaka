# ADR-0005: Directory walker

Status: accepted (fix after Iteration 4)

## Context
The scan used `jwalk` with a back-pressure loop that slept inside jwalk's worker callback once 50,000
entries were in flight. jwalk yields folders in a fixed order. A sleeping worker could hold the folder
the consumer needed next, and the scan froze for good. A single folder with more entries than the cap
(a camera dump, `target/debug/deps`) triggered it every time. Removing the loop fixed the hang, but
memory then grew with the tree (about 270 MB at 1M entries, over the 150 MB I1 limit). jwalk itself
is unmaintained: its repo was archived on 2026-08-05, and 0.9.0 says "Use dua-core instead".

## Decision
- Walk with `dua-core`, pinned to an exact version, in `Order::Completion` (unordered).
- Its result queue is bounded and workers wait when it is full. This is safe because the consumer takes
  any entry; nothing waits for one particular folder.
- Folder rules live in the `descend` callback. Bundles and Yojaka's own folders are always pruned. The
  skip list and the hidden rule apply below the root only, never to the folder the user chose.
- Walker metadata is skipped (`Options::skip_metadata`). Yojaka keeps its own `lstat` per file because
  dua-core exposes no file flags, ctime or birth time, and the dataless check (`SF_DATALESS`) is a safety rule.
- Never add waiting inside the walker, and never use an ordered walk with any waiting.

## Consequences
- Memory stays flat: about 24 MB for a 104k-file and a 191k-file first scan (was 52 / 74 MB).
- The scan is about 5% slower on small files (4.8 s versus 4.6 s on 104k files); the walk itself is
  about 2% of scan time.
- Walk order varies from run to run. Nothing may depend on it.
- dua-core is young and changes often: upgrade on purpose and re-run the walker tests.
- dua-core reports APFS clone IDs (`Metadata::clone_id`). The duplicates feature could use them later:
  deleting a clone frees no space.
