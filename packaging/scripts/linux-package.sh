#!/bin/bash
# Packages the Linux artifacts for one architecture from binaries already built into --stage.
#
#   linux-package.sh --version 0.1.0-beta.1 --arch x64|arm64 --stage DIR --out DIR --desktop real|stub|none
#
# --stage holds `panoply` and, with --desktop real, `panoply-desktop`, and `modules/` (the output of
# tools/build-shipped-modules.sh). The shipped community modules go to bin/modules in the tarball, usr/bin/modules
# in the AppImage and next to the CLI in its archive: the folder next to the executable is where the catalog looks.
#   real  ships the desktop app
#   stub  uses the CLI as the desktop binary to exercise the tarball and AppImage paths while the
#         desktop crate is absent; outputs get a `selftest-` prefix and are never released
#   none  CLI tarball only
# Output in --out: panoply-cli-V-linux-ARCH.tar.gz, Panoply-V-linux-ARCH.tar.gz, Panoply-V-linux-ARCH.AppImage.
#
# The AppImage is built with appimagetool (MIT) and the type2 runtime (MIT), both pinned by checksum.
# It relies on the host for the graphics, windowing and toolkit libraries (libxkbcommon, libvulkan,
# fontconfig, Wayland or X11, GTK3, ALSA), which desktop distributions have; bundling them is a later step.
# Linux needs no code signing.
set -euo pipefail

VERSION=""; ARCH=""; STAGE=""; OUT=""; DESKTOP="none"
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --arch) ARCH="$2"; shift 2 ;;
    --stage) STAGE="$(cd "$2" && pwd)"; shift 2 ;;
    --out) mkdir -p "$2"; OUT="$(cd "$2" && pwd)"; shift 2 ;;
    --desktop) DESKTOP="$2"; shift 2 ;;
    *) echo "unknown option $1"; exit 2 ;;
  esac
done
[ -n "$VERSION" ] && [ -n "$ARCH" ] && [ -n "$STAGE" ] && [ -n "$OUT" ] || { echo "usage: see header"; exit 2; }

HERE="$(cd "$(dirname "$0")" && pwd)"
PKG="$(cd "$HERE/.." && pwd)"
ROOT="$(cd "$PKG/.." && pwd)"
WORK="$(mktemp -d)"
PREFIX=""; [ "$DESKTOP" = stub ] && PREFIX="selftest-"
PY="python3 -I"

# appimagetool 1.9.1 and type2-runtime 20251108, by the sha256 GitHub publishes for each release asset.
case "$ARCH" in
  x64)
    APPIMAGE_ARCH=x86_64
    TOOL_SHA=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
    RUNTIME_SHA=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
  arm64)
    APPIMAGE_ARCH=aarch64
    TOOL_SHA=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
    RUNTIME_SHA=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
  *) echo "unknown arch $ARCH"; exit 2 ;;
esac

