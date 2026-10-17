#!/usr/bin/env bash
# Checks the package manager files for a published release, with nothing sent anywhere:
#
#     tools/validate-packaging.sh v0.2.0-beta [--keep]
#
#   1. renders every channel (packaging/scripts/render_manifests.py --tag, needs the gh CLI)
#   2. winget YAML against Microsoft's JSON schemas (packaging/scripts/validate_manifests.py)
#   3. flatpak/cargo-sources.json from the tag's Cargo.lock, with flatpak-cargo-generator (pinned by commit and
#      SHA-256) in a python container, as the Flathub source-build manifest needs it
#   4. in an Arch Linux container: makepkg --printsrcinfo must equal the rendered .SRCINFO, namcap on the
#      PKGBUILD, and appstreamcli validate on the Flatpak metainfo
#
# Steps 3 and 4 take the machine's heavy lock and uses the colima profile "panoply" (the same small VM as
# tools/preflight-linux.sh). The VM is stopped again when the check ends if this script started it
# (--keep leaves it running). The Arch image is amd64, so on an Apple Silicon Mac it runs under emulation.
set -euo pipefail
TAG="${1:?usage: tools/validate-packaging.sh vX.Y.Z[-pre] [--keep]}"; shift || true
keep=0; [ "${1:-}" = "--keep" ] && keep=1
cd "$(git rev-parse --show-toplevel)"
OWN_OUT=0
if [ -z "${PANOPLY_PKG_OUT:-}" ]; then OWN_OUT=1; PANOPLY_PKG_OUT="$(mktemp -d)"; fi
OUT="$PANOPLY_PKG_OUT"

python3 -I packaging/scripts/render_manifests.py --tag "$TAG" --out "$OUT"
# Not -I: PyYAML and jsonschema live in the user site-packages.
python3 packaging/scripts/validate_manifests.py "$OUT"

# shellcheck source=lib/heavy-lock.sh
. tools/lib/heavy-lock.sh
profile="${PANOPLY_COLIMA_PROFILE:-panoply}"; ctx="colima-$profile"
started=0
prev_ctx="$(docker context show 2>/dev/null || echo default)"
cleanup() {
  [ "$OWN_OUT" = 1 ] && rm -rf "$OUT"
  if [ "$started" = 1 ] && [ "$keep" = 0 ]; then colima stop "$profile" >/dev/null 2>&1 && echo "[arch] VM stopped (colima stop $profile)" || true; fi
  docker context use "$prev_ctx" >/dev/null 2>&1 || true
  heavy_lock_release
}
trap cleanup EXIT
heavy_lock_acquire
if ! colima status "$profile" >/dev/null 2>&1; then
  colima start "$profile" --cpu 4 --memory 4 --disk 60 --vm-type vz --mount-type virtiofs >/dev/null 2>&1
  started=1
fi
docker context use "$prev_ctx" >/dev/null 2>&1 || true

# flatpak-cargo-generator at a pinned commit of flatpak/flatpak-builder-tools, checked by SHA-256.
GEN_COMMIT=74697c75b630d7330e77250fc13cb5ea688d9479
GEN_SHA=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380
git fetch -q origin "refs/tags/$TAG:refs/tags/$TAG" 2>/dev/null || true
git show "$TAG:Cargo.lock" | nice -n 15 docker --context "$ctx" run --rm -i --platform linux/arm64 python:3.12-slim bash -euo pipefail -c "
  apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends git curl ca-certificates >/dev/null
  pip install -q aiohttp tomlkit 2>&1 | tail -1 >&2
  curl -fsSL -o /tmp/gen.py https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$GEN_COMMIT/cargo/flatpak-cargo-generator.py
  echo '$GEN_SHA  /tmp/gen.py' | sha256sum -c - >&2
  cat > /tmp/Cargo.lock
  python /tmp/gen.py /tmp/Cargo.lock -o /tmp/cargo-sources.json >&2
  cat /tmp/cargo-sources.json" > "$OUT/flatpak/cargo-sources.json"
python3 -I -c "
import json,sys
d=json.load(open(sys.argv[1]))
print('cargo-sources.json: %d sources, %d KB' % (len(d), __import__('os').path.getsize(sys.argv[1])//1024))" "$OUT/flatpak/cargo-sources.json"

# The files go in as a tar stream: the VM only mounts the home folder, and OUT may be elsewhere.
tar -C "$OUT" -cf - aur flatpak | nice -n 15 docker --context "$ctx" run --rm -i --platform linux/amd64 archlinux:latest bash -euo pipefail -c '
  mkdir /rendered && tar -xf - -C /rendered
  pacman --disable-sandbox -Syu --noconfirm --needed base-devel namcap appstream git 2>&1 | tail -3
  useradd -m builder
  install -d -o builder /home/builder/pkg
  cp /rendered/aur/PKGBUILD /home/builder/pkg/ && chown builder /home/builder/pkg/PKGBUILD
  cd /home/builder/pkg
  echo "== makepkg --printsrcinfo"
  su builder -c "makepkg --printsrcinfo" | tee /tmp/printed.srcinfo
  if diff -u /rendered/aur/.SRCINFO /tmp/printed.srcinfo; then echo "SRCINFO MATCH: rendered .SRCINFO equals makepkg output"; else echo "SRCINFO DIFFERS"; exit 1; fi
  echo "== namcap PKGBUILD"
  namcap PKGBUILD && echo "(namcap printed nothing above: no warnings)"
  echo "== appstreamcli validate --no-net"
  appstreamcli validate --no-net /rendered/flatpak/io.github.anzal1.Panoply.metainfo.xml
  echo "== appstreamcli validate --no-net --pedantic"
  appstreamcli validate --no-net --pedantic /rendered/flatpak/io.github.anzal1.Panoply.metainfo.xml
'
