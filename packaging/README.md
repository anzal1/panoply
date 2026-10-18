# Release packaging

How a tag becomes a draft GitHub release with macOS, Windows and Linux artifacts. Everything here runs
on free GitHub-hosted runners for the public repository. The desktop app's own notes (bundle script,
platform caveats) are in `apps/desktop/packaging/README.md`.

## Cutting a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml`, commit a current `Cargo.lock` (builds use
   `cargo build --locked` and fail on a stale lock) and merge.
2. `git tag v0.1.0-beta.1 && git push origin v0.1.0-beta.1` (the tag minus `v` must start with the
   Cargo version; tags containing `-test` are exempt, used for dry runs).
3. `.github/workflows/release.yml` builds, packages, and creates a **draft** release. Review it, edit
   the notes, click Publish. Tags with a `-` are marked pre-release.
4. When you publish the draft, `.github/workflows/update-manifests.yml` opens a pull request to `main` with
   the new cask, Scoop manifest and the winget, AUR and Flatpak sources. Merge it (below).

A manual run (Actions > Release > Run workflow) builds everything and keeps the files as workflow
artifacts without creating a release.

### Build profile and caches

Stable tags (`v0.2.0`) build with the `release` profile, fat LTO. Prerelease tags (anything with a `-`, such
as `v0.2.0-beta.1`) build with the `beta` profile in `Cargo.toml`, which is `release` with thin LTO and links
much faster. The plan job picks the profile from the tag.

Compiled dependencies are cached per target and per profile with `Swatinem/rust-cache`. A tag run can only
read caches that the default branch saved, not another tag's, so after `Cargo.lock` changes run Actions >
Release > Run workflow from `main` with "Only warm the build caches" ticked, once for `beta` and once for
`release`. That builds the six legs, skips packaging and publishes nothing.

## What the workflow does

| Job | Runs on | Produces |
|---|---|---|
| `plan` | ubuntu | version, MSI version, and whether `apps/desktop` is in the workspace |
| `build` (6 legs) | macos-latest (arm64 and x64 cross), windows-latest, windows-11-arm, ubuntu-22.04, ubuntu-22.04-arm | `panoply` and `panoply-desktop` per target |
| `modules` | ubuntu | the community modules in `modules/shipped.txt`, built once for wasm32-wasip2 (`tools/build-shipped-modules.sh`) |
| `package-macos` (arm64, x64) | macos-latest | `.app` in `.zip` and `.dmg` per architecture, ad-hoc signed, not notarized |
| `package-windows` (x64, arm64) | windows-latest, windows-11-arm | `.zip` and `.msi` (WiX v4), MSI installed, checked and removed on the runner |
| `package-linux` (x64, arm64) | ubuntu-22.04, ubuntu-22.04-arm | `.tar.gz` and `.AppImage` |
| `publish` (tags only) | ubuntu | `SHA256SUMS`, filled manifests, provenance attestation, draft release |

### Community modules in the bundles

Three community modules ship with the app: `doc.markdown_to_html`, `data.format` and `image.qr_code`.
`modules/shipped.txt` is the list (folder in `modules/examples-wasm` and the module id it must hold); the
learning examples `slugify-lines` and `word-count` are not in it and never ship. The `modules` job builds
exactly that list, and each package job downloads the result into `stage/modules`. They are placed where the
catalog looks (`panoply_core::modules::search_dirs`):

