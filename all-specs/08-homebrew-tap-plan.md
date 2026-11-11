# 08: Homebrew tap plan (Iteration 9, Phase 3)

Written 2026-11-11. The user wants a Homebrew tap (decision D9 in `06-i8-hardening-release-plan.md`).
Other decisions this plan builds on:
- **D6:** no Apple Developer ID for now, so the binary is unsigned (ad-hoc signed).
- **D7:** the repo `github.com/nithincvpoyyil/Yojaka` is already public.
- **D8:** support the last 4 major macOS versions. This Mac runs macOS 27.2, so that means 14, 15, 26 and 27,
  and the minimum is **macOS 14 (Sonoma)**.

## 1. Goal

```sh
brew install nithincvpoyyil/yojaka/yojaka
yojaka --version
brew upgrade yojaka      # after each release
```

## 2. Why the tap suits an unsigned binary

- The Rust linker already **ad-hoc signs** the binary (`codesign -dv target/release/yojaka`:
  `flags=0x20002(adhoc,linker-signed)`). Apple Silicon needs at least that to run anything. Nothing to add.
- **Gatekeeper only checks files that carry the quarantine flag** (`com.apple.quarantine`). A browser
  download gets the flag, so the GitHub tarball is blocked until the user runs
  `xattr -d com.apple.quarantine yojaka`. Homebrew formulae are downloaded with `curl` and get no flag, so a
  tap install runs straight away. **Check this on a clean user account** (step 5.3); do not just assume it.
- Developer ID signing and notarization can be added later without changing the tap. Only the release
  workflow changes.

## 3. Pieces

| Piece | Where | What |
|---|---|---|
| Release tarball | GitHub Release on `nithincvpoyyil/Yojaka` | `yojaka-<ver>-aarch64-apple-darwin.tar.gz` with `yojaka`, `LICENSE`, `README.md`, `CHANGELOG.md`; plus `.sha256` |
| Tap repo | new public repo **`nithincvpoyyil/homebrew-yojaka`** (the `homebrew-` prefix is required) | `Formula/yojaka.rb`, `README.md`, a CI workflow |
| Bump | release workflow in the main repo | after a release, commit the new URL and sha256 to the tap |

## 4. Steps in the main repo (before the tap)

1. **Minimum macOS 14.** Add `MACOSX_DEPLOYMENT_TARGET = "14.0"` to `.cargo/config.toml` (`[env]`). Check with
   `otool -l target/release/yojaka | grep -A4 LC_BUILD_VERSION`: `minos 14.0` (it says `11.0` today).
2. **CI on the oldest supported version:** add `macos-14` next to `macos-latest` in `ci.yml`. ImageIO (HEIC,
   AVIF, raw) must pass there; AVIF decoding needs macOS 13 or later, so 14 is fine.
3. **Release workflow** `.github/workflows/release.yml`, triggered by a `v*` tag:
   - build `cargo build --release -p yojaka-cli` on `macos-14`
   - check `minos`, `yojaka --version` (must match the tag) and `yojaka --about`
   - make the tarball and `shasum -a 256` file
   - `gh release create` with the CHANGELOG section as the notes
   - mark `-rc` tags as pre-releases
4. **README install section:** brew first. The tarball second, with the `xattr` line and the reason for it
   ("not signed with an Apple Developer ID yet"). Then build from source.

## 5. The tap repo

### 5.1 Create it (outward-facing: the user creates it, or Claude runs `gh repo create` after the user agrees)

```sh
gh repo create nithincvpoyyil/homebrew-yojaka --public --description "Homebrew tap for Yojaka"
```

### 5.2 `Formula/yojaka.rb`

A binary formula: installing takes seconds and needs no Rust toolchain. A source build would mean compiling
with fat LTO on the user's Mac, which takes minutes.

```ruby
class Yojaka < Formula
  desc "Terminal app to index, de-duplicate and organize photos and videos"
  homepage "https://github.com/nithincvpoyyil/Yojaka"
  url "https://github.com/nithincvpoyyil/Yojaka/releases/download/v1.0.0/yojaka-1.0.0-aarch64-apple-darwin.tar.gz"
  sha256 "<from the .sha256 file>"
  license "MIT"

  depends_on arch: :arm64
  depends_on macos: :sonoma

  def install
    bin.install "yojaka"
  end

  def caveats
    <<~EOS
      Yojaka is read-only unless you pass --write. To scan folders such as ~/Pictures or
      external drives, your terminal app may need Full Disk Access
      (System Settings > Privacy & Security > Full Disk Access).
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/yojaka --version")
    assert_match "GeoNames", shell_output("#{bin}/yojaka --about")
  end
end
```

- `--version` and `--about` must write nothing to disk; `--about` already creates nothing. Check
  `--version` too, because `brew test` runs in a sandbox.
- `exiftool` is **not** a dependency: it is an optional fallback. The README says `brew install exiftool`
  for it.
- `LICENSE`, `README.md` and `CHANGELOG.md` in the tarball are installed into the keg by Homebrew itself.

### 5.3 Test before the first real release

1. Tag `v1.0.0-rc.1` → the release workflow makes a pre-release.
2. Point the formula at the rc tarball on a branch of the tap and run
   `brew install --formula ./Formula/yojaka.rb`, `brew test yojaka`, `brew audit --strict --online yojaka`
   and `brew style Formula/yojaka.rb`.
3. **On a clean macOS user account** (or a fresh VM): `brew tap nithincvpoyyil/yojaka && brew install yojaka`,
   then run `yojaka --version` and a scan of a fixture tree. Check that Gatekeeper does not block it and that
   `xattr -l $(which yojaka)` shows no quarantine flag.
4. `brew uninstall yojaka` leaves nothing in `/opt/homebrew`. Yojaka's own data in `~/Library` stays; the
   README says how to remove it.

### 5.4 Tap CI (`.github/workflows/test.yml` in the tap)

On a push or pull request: on `macos-14` and `macos-latest`, run `brew audit --strict`, `brew install
--formula ./Formula/yojaka.rb` and `brew test yojaka`.

## 6. Bumping the formula on each release

- **v1.0 (manual):** `tools/release/bump-formula.sh <version>` downloads the `.sha256`, rewrites `url` and
  `sha256` in a local clone of the tap, and opens a PR with `gh pr create`. The user merges it.
- **After v1.0 (automatic):** the release workflow does the same with a fine-grained token that can only
  write to `homebrew-yojaka` (secret `HOMEBREW_TAP_TOKEN`). The tap's CI checks the PR before merging. The
  user creates the token, never Claude.

## 7. Later (not v1.0)

- Developer ID signing and notarization (D6) in the release workflow. The formula stays the same.
- Shell completions (`clap_complete`, a `yojaka completions zsh` subcommand) installed by the formula.
- A `head` formula that builds from `main` for testers.
- `homebrew-core` (plain `brew install yojaka`) needs proof the project is notable: stars, forks and outside
  users. Revisit after v1.0.

## Done when

- [ ] `brew install nithincvpoyyil/yojaka/yojaka` works on a clean account on macOS 14 (CI) and 27 (this Mac).
- [ ] `brew test`, `brew audit --strict` and `brew style` pass in the tap's CI.
- [ ] Gatekeeper does not block the installed binary, and the install has no quarantine flag.
- [ ] The README has the brew install and the tarball `xattr` note.
- [ ] Bumping to a new version takes one script run plus a PR merge.
