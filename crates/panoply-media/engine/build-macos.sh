#!/usr/bin/env bash
# Build the LGPL ffmpeg and ffprobe Panoply runs as a helper process on macOS.
#
#   ARCH=arm64|x86_64 FFMPEG_VERSION=9.0.2 ./build-macos.sh
#
# Every library is permissive or LGPL: opus (BSD-3), lame (LGPL-2), vorbis (BSD-3), libvpx (BSD-3),
# SVT-AV1 (BSD-3-Clause-Clear plus the AOM patent license), libaom (BSD-2 plus the AOM patent license),
# dav1d (BSD-2), libvmaf (BSD-2-Clause-Patent), zimg (WTFPL). No x264, x265, fdk-aac or rubberband.
# Source tarballs are fetched with the checksums in SHA256SUMS next to this script (fill it on the
# first build, review it, commit it): nothing is built from an unchecked download.
set -euo pipefail

ARCH="${ARCH:-$(uname -m)}"
FFMPEG_VERSION="${FFMPEG_VERSION:-9.0.2}"
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="${WORK:-$PWD/build-$ARCH}"
PREFIX="$WORK/prefix"
mkdir -p "$WORK" "$PREFIX"

fetch() { # name url
  local name="$1" url="$2" file="$WORK/$(basename "$url")"
  [ -f "$file" ] || curl --fail --location --proto '=https' --tlsv1.2 --output "$file" "$url"
  local want
  want="$(awk -v n="$(basename "$url")" '$2 == n {print $1}' "$HERE/SHA256SUMS" 2>/dev/null || true)"
  [ -n "$want" ] || { echo "no checksum for $(basename "$url") in SHA256SUMS"; shasum -a 256 "$file"; exit 1; }
  echo "$want  $file" | shasum -a 256 -c -
}

# Dependencies are built statically into $PREFIX by their own scripts (cmake/meson/autotools); the list
# and versions live in deps.txt so the relink material can be published with the binary.
# shellcheck disable=SC1091
. "$HERE/deps.sh"

fetch ffmpeg "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
tar -xf "$WORK/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$WORK"
cd "$WORK/ffmpeg-$FFMPEG_VERSION"

export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
./configure \
  --prefix="$PREFIX/out" \
  --arch="$ARCH" \
  --disable-gpl --disable-nonfree --enable-version3 \
  --disable-autodetect \
  --enable-static --disable-shared --pkg-config-flags=--static \
  --enable-ffmpeg --enable-ffprobe --disable-ffplay --disable-doc --disable-debug \
  --enable-runtime-cpudetect \
  --enable-videotoolbox --enable-audiotoolbox \
  --enable-zlib --enable-bzlib --enable-iconv --enable-lzma \
  --enable-libopus --enable-libmp3lame --enable-libvorbis \
  --enable-libvpx --enable-libsvtav1 --enable-libaom --enable-libdav1d \
  --enable-libvmaf --enable-libzimg --enable-libwebp \
  --extra-cflags="-mmacosx-version-min=12.0 -I$PREFIX/include" \
  --extra-ldflags="-mmacosx-version-min=12.0 -L$PREFIX/lib" \
  --extra-libs="-lc++"
make -j"$(sysctl -n hw.ncpu)"
make install

# The result must not claim GPL or nonfree: stop the build otherwise.
if "$PREFIX/out/bin/ffmpeg" -version | grep -E -- '--enable-(gpl|nonfree)'; then
  echo "this build is not LGPL" >&2; exit 1
fi
strip "$PREFIX/out/bin/ffmpeg" "$PREFIX/out/bin/ffprobe"
codesign --force --sign - "$PREFIX/out/bin/ffmpeg" "$PREFIX/out/bin/ffprobe"
OUT="ffmpeg-$FFMPEG_VERSION-macos-$ARCH-lgpl"
rm -rf "$WORK/$OUT" && mkdir -p "$WORK/$OUT/bin"
cp "$PREFIX/out/bin/ffmpeg" "$PREFIX/out/bin/ffprobe" "$WORK/$OUT/bin/"
cp "$WORK/ffmpeg-$FFMPEG_VERSION/COPYING.LGPLv3" "$WORK/$OUT/LICENSE.txt"
tar -C "$WORK" -cJf "$WORK/$OUT.tar.xz" "$OUT"
shasum -a 256 "$WORK/$OUT.tar.xz" | tee "$WORK/$OUT.tar.xz.sha256"
echo "size: $(stat -f %z "$WORK/$OUT.tar.xz") bytes"