[ "$DESKTOP" = none ] || ls "$STAGE"/modules/*/module.wasm >/dev/null 2>&1 \
  || { echo "no built modules in $STAGE/modules (tools/build-shipped-modules.sh)"; exit 1; }
MODS=(); MODS_BIN=()
if [ -d "$STAGE/modules" ]; then MODS=("$STAGE/modules=modules"); MODS_BIN=("$STAGE/modules=bin/modules"); fi

# ---- CLI ------------------------------------------------------------------------------------------
CLI_NAME="panoply-cli-$VERSION-linux-$ARCH"
# The archive keeps the file mode, so the program must be executable before it is packed.
chmod +x "$STAGE/panoply"
EXTRA=()
[ -f "$ROOT/THIRD-PARTY-NOTICES.md" ] && EXTRA+=("$ROOT/THIRD-PARTY-NOTICES.md")
$PY "$HERE/mkarchive.py" "$OUT/$CLI_NAME.tar.gz" "$CLI_NAME" \
  "$STAGE/panoply=panoply" ${MODS[@]+"${MODS[@]}"} "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/README.md" ${EXTRA[@]+"${EXTRA[@]}"}
[ "$DESKTOP" = none ] && exit 0

if [ "$DESKTOP" = stub ]; then cp "$STAGE/panoply" "$STAGE/panoply-desktop"; fi
chmod +x "$STAGE/panoply" "$STAGE/panoply-desktop"
DESKTOP_FILE="$ROOT/apps/desktop/packaging/linux/panoply.desktop"
[ -f "$DESKTOP_FILE" ] || DESKTOP_FILE="$PKG/linux/panoply.desktop"
# The app's own icons (a hicolor tree) when present, else the placeholder.
ICONS_DIR="$ROOT/apps/desktop/packaging/linux/icons/hicolor"
ICON="$ICONS_DIR/512x512/apps/panoply.png"
if [ -d "$ICONS_DIR" ] && [ -f "$ICON" ]; then
  ICON_SPEC="$ICONS_DIR=share/icons/hicolor"
else
  ICON="$PKG/assets/panoply-512.png"
  ICON_SPEC="$ICON=share/icons/hicolor/512x512/apps/panoply.png"
fi

# ---- tarball --------------------------------------------------------------------------------------
APP_NAME="${PREFIX}Panoply-$VERSION-linux-$ARCH"
$PY "$HERE/mkarchive.py" "$OUT/$APP_NAME.tar.gz" "$APP_NAME" \
  "$STAGE/panoply=bin/panoply" "$STAGE/panoply-desktop=bin/panoply-desktop" \
  ${MODS_BIN[@]+"${MODS_BIN[@]}"} \
  "$DESKTOP_FILE=share/applications/panoply.desktop" \
  "$ICON_SPEC" \
  "$PKG/linux/install.sh=install.sh" \
  "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/README.md" ${EXTRA[@]+"${EXTRA[@]}"}

# ---- AppImage -------------------------------------------------------------------------------------
TOOLS="$WORK/tools"; mkdir -p "$TOOLS"
fetch() {  # url sha256 dest
  curl -fsSL --retry 3 -o "$3" "$1"
  echo "$2  $3" | sha256sum -c -
}
fetch "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$APPIMAGE_ARCH.AppImage" \
  "$TOOL_SHA" "$TOOLS/appimagetool"
fetch "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-$APPIMAGE_ARCH" \
  "$RUNTIME_SHA" "$TOOLS/runtime"
chmod +x "$TOOLS/appimagetool"

APPDIR="$WORK/Panoply.AppDir"
mkdir -p "$APPDIR/usr/bin"
cp "$STAGE/panoply" "$STAGE/panoply-desktop" "$APPDIR/usr/bin/"
[ -d "$STAGE/modules" ] && cp -R "$STAGE/modules" "$APPDIR/usr/bin/modules"
mkdir -p "$APPDIR/usr/share/doc/panoply"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" ${EXTRA[@]+"${EXTRA[@]}"} "$APPDIR/usr/share/doc/panoply/"
cp "$DESKTOP_FILE" "$APPDIR/panoply.desktop"
cp "$ICON" "$APPDIR/panoply.png"
ln -s panoply.png "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
# Run as `panoply` (a symlink to the AppImage) for the command line tool, otherwise open the app.
HERE="$(dirname "$(readlink -f "$0")")"
case "$(basename "${ARGV0:-$0}")" in
  panoply) exec "$HERE/usr/bin/panoply" "$@" ;;
esac
exec "$HERE/usr/bin/panoply-desktop" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

APPIMAGE="$OUT/$APP_NAME.AppImage"
ARCH="$APPIMAGE_ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOLS/appimagetool" --no-appstream \
  --runtime-file "$TOOLS/runtime" "$APPDIR" "$APPIMAGE"
chmod +x "$APPIMAGE"

# ---- the CLI in the tarball finds the shipped modules ---------------------------------------------
if [ "$(uname -m)" = "$APPIMAGE_ARCH" ]; then
  mkdir -p "$WORK/untar" && tar -xzf "$OUT/$APP_NAME.tar.gz" -C "$WORK/untar"
  # From a neutral folder: in a source checkout the CLI also lists modules/examples-wasm from here.
  listing="$(cd "$WORK/untar" && "$WORK/untar/$APP_NAME/bin/panoply" modules)"
  while read -r _ id _; do
    printf '%s\n' "$listing" | grep -q "^$id " || { echo "panoply modules does not list $id:"; echo "$listing"; exit 1; }
  done < <(grep -v '^#' "$ROOT/modules/shipped.txt" | grep .)
  if printf '%s\n' "$listing" | grep -E 'text\.slugify_lines|text\.word_count'; then echo "an example module leaked into the bundle"; exit 1; fi
  echo "panoply lists the shipped modules"
fi

# ---- smoke test -----------------------------------------------------------------------------------
file "$APPIMAGE" | grep -q ELF
if [ "$DESKTOP" = stub ]; then
  APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE" --version
else
  # The real app opens a window, so only check what is inside.
  (cd "$WORK" && "$APPIMAGE" --appimage-extract >/dev/null)
  test -x "$WORK/squashfs-root/usr/bin/panoply-desktop"
  test -x "$WORK/squashfs-root/usr/bin/panoply"
  for m in "$STAGE"/modules/*/; do
    test -s "$WORK/squashfs-root/usr/bin/modules/$(basename "$m")/module.wasm"
  done
  test -f "$WORK/squashfs-root/usr/share/doc/panoply/THIRD-PARTY-NOTICES.md"
  test -f "$WORK/squashfs-root/panoply.png"
fi
ls -lh "$OUT"
