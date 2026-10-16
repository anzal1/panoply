#!/bin/sh
# Installs Panoply from the extracted release folder.
#   ./install.sh              to ~/.local   (no root needed)
#   sudo ./install.sh /usr/local
set -eu
PREFIX="${1:-$HOME/.local}"
HERE="$(cd "$(dirname "$0")" && pwd)"
install -d "$PREFIX/bin" "$PREFIX/share/applications" "$PREFIX/share/icons" "$PREFIX/share/doc/panoply"
install -m 755 "$HERE/bin/panoply" "$HERE/bin/panoply-desktop" "$PREFIX/bin/"
# The community modules ship next to the programs: that is where Panoply looks for them.
if [ -d "$HERE/bin/modules" ]; then
  rm -rf "$PREFIX/bin/modules"
  cp -R "$HERE/bin/modules" "$PREFIX/bin/modules"
fi
install -m 644 "$HERE/share/applications/panoply.desktop" "$PREFIX/share/applications/"
cp -R "$HERE/share/icons/." "$PREFIX/share/icons/"
for f in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md; do
  [ -f "$HERE/$f" ] && install -m 644 "$HERE/$f" "$PREFIX/share/doc/panoply/"
done
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" || true
echo "Installed to $PREFIX. Make sure $PREFIX/bin is on your PATH."
