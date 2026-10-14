# Dependencies of the macOS engine, built static into $PREFIX. Sourced by build-macos.sh.
# NOT RUN YET: this recipe was written, not executed (building it is an Actions job, not something to do
# on a developer's Mac). The first Actions run will say which version numbers need adjusting; the
# SHA256SUMS file is filled from that run and reviewed before anything is pinned.
OPUS_VERSION="${OPUS_VERSION:-1.5.2}"
LAME_VERSION="${LAME_VERSION:-3.100}"
OGG_VERSION="${OGG_VERSION:-1.3.5}"
VORBIS_VERSION="${VORBIS_VERSION:-1.3.7}"
LIBVPX_VERSION="${LIBVPX_VERSION:-1.15.0}"
SVTAV1_VERSION="${SVTAV1_VERSION:-3.0.0}"
AOM_VERSION="${AOM_VERSION:-3.11.0}"
DAV1D_VERSION="${DAV1D_VERSION:-1.5.0}"
VMAF_VERSION="${VMAF_VERSION:-3.0.0}"
ZIMG_VERSION="${ZIMG_VERSION:-3.0.5}"
WEBP_VERSION="${WEBP_VERSION:-1.5.0}"

autotools_dep() { # name tarball-url [extra configure args]
  local name="$1" url="$2"; shift 2
  fetch "$name" "$url"
  tar -xf "$WORK/$(basename "$url")" -C "$WORK"
  ( cd "$WORK/$(basename "$url" | sed -E 's/\.tar\.(gz|xz|bz2)$//')" \
    && ./configure --prefix="$PREFIX" --disable-shared --enable-static "$@" \
    && make -j"$(sysctl -n hw.ncpu)" && make install )
}

cmake_dep() { # name tarball-url src-subdir [cmake args]
  local name="$1" url="$2" sub="$3"; shift 3
  fetch "$name" "$url"
  tar -xf "$WORK/$(basename "$url")" -C "$WORK"
  local src="$WORK/$(basename "$url" | sed -E 's/\.tar\.(gz|xz|bz2)$//')/$sub"
  cmake -S "$src" -B "$WORK/b-$name" -DCMAKE_INSTALL_PREFIX="$PREFIX" -DBUILD_SHARED_LIBS=OFF \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_DEPLOYMENT_TARGET=12.0 "$@"
  cmake --build "$WORK/b-$name" -j"$(sysctl -n hw.ncpu)" && cmake --install "$WORK/b-$name"
}

autotools_dep opus "https://downloads.xiph.org/releases/opus/opus-$OPUS_VERSION.tar.gz"
autotools_dep lame "https://downloads.sourceforge.net/project/lame/lame/$LAME_VERSION/lame-$LAME_VERSION.tar.gz"
autotools_dep ogg "https://downloads.xiph.org/releases/ogg/libogg-$OGG_VERSION.tar.xz"
autotools_dep vorbis "https://downloads.xiph.org/releases/vorbis/libvorbis-$VORBIS_VERSION.tar.xz"
autotools_dep zimg "https://github.com/sekrit-twc/zimg/archive/refs/tags/release-$ZIMG_VERSION.tar.gz"
cmake_dep libwebp "https://storage.googleapis.com/downloads.webmproject.org/releases/webp/libwebp-$WEBP_VERSION.tar.gz" "" -DWEBP_BUILD_ANIM_UTILS=OFF -DWEBP_BUILD_CWEBP=OFF -DWEBP_BUILD_DWEBP=OFF
cmake_dep svtav1 "https://gitlab.com/AOMediaCodec/SVT-AV1/-/archive/v$SVTAV1_VERSION/SVT-AV1-v$SVTAV1_VERSION.tar.gz" "" -DBUILD_APPS=OFF -DBUILD_TESTING=OFF
cmake_dep aom "https://storage.googleapis.com/aom-releases/libaom-$AOM_VERSION.tar.gz" "" -DENABLE_DOCS=OFF -DENABLE_TESTS=OFF -DENABLE_EXAMPLES=OFF -DENABLE_TOOLS=OFF
autotools_dep libvpx "https://github.com/webmproject/libvpx/archive/refs/tags/v$LIBVPX_VERSION.tar.gz" --disable-examples --disable-unit-tests --disable-docs --enable-vp9-highbitdepth --enable-pic
# dav1d and libvmaf use meson (install it on the runner: brew install meson ninja nasm).
fetch dav1d "https://code.videolan.org/videolan/dav1d/-/archive/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.gz"
tar -xf "$WORK/dav1d-$DAV1D_VERSION.tar.gz" -C "$WORK"
meson setup "$WORK/b-dav1d" "$WORK/dav1d-$DAV1D_VERSION" --prefix="$PREFIX" --default-library=static -Denable_tools=false -Denable_tests=false && ninja -C "$WORK/b-dav1d" install
fetch vmaf "https://github.com/Netflix/vmaf/archive/refs/tags/v$VMAF_VERSION.tar.gz"
tar -xf "$WORK/v$VMAF_VERSION.tar.gz" -C "$WORK"
meson setup "$WORK/b-vmaf" "$WORK/vmaf-$VMAF_VERSION/libvmaf" --prefix="$PREFIX" --default-library=static -Denable_tests=false -Denable_docs=false -Dbuilt_in_models=true && ninja -C "$WORK/b-vmaf" install
