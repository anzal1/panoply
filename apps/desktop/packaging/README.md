# Packaging the desktop app

One binary, `panoply-desktop`, on all three desktops. This file says how each one is packaged and
what is verified and what is not.

## macOS (verified)

```bash
apps/desktop/packaging/macos/bundle.sh                 # release build, then Panoply.app in apps/desktop/dist/
apps/desktop/packaging/macos/bundle.sh --binary PATH   # bundle a binary you already built
apps/desktop/packaging/macos/bundle.sh --sign "Developer ID Application: Name (TEAM)"
```

- `Info.plist` has `LSUIElement` false: the app has a Dock icon and an app window, and the command
  bar is created hidden at start and opens from the global hotkey (Option+Space, or
  `PANOPLY_HOTKEY="ctrl+alt+p"`). Closing the app window keeps the bar alive. Clicking the Dock icon
  reopens the window. `--background` starts with no window (a login item).
- Files dropped on the Dock icon arrive as file URLs (`CFBundleDocumentTypes` for `public.data` and
  `public.folder`).
- Community modules in `Resources/modules/<name>/` (manifest plus `module.wasm`) load at start, next to
  the user's own folder `~/Library/Application Support/Panoply/modules/`.
- Release builds use the workspace profile (fat LTO). For a quick bundle while developing, build with
  `CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` and pass `--binary`.
- The icon is `macos/Panoply.icns` (16, 32, 128, 256 and 512 px, each as an @1x and @2x pair), copied into
  `Contents/Resources` and named by `CFBundleIconFile`. The source is `icon/panoply-macos.svg`: a continuous-corner
  squircle body of 824 of 1024 px on the macOS grid, a light plate, and the four squares as a raised layer with a contact
  shadow and a hairline rim, with the soft body shadow baked in. 16, 32 and 64 px use their own pixel-grid drawings
  (`icon/panoply-macos-<size>.svg`). `icon/panoply-macos-dark.svg` is a dark-plate variant kept for reference.
  `tools/gen_app_icon.mjs` regenerates every icon file and `docs/design/icon-sheet.png` (needs `resvg`).
- Still to do: notarization (`xcrun notarytool submit` with the Developer ID
  signature, then `stapler staple`), the Finder and share extensions (small Swift targets, Phase 2 later).

## Linux (compiles in Docker; not driven by hand yet)

- Build dependencies (Debian names): `pkg-config cmake clang libxkbcommon-dev libxkbcommon-x11-dev
  libwayland-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libssl-dev libvulkan-dev`.
  `apps/desktop/tools/linux-check.sh [check|clippy|test]` runs them in Docker (colima is enough).
- `linux/panoply.desktop` is the launcher and file handler (`Exec=panoply-desktop %F` takes files from
  "Open with"). Install to `/usr/share/applications/` with the binary in `/usr/bin/` and an icon named
  `panoply`. The icon files are in `linux/icons/hicolor/` (16, 22, 24, 32, 48, 64, 128, 256 and 512 px PNG and a scalable SVG, in the GNOME
  app icon style with its shadow baked in; 64 px and below are pixel-grid drawings): copy that folder
  into `/usr/share/icons/` and run `gtk-update-icon-cache`.
- Channels: Flatpak (Flathub) and AppImage. The Flatpak needs `--socket=wayland --socket=fallback-x11
  --device=dri --filesystem=home` (or the file chooser portal only, which is the cleaner choice) and the
  `org.freedesktop.Platform` runtime. No signing is needed.
- File manager actions: Nautilus (Python extension or `Scripts`), Dolphin (`.desktop` service menu in
  `~/.local/share/kio/servicemenus/`). Both call `panoply-desktop <files>`.
- The global hotkey works on X11 (`global-hotkey`). Wayland has no global hotkeys without the compositor's
  GlobalShortcuts portal, which is not wired up yet: on Wayland bind a key in the desktop settings to
  `panoply-desktop --background` plus a future `--bar` flag, or use the app window.
- Trash uses the freedesktop trash folders (`~/.local/share/Trash`).
- Unverified until someone runs it on a real desktop: bar window kind (`WindowKind::PopUp` on X11 and
  Wayland), hide and show of the bar, Reduce Motion (read from GNOME gsettings), IME on the search field.

## Windows (compile target only so far)

- The icon is `windows/panoply.ico` (16, 20, 24, 32, 40, 48, 64, 96, 128 and 256 px; 32-bit BMP entries below 256 and
  a PNG entry at 256). It is a flat Fluent style tile, nearly full bleed, drawn separately from the macOS icon; sizes up
  to 64 px are pixel-grid drawings (`icon/panoply-windows-<size>.svg`). The installer and the Start menu shortcut use
  it. Embedding it in the `.exe` itself (so Explorer and the taskbar show it without a shortcut) needs a build script
  with a resource compiler (the `winresource` crate); that is not wired yet.

- Build with MSVC: `cargo build --release -p panoply-desktop`. GPUI uses DirectX through its Windows
  platform crate. No extra system packages are needed.
- Installer: MSIX or an Inno Setup `.exe`, plus winget and Scoop manifests. SmartScreen warns on unsigned
  binaries until reputation builds; the beta ships unsigned on purpose.
- Explorer context menu and Send To: a registry entry `HKCU\Software\Classes\*\shell\Panoply\command`
  with `"panoply-desktop.exe" "%1"`, and a shortcut in `shell:sendto`.
- Trash is not wired: `Undo` removes the new file instead of sending it to the Recycle Bin (needs
  `SHFileOperationW`, which needs the `windows` crate behind `cfg(windows)`).
- Reduce Motion is not read yet (`SPI_GETCLIENTAREAANIMATION`); `PANOPLY_REDUCE_MOTION=1` forces it.
- Global hotkey uses `RegisterHotKey` through `global-hotkey`; Alt+Space is the system menu on Windows, so
  the default should change there (try Ctrl+Alt+Space) before launch.