| Bundle | Folder |
|---|---|
| macOS app | `Panoply.app/Contents/Resources/modules/<name>/` |
| Windows MSI and zip | `modules\<name>\` next to `panoply-desktop.exe` |
| Linux AppImage | `usr/bin/modules/<name>/` |
| Linux tarball (and `install.sh`) | `bin/modules/<name>/` |
| `panoply-cli-*` archives | `modules/<name>/` next to the `panoply` binary |

Each package script refuses to run without the built modules, checks the result (the macOS app and disk
image, the AppImage, the Windows MSI after it is installed), and runs the packaged `panoply modules` to see
the three listed and no example. `verify_release.py` checks every `.zip` and `.tar.gz` for exactly the shipped list.

Linux builds on 22.04 so the binaries run on older glibc. Windows builds link the C runtime statically
(no VC++ redistributable needed).

### Files in a release (version `V`, no leading `v`)

| File | What |
|---|---|
| `Panoply-V-macos-{arm64,x64}.dmg` / `.zip` | the app (`Panoply.app`, with the CLI inside as `Contents/MacOS/panoply-cli`) |
| `Panoply-V-windows-{x64,arm64}.msi` / `.zip` | installer / portable folder with `panoply-desktop.exe` and `panoply.exe` |
| `Panoply-V-linux-{x64,arm64}.AppImage` / `.tar.gz` | AppImage / folder with `bin/`, `share/` and `install.sh` |
| `panoply-cli-V-<platform>.tar.gz` or `.zip` | the command line tool alone (also the MCP server) |
| `SHA256SUMS` | checksums of all of the above |
| `panoply-package-manifests-V.zip` | the rendered Homebrew, Scoop, winget and Flatpak files |

The Windows MSI installs per machine, shows three features in the setup dialog (the app, `panoply` on
PATH, and an **Explorer right-click "Open with Panoply"** entry), and removes them on uninstall.
The right-click entry is in `HKLM\Software\Classes`, so Windows 11 lists it under "Show more options".

### The desktop gate

`plan` builds the desktop app only when `panoply-desktop` is a member of the Cargo workspace
(`cargo metadata`). Otherwise the CLI is released and the desktop packaging steps run on the CLI as a
stand-in, with files named `selftest-*` that the publish job never releases, so the zip, MSI, dmg and
AppImage paths stay exercised. Set the repository variable `PANOPLY_SKIP_DESKTOP=1` to force a
CLI-only release even when the desktop crate exists. If the desktop crate is in the workspace and does
not build, the release fails (by design: a release must not silently drop the app).

## Signing

Releases are unsigned on purpose (no Apple Developer account, no Windows certificate). Nothing signs or
notarizes in the workflow, and the release notes, the README and the Homebrew cask tell users the first-open step.

- macOS: the app is ad-hoc signed (Apple Silicon will not run unsigned code at all) and not notarized.
  First open: System Settings > Privacy & Security > Open Anyway, or `xattr -dr com.apple.quarantine /Applications/Panoply.app`.
- Windows: not signed. SmartScreen shows "Windows protected your PC": More info, then Run anyway.
  `package.ps1` has `SIGNING HOOK` markers where a signing step would go.
- Linux: no signing needed. Every release file carries a GitHub build provenance attestation:
  `gh attestation verify FILE --repo anzal1/panoply`.

## Package managers

`render_manifests.py` fills the `*.in` templates from `SHA256SUMS`: at the end of the release workflow (the zip asset) and again
when the release is published (the pull request below).

| Channel | Template | Goes to | Notes |
|---|---|---|---|
| Homebrew cask | `homebrew/panoply.rb.in` | `Casks/panoply.rb` in this repository | links the CLI as `panoply`; carries the unsigned-app caveat until notarized |
| Scoop | `scoop/panoply.json.in` | `bucket/panoply.json` in this repository | `autoupdate` is set; the SHA256SUMS regex is read from each release |
| winget | `winget/*.yaml.in` (schema 1.10; `AppsAndFeaturesEntries` carry the MSI DisplayVersion, ProductCode and UpgradeCode so winget matches the installed MSI) | PR to microsoft/winget-pkgs, `manifests/a/anzal1/Panoply/V/` (or `wingetcreate submit`) | x64 and arm64 MSI with their ProductCodes. The identifier `anzal1.Panoply` is permanent once accepted: change it in the three templates first if a project or org name should own it |
| AUR | `aur/PKGBUILD.in`, `aur/SRCINFO.in` | AUR git repo `ssh://aur@aur.archlinux.org/panoply-bin.git`, files `PKGBUILD` and `.SRCINFO` | `panoply-bin` for x86_64 and aarch64 from the Linux tarballs; installs to `/usr/lib/panoply` (modules sit next to the executables) with symlinks in `/usr/bin`; `pkgver` is the version with `-` turned into `_` |
| Flatpak (Flathub) | `flatpak/io.github.anzal1.Panoply.yml.in`, metainfo | Flathub new-app PR (github.com/flathub/flathub, branch `new-pr`) | builds from the tagged source, offline, on runtime 26.08 (rust-stable extension); `cargo-sources.json` comes from the tag's Cargo.lock via `tools/validate-packaging.sh` (flatpak-cargo-generator, pinned) and is committed beside the manifest; pdfium is its own module at the pinned release. The community wasm modules are not in this build yet (no wasm32-wasip2 target in the sandbox). Not built in CI |
| Flatpak (sideload) | `flatpak/io.github.anzal1.Panoply.sideload.yml.in` | a local or release-attached bundle only, never Flathub | installs the prebuilt release tarball per architecture, runtime 26.08. Flathub requires open-source apps to build from source, so this one is for people who want a Flatpak without waiting for Flathub |
| AppImage | built by `linux-package.sh` | the release (`Panoply-V-linux-{x64,arm64}.AppImage`) | appimagetool and the type2 runtime are pinned by SHA-256; relies on the host's windowing, graphics, GTK3 and ALSA libraries |

One run renders every channel for a published tag, and `tools/validate-packaging.sh` checks them (winget schemas,
`makepkg --printsrcinfo`, `namcap`, `appstreamcli validate`; the Arch part runs in a container under the heavy lock):

```bash
python3 -I packaging/scripts/render_manifests.py --tag v0.2.0-beta --out /tmp/m     # needs the gh CLI; --msi-version defaults to what the plan job derives
tools/validate-packaging.sh v0.2.0-beta
```

The release workflow renders the same files into `panoply-package-manifests-V.zip` (with the ProductCodes the
Windows jobs recorded) and validates the winget YAML against the schemas as a warning. Nothing here submits
anything: the winget, AUR and Flathub steps are a person's pull request or push.

### This repository is the tap and the bucket

There are no separate `homebrew-panoply` or `scoop-panoply` repositories and no token. The files live at the repository root:

| File | Install |
|---|---|
| `Casks/panoply.rb` | `brew tap anzal1/panoply https://github.com/anzal1/panoply`, then `brew install --cask anzal1/panoply/panoply` |
| `bucket/panoply.json` | `scoop bucket add panoply https://github.com/anzal1/panoply`, then `scoop install panoply/panoply` |
| `packaging/rendered/{winget,aur,flatpak}/` | sources for the pull requests and pushes a person makes by hand (table above); the Flatpak `cargo-sources.json` is still made by `tools/validate-packaging.sh` |

Why these layouts work for a repository that is not named `homebrew-*` and also holds the source code:

- Homebrew: the two-argument `brew tap <user>/<repo> <URL>` form clones any Git URL and does not require the `homebrew-`
  prefix ([Taps](https://docs.brew.sh/Taps): "The two-argument form does not impose this naming convention because the full
  URL is explicit"). The clone lands in `Library/Taps/anzal1/homebrew-panoply`. Casks are read only from the tap's `Casks/`
  folder (`Tap#cask_dir` in Homebrew's `tap.rb`; see [How to Create and Maintain a Tap](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)),
  so the rest of the repository is ignored. The repository has no `Formula/`, `HomebrewFormula/` or top-level `.rb` file, which
  Homebrew would otherwise scan for formulae. Keep it that way.
- Homebrew trust ([Tap Trust](https://docs.brew.sh/Tap-Trust)): non-official taps need explicit trust since Homebrew 6.0.0, and
  a fully qualified `brew install --cask anzal1/panoply/panoply` trusts only that cask, so the one-liner needs no `brew trust`.
- Scoop: `scoop bucket add <name> <repo>` clones the repository, and Scoop reads manifests from its `bucket/` subfolder when
  that folder exists, otherwise from the root (`Find-BucketDirectory` in Scoop's [`lib/buckets.ps1`](https://github.com/ScoopInstaller/Scoop/blob/master/lib/buckets.ps1);
  the [Scoop README](https://github.com/ScoopInstaller/Scoop#readme) lists `scoop bucket add`). `scoop install panoply/panoply` is the
  `bucket/app` form.

Homebrew 7 has no `--no-quarantine` option for `brew install`, so an unsigned cask keeps the quarantine flag and macOS blocks its first open. The
cask's `caveats` block prints the Open Anyway steps and the `xattr` command (it goes away with `PANOPLY_MACOS_NOTARIZED=1`).

Both manifests follow the newest published release, prereleases included, because every release is a beta for now (the choice
is recorded in `docs/DECISIONS.md`). The cask's `livecheck` reads git tags, since Homebrew's GitHub strategies skip prereleases.

`.github/workflows/update-manifests.yml` runs when a release is published (or by hand with a tag). It runs
`render_manifests.py --tag`, copies the results into the paths above on a branch `bot/manifests-<tag>` and opens a
pull request to `main` with `GITHUB_TOKEN` (contents and pull-requests write). It never pushes to `main`. One setting is
needed: Settings > Actions > General > Workflow permissions > "Allow GitHub Actions to create and approve pull requests".
Without it the branch is pushed, the run fails with a link to open the pull request by hand. Pull requests opened with
`GITHUB_TOKEN` do not start other workflows, so close and reopen the pull request to run CI on it. Set the repository variable
`PANOPLY_MACOS_NOTARIZED=1` once macOS builds are notarized, to drop the cask caveat.

To refresh by hand: `python3 -I packaging/scripts/render_manifests.py --tag v0.2.0-beta --out /tmp/m`, then copy `homebrew/panoply.rb`
to `Casks/panoply.rb` and `scoop/panoply.json` to `bucket/panoply.json`.

Template placeholders are `@NAME@` (see the docstring in `scripts/render_manifests.py`). An asset
missing from a release leaves a zero hash and a visible workflow warning.

## Layout

```
packaging/
  assets/        placeholder icon (gen_icon.py draws it with the standard library)
  scripts/       macos-package.sh, linux-package.sh, mkarchive.py, render_manifests.py
  windows/       panoply.wxs (WiX v4), package.ps1 (zips, MSI, install smoke test)
  macos/         entitlements.plist for the hardened runtime
  linux/         panoply.desktop, install.sh (tarball installer)
  homebrew/ scoop/ winget/ flatpak/ aur/   manifest templates
  signing/       APPLE.md (checklist for when the Apple Developer account exists)
  rendered/      the winget, AUR and Flatpak files for the latest release (written by update-manifests.yml)
```

Icons: the scripts use `apps/desktop/packaging/icons/panoply-512.png` and `panoply.ico` when the
desktop app adds them, and the placeholder in `assets/` otherwise. The macOS and Linux scripts also
prefer `apps/desktop/packaging/macos/bundle.sh` and `linux/panoply.desktop` from the desktop crate.

## Tools and licenses

Build-time only, nothing is shipped from them: WiX Toolset 4.0.5 (MS-RL; it builds the MSI, the
resulting package is ours), appimagetool 1.9.1 and type2-runtime 20251108 (MIT, pinned by SHA-256 in
`linux-package.sh`). The AppImage runtime is embedded in the AppImage.

## Running pieces locally

```bash
python3 packaging/scripts/render_manifests.py --version 0.1.0 --repo anzal1/panoply --sums SHA256SUMS --out /tmp/m
packaging/scripts/macos-package.sh --version 0.0.0 --stage DIR_WITH_panoply --out /tmp/out --desktop stub   # on a Mac
```
Windows and Linux packaging need those OSes (the workflow's runners are the test bed).
