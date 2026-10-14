#!/bin/sh
# Installs Panoply from the extracted release folder.
#   ./install.sh              to ~/.local   (no root needed)
#   sudo ./install.sh /usr/local
set -eu
PREFIX="${1:-$HOME/.local}"
HERE="$(cd "$(dirname "$0")" && pwd)"
install -d "$PREFIX/bin" "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/512x512/apps" "$PREFIX/share/doc/panoply"
install -m 755 "$HERE/bin/panoply" "$HERE/bin/panoply-desktop" "$PREFIX/bin/"
install -m 644 "$HERE/share/applications/panoply.desktop" "$PREFIX/share/applications/"
install -m 644 "$HERE/share/icons/hicolor/512x512/apps/panoply.png" "$PREFIX/share/icons/hicolor/512x512/apps/"
install -m 644 "$HERE/LICENSE-MIT" "$HERE/LICENSE-APACHE" "$HERE/THIRD-PARTY-NOTICES.md" "$PREFIX/share/doc/panoply/"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" || true
echo "Installed to $PREFIX. Make sure $PREFIX/bin is on your PATH."
