# Release packaging

How a tag becomes a draft GitHub release with macOS, Windows and Linux artifacts. Everything here runs
on free GitHub-hosted runners for the public repository. The desktop app's own notes (bundle script,
platform caveats) are in `apps/desktop/packaging/README.md`.

## Cutting a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml` and merge it.
2. `git tag v0.1.0-beta.1 && git push origin v0.1.0-beta.1` (the tag minus `v` must start with the
   Cargo version; tags containing `-test` are exempt, used for dry runs).
3. `.github/workflows/release.yml` builds, packages, and creates a **draft** release. Review it, edit
   the notes, click Publish. Tags with a `-` are marked pre-release.
4. Copy the rendered manifests from the `panoply-package-manifests-*.zip` asset to the tap, Scoop
   bucket and winget-pkgs (below).

A manual run (Actions > Release > Run workflow) builds everything and keeps the files as workflow
artifacts without creating a release.

## What the workflow does

| Job | Runs on | Produces |
|---|---|---|
| `plan` | ubuntu | version, MSI version, and whether `apps/desktop` is in the workspace |
| `build` (6 legs) | macos-latest (arm64 and x64 cross), windows-latest, windows-11-arm, ubuntu-22.04, ubuntu-22.04-arm | `panoply` and `panoply-desktop` per target |
| `package-macos` | macos-latest | universal2 binaries (lipo), `.app` in `.zip` and `.dmg`, optional signing and notarization |
| `package-windows` (x64, arm64) | windows-latest, windows-11-arm | `.zip` and `.msi` (WiX v4), MSI installed, checked and removed on the runner |
| `package-linux` (x64, arm64) | ubuntu-22.04, ubuntu-22.04-arm | `.tar.gz` and `.AppImage` |
| `publish` (tags only) | ubuntu | `SHA256SUMS`, filled manifests, provenance attestation, draft release |

Linux builds on 22.04 so the binaries run on older glibc. Windows builds link the C runtime statically
(no VC++ redistributable needed).

### Files in a release (version `V`, no leading `v`)

| File | What |
|---|---|
| `Panoply-V-macos-universal.dmg` / `.zip` | the app (`Panoply.app`, with the CLI inside as `Contents/MacOS/panoply-cli`) |
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

Nothing is required to produce a release; every signing step skips cleanly when its secrets are absent.

- macOS: set the six `APPLE_*` repository secrets, see `signing/APPLE.md`.
- Windows: the free route is SignPath Foundation, see `signing/SIGNPATH.md` (application text, what it
  needs, and the workflow wiring; `package.ps1` has two `SIGNING HOOK` markers).
- Linux: none needed. Every release file carries a GitHub build provenance attestation:
  `gh attestation verify FILE --repo anzal1/panoply`.

## Package managers

`render_manifests.py` fills the `*.in` templates from `SHA256SUMS` at the end of the workflow.

| Channel | Template | Goes to | Notes |
|---|---|---|---|
| Homebrew cask | `homebrew/panoply.rb.in` | a tap repo, `anzal1/homebrew-tap`, `Casks/panoply.rb` | links the CLI as `panoply`; adds an "Open Anyway" caveat until notarized |
| Scoop | `scoop/panoply.json.in` | a bucket repo, or the Extras bucket later | `autoupdate` is set; the SHA256SUMS regex is read from each release |
| winget | `winget/*.yaml.in` | PR to microsoft/winget-pkgs, `manifests/a/anzal1/Panoply/V/` (or `wingetcreate`) | uses the MSI and its ProductCode. The identifier `anzal1.Panoply` is permanent once accepted: change it in the three templates first if a project or org name should own it |
| Flatpak | `flatpak/*.in` | Flathub submission | a skeleton: needs `cargo-sources.json`, a bundled pdfium, screenshots, a local build. Not built in CI yet |
| AppImage | built by the workflow | the release | relies on the host's windowing and graphics libraries; bundling them is a later step |

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
  homebrew/ scoop/ winget/ flatpak/   manifest templates
  signing/       SIGNPATH.md, APPLE.md
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
