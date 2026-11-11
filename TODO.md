# TODO

Done items stay for reference; the date is when they were finished (2026-11-07).

Items that are not part of the current iteration.

- [x] **TUI: flush index and full rescan** of the folder Yojaka was opened on.
  - Today `r` runs an incremental scan and keeps existing rows. Only `yojaka scan --force` flushes (`lib.rs`, `opts.force`).
  - Add a key (not `r`) that sets `ScanOptions.force = true` and starts a scan.
  - Destructive to the index, so ask for confirmation first. It must never touch the user's files.
  - Show it in the help overlay. Add a key-sequence test and a snapshot of the confirm prompt.
  - Rescanning should also retry files whose metadata failed earlier.

- [x] **TUI: tab bar is unclear.** The header shows only a number for the active screen, so you can't tell what screens 2, 3 and 5 are.
  - Show every screen in the header with its name, e.g. `< 1 Dashboard | 2 Browser | 3 Duplicates | [4 Organize] | 5 History >`, and highlight the selected tab with the theme's selected style.
  - Keep `1`-`5` as the keys. Shorten labels (or show only numbers plus the active name) when the terminal is narrow.
  - Update the 80x24 and 160x48 snapshots, and check the header diff is only the tab bar.

- [x] **TUI: Top places on the dashboard** (done 2026-11-08).
- [x] **GeoNames attribution** in `--about` and the README (done 2026-11-08).
- [x] **Enter opens the selected file** with the OS default app (done 2026-11-08).

## Open

See "Open items" in `docs/STATUS.md`. Next iteration: Iteration 8, hardening and release.
- [ ] Optional inline thumbnails on the Duplicates screen (`ratatui-image`).
- [ ] Detailed dashboard (the user will describe it).
- [ ] Select a place on the dashboard to filter the file browser.
- [ ] Another theme (last iteration).
