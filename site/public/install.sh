#!/bin/sh
# Panoply installer for macOS and Linux.
#
#   curl -fsSL https://panoply.anzalabidi.dev/install.sh | sh
#
# What it does: picks the right build for this computer, downloads the smallest archive over 4 parallel
# connections, checks it against the release's SHA256SUMS, and installs it without sudo. No telemetry:
# the only servers it talks to are api.github.com and github.com (release files).
#
# Environment overrides (all optional):
#   PANOPLY_VERSION=v0.3.0-beta  install this release tag instead of the newest one
#   PANOPLY_APP_DIR=DIR          macOS: the folder that gets Panoply.app (default /Applications, else ~/Applications)
#                                Linux: the folder the app is unpacked into (default ~/.local/share/panoply)
#   PANOPLY_BIN_DIR=DIR          where the `panoply` command is linked (default ~/.local/bin)
#   PANOPLY_CLI_ONLY=1           install only the command line tool
#   PANOPLY_DATA_HOME=DIR        Linux: where the menu entry and icons go (default $XDG_DATA_HOME or ~/.local/share)
#   GITHUB_TOKEN=...             optional, only sent to api.github.com, to avoid its anonymous rate limit
#
# The whole script sits inside main() and runs on the last line, so a connection that drops half way through
# the download never runs half a script.

REPO="anzal1/panoply"
STREAMS=4
TMP=""
STAGE=""
OLD=""
NEWAPP=""

say() { printf '%s\n' "$*"; }
fail() { printf 'panoply install: %s\n' "$*" >&2; exit 1; }

now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time' 2>/dev/null || date +%s; }

cleanup() {
  [ -n "$TMP" ] && rm -rf "$TMP"
  [ -n "$STAGE" ] && rm -rf "$STAGE"
  [ -n "$OLD" ] && rm -rf "$OLD"
  [ -n "$NEWAPP" ] && rm -rf "$NEWAPP"
  return 0
}

need() { command -v "$1" >/dev/null 2>&1 || fail "$1 is required but was not found"; }

api() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" -H "Accept: application/vnd.github+json" "$1"
  else
    curl -fsSL -H "Accept: application/vnd.github+json" "$1"
  fi
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  else
    fail "no sha256 tool found (shasum, sha256sum or openssl)"
  fi
}

# Ask for one byte of the file. This tells us in one request whether it exists, how big it is, whether the
# server takes range requests, and the final (signed) download address after GitHub's redirect.
# probe URL OUT writes "TOTAL RANGES FINAL" to OUT on success and leaves OUT empty otherwise.
probe() {
  p_hdr="$2.hdr"
  p_res=$(curl -sSL -r 0-0 -D "$p_hdr" -o /dev/null -w '%{http_code} %{url_effective}' "$1" 2>/dev/null) || return 0
  case "$p_res" in 200\ *|206\ *) ;; *) return 0 ;; esac
  p_final=${p_res#* }
  tr '[:upper:]' '[:lower:]' <"$p_hdr" | tr -d '\r' >"$p_hdr.lc"
  p_total=$(sed -n 's/^content-range: *bytes [0-9]*-[0-9]*\/\([0-9][0-9]*\).*/\1/p' "$p_hdr.lc" | tail -n 1)
  p_ranges=1
  if [ -z "$p_total" ]; then
    p_ranges=0
    p_total=$(sed -n 's/^content-length: *\([0-9][0-9]*\).*/\1/p' "$p_hdr.lc" | tail -n 1)
  fi
  printf '%s %s %s\n' "${p_total:-0}" "$p_ranges" "$p_final" >"$2"
}

# download URL OUT: 4 parallel ranges when the server allows it, one stream when it does not.
download() {
  url=$1
  out=$2
  if [ "$RANGES" = 1 ] && [ -n "$TOTAL" ] && [ "$TOTAL" -ge 1048576 ]; then
    chunk=$(( (TOTAL + STREAMS - 1) / STREAMS ))
    i=0
    pids=""
    while [ "$i" -lt "$STREAMS" ]; do
      start=$(( i * chunk ))
      end=$(( start + chunk - 1 ))
      [ "$end" -ge "$TOTAL" ] && end=$(( TOTAL - 1 ))
      curl -fsSL --retry 3 -r "$start-$end" -o "$out.part$i" "$FINAL" &
      pids="$pids $!"
      i=$(( i + 1 ))
    done
    ok=1
    for pid in $pids; do
      wait "$pid" || ok=0
    done
    if [ "$ok" = 1 ]; then
      : >"$out"
      i=0
      while [ "$i" -lt "$STREAMS" ]; do
        cat "$out.part$i" >>"$out"
        rm -f "$out.part$i"
        i=$(( i + 1 ))
      done
      size=$(wc -c <"$out" | tr -d ' ')
      [ "$size" = "$TOTAL" ] && return 0
    fi
    rm -f "$out" "$out".part*
    say "  parallel download failed, retrying on one connection"
  fi
  curl -fsSL --retry 3 -o "$out" "$url" || fail "download failed: $url"
}

cli_dest() {
  if [ -n "${PANOPLY_APP_DIR:-}" ]; then printf '%s\n' "$PANOPLY_APP_DIR/panoply-cli"; else printf '%s\n' "$HOME/.local/share/panoply-cli"; fi
}

mac_apps_dir() {
  if [ -n "${PANOPLY_APP_DIR:-}" ]; then
    printf '%s\n' "$PANOPLY_APP_DIR"
  elif [ -w /Applications ]; then
    printf '%s\n' /Applications
  else
    printf '%s\n' "$HOME/Applications"
  fi
}

linux_dest() {
  printf '%s\n' "${PANOPLY_APP_DIR:-$HOME/.local/share/panoply}"
}

# The install folder is moved aside and deleted when it is replaced, so only touch one that is missing,
# empty, or already a Panoply install (it holds MARKER). Anything else could be somebody's own files.
guard_dest() {
  g_dest=$1
  g_marker=$2
  if [ ! -e "$g_dest" ] && [ ! -L "$g_dest" ]; then return 0; fi
  if [ -d "$g_dest" ] && [ ! -L "$g_dest" ]; then
    if [ -f "$g_dest/$g_marker" ]; then return 0; fi
    if [ -z "$(ls -A "$g_dest")" ]; then return 0; fi
  fi
  fail "$g_dest exists and is not a Panoply install, pick another folder"
}

# Replace DEST with SRC. SRC must sit on the same filesystem as DEST, so each step is a rename:
# the old copy only goes away after the new one is in place.
swap_in() {
  src=$1
  dest=$2
  OLD=""
  if [ -e "$dest" ] || [ -L "$dest" ]; then
    OLD="$(dirname "$dest")/.panoply-old.$$"
    mv "$dest" "$OLD" || fail "could not move the existing copy of $dest aside"
  fi
  if ! mv "$src" "$dest"; then
    [ -n "$OLD" ] && mv "$OLD" "$dest"
    OLD=""
    fail "could not put the new copy in place at $dest"
  fi
  [ -n "$OLD" ] && rm -rf "$OLD"
  OLD=""
  return 0
}

# unpack ARCHIVE STAGE_DIR
unpack() {
  case "$1" in
    *.tar.xz) tar -xJf "$1" -C "$2" ;;
    *.tar.gz) tar -xzf "$1" -C "$2" ;;
    *.zip)
      if command -v ditto >/dev/null 2>&1; then ditto -x -k "$1" "$2"; else unzip -q "$1" -d "$2"; fi
      ;;
    *) fail "unknown archive type: $1" ;;
  esac
}

# The folder that holds the payload: the single top-level folder of a tarball, or the stage itself.
payload_dir() {
  n=$(find "$1" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
  d=$(find "$1" -mindepth 1 -maxdepth 1 -type d | head -n 1)
  if [ "$n" = 1 ] && [ -n "$d" ]; then printf '%s\n' "$d"; else printf '%s\n' "$1"; fi
}

main() {
  need curl
  need tar
  need awk
  need sed

  t0=$(now)

  case "$(uname -s)" in
    Darwin) OS=macos ;;
    Linux) OS=linux ;;
    *) fail "this installer is for macOS and Linux. On Windows use: irm https://panoply.anzalabidi.dev/install.ps1 | iex" ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) ARCH=arm64 ;;
    x86_64|amd64) ARCH=x64 ;;
    *) fail "unsupported chip: $(uname -m)" ;;
  esac
  # A shell started under Rosetta reports x86_64 on an Apple silicon Mac. Run the native build instead.
  if [ "$OS" = macos ] && [ "$ARCH" = x64 ]; then
    if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then ARCH=arm64; fi
  fi

  TMP=$(mktemp -d "${TMPDIR:-/tmp}/panoply-install.XXXXXX") || fail "could not make a temp folder"
  trap cleanup EXIT
  trap 'exit 130' INT TERM

  BIN_DIR=${PANOPLY_BIN_DIR:-$HOME/.local/bin}
  CLI_ONLY=${PANOPLY_CLI_ONLY:-0}

  # Refuse early, before anything is downloaded, when the install folder holds somebody else's files.
  if [ "$CLI_ONLY" = 1 ]; then
    guard_dest "$(cli_dest)" panoply
  elif [ "$OS" = macos ]; then
    guard_dest "$(mac_apps_dir)/Panoply.app" Contents/Info.plist
  else
    guard_dest "$(linux_dest)" bin/panoply
  fi

  # Step 1: which release.
  if [ -n "${PANOPLY_VERSION:-}" ]; then
    TAG=$PANOPLY_VERSION
    case "$TAG" in v*) ;; *) TAG="v$TAG" ;; esac
  else
    # Every release is a prerelease for now, so /releases/latest does not work. Read the newest one.
    # Take the newest one that is not a draft (a token can see drafts, and their downloads answer 404).
    json=$(api "https://api.github.com/repos/$REPO/releases?per_page=5") || fail "could not read the release list from GitHub (rate limit? set PANOPLY_VERSION=v0.3.0-beta or GITHUB_TOKEN)"
    TAG=$(printf '%s' "$json" | tr -d ' \n' | grep -oE '"(tag_name":"[^"]*|draft":(true|false))' | awk '/^"tag_name"/{t=$0; sub(/^"tag_name":"/, "", t)} /^"draft":false/{if (t != "") {print t; exit}}')
    [ -n "$TAG" ] || fail "no release found for $REPO"
  fi
  VER=${TAG#v}
  BASE="https://github.com/$REPO/releases/download/$TAG"
  say "Panoply $VER for $OS $ARCH"

  # Step 2: which asset. Smallest first: .tar.xz when the release has it, else the older formats.
  if [ "$CLI_ONLY" = 1 ]; then
    stem="panoply-cli-$VER-$OS-$ARCH"
    candidates="$stem.tar.xz $stem.tar.gz"
  elif [ "$OS" = macos ]; then
    stem="Panoply-$VER-macos-$ARCH"
    candidates="$stem.tar.xz $stem.zip"
  else
    stem="Panoply-$VER-linux-$ARCH"
    candidates="$stem.tar.xz $stem.tar.gz"
  fi

  # Ask about every candidate at once (and fetch the checksums meanwhile): the missing ones answer 404.
  curl -fsSL --retry 3 -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS" &
  sums_pid=$!
  n=0
  pids=""
  for c in $candidates; do
    n=$(( n + 1 ))
    if [ "$OS" = linux ] && [ "${c%.tar.xz}" != "$c" ] && ! command -v xz >/dev/null 2>&1; then continue; fi
    probe "$BASE/$c" "$TMP/probe.$n" &
    pids="$pids $!"
  done
  for pid in $pids; do wait "$pid" || true; done
  ASSET=""
  n=0
  for c in $candidates; do
    n=$(( n + 1 ))
    if [ -s "$TMP/probe.$n" ]; then
      read -r TOTAL RANGES FINAL <"$TMP/probe.$n"
      ASSET=$c
      break
    fi
  done
  [ -n "$ASSET" ] || fail "no $OS $ARCH download in release $TAG"

  # Step 3: download the archive. The checksums are already on their way.
  mb=$(awk -v b="${TOTAL:-0}" 'BEGIN{printf "%.1f", b/1048576}')
  say "Downloading $ASSET ($mb MB)"
  download "$BASE/$ASSET" "$TMP/$ASSET"
  wait "$sums_pid" || fail "could not download SHA256SUMS"

  # Step 4: verify.
  want=$(awk -v f="$ASSET" '{n=$2; sub(/^\*/, "", n); if (n == f) print $1}' "$TMP/SHA256SUMS" | head -n 1)
  [ -n "$want" ] || fail "$ASSET is not listed in SHA256SUMS, refusing to install it"
  got=$(sha256_of "$TMP/$ASSET")
  if [ "$got" != "$want" ]; then
    fail "checksum mismatch for $ASSET (expected $want, got $got). Nothing was installed."
  fi
  say "Checksum ok"

  # Step 5: install.
  mkdir -p "$BIN_DIR" || fail "could not create $BIN_DIR"
  if [ "$CLI_ONLY" = 1 ]; then
    install_cli
  elif [ "$OS" = macos ]; then
    install_macos
  else
    install_linux
  fi

  t1=$(now)
  secs=$(awk -v a="$t0" -v b="$t1" 'BEGIN{printf "%.1f", b-a}')
  say "Installed Panoply $VER in $secs s"
  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) say "Add the command to your PATH:  export PATH=\"$BIN_DIR:\$PATH\"" ;;
  esac
}

install_cli() {
  dest=$(cli_dest)
  say "Installing the command line tool to $dest"
  parent=$(dirname "$dest")
  mkdir -p "$parent" || fail "could not create $parent"
  STAGE="$parent/.panoply-stage.$$"
  mkdir -p "$STAGE"
  unpack "$TMP/$ASSET" "$STAGE"
  src=$(payload_dir "$STAGE")
  [ -f "$src/panoply" ] || fail "the archive has no panoply program"
  # The Linux command line archive of v0.3.0-beta ships panoply without the execute bit (the program in the
  # macOS archive and in the Linux app archive has it). Set it on the program we are about to link.
  chmod 755 "$src/panoply"
  swap_in "$src" "$dest"
  rm -rf "$STAGE"
  STAGE=""
  ln -sf "$dest/panoply" "$BIN_DIR/panoply"
  say "Linked $BIN_DIR/panoply"
}

install_macos() {
  apps=$(mac_apps_dir)
  mkdir -p "$apps" || fail "could not create $apps"
  say "Installing Panoply.app to $apps"
  STAGE="$apps/.panoply-stage.$$"
  mkdir -p "$STAGE"
  unpack "$TMP/$ASSET" "$STAGE"
  src=$(find "$STAGE" -maxdepth 2 -name Panoply.app -type d | head -n 1)
  [ -n "$src" ] || fail "the archive has no Panoply.app"
  # Files fetched by curl carry no quarantine mark, so Gatekeeper stays out of the way. Make sure of it.
  xattr -dr com.apple.quarantine "$src" 2>/dev/null || true
  # Unpacked beside the old copy, then swapped by rename.
  NEWAPP="$apps/.panoply-new.$$"
  mv "$src" "$NEWAPP" || fail "could not stage the new copy"
  rm -rf "$STAGE"
  STAGE=""
  swap_in "$NEWAPP" "$apps/Panoply.app"
  NEWAPP=""
  cli="$apps/Panoply.app/Contents/MacOS/panoply-cli"
  [ -f "$cli" ] || fail "the app has no command line tool at $cli"
  ln -sf "$cli" "$BIN_DIR/panoply"
  say "Linked $BIN_DIR/panoply"
}

install_linux() {
  dest=$(linux_dest)
  say "Installing Panoply to $dest"
  parent=$(dirname "$dest")
  mkdir -p "$parent" || fail "could not create $parent"
  STAGE="$parent/.panoply-stage.$$"
  mkdir -p "$STAGE"
  unpack "$TMP/$ASSET" "$STAGE"
  src=$(payload_dir "$STAGE")
  [ -f "$src/bin/panoply" ] || fail "the archive has no bin/panoply"
  chmod 755 "$src/bin/panoply"
  [ -f "$src/bin/panoply-desktop" ] && chmod 755 "$src/bin/panoply-desktop"
  # The tarball's own install.sh is for copying into a prefix. Here the whole folder stays together (the
  # shipped modules sit next to the programs, which is where Panoply looks) and the commands are linked.
  rm -f "$src/install.sh"
  swap_in "$src" "$dest"
  rm -rf "$STAGE"
  STAGE=""
  ln -sf "$dest/bin/panoply" "$BIN_DIR/panoply"
  ln -sf "$dest/bin/panoply-desktop" "$BIN_DIR/panoply-desktop"
  say "Linked $BIN_DIR/panoply and $BIN_DIR/panoply-desktop"

  # Menu entry and icons. When PANOPLY_APP_DIR points somewhere else (a test run), leave the real home alone.
  if [ -n "${PANOPLY_DATA_HOME:-}" ]; then
    data=$PANOPLY_DATA_HOME
  elif [ -n "${PANOPLY_APP_DIR:-}" ]; then
    data=""
  else
    data=${XDG_DATA_HOME:-$HOME/.local/share}
  fi
  if [ -n "$data" ] && [ -f "$dest/share/applications/panoply.desktop" ]; then
    mkdir -p "$data/applications" "$data/icons"
    sed "s|^Exec=panoply-desktop|Exec=$dest/bin/panoply-desktop|" "$dest/share/applications/panoply.desktop" >"$data/applications/panoply.desktop"
    if [ -d "$dest/share/icons" ]; then cp -R "$dest/share/icons/." "$data/icons/"; fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true; fi
    if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$data/applications" 2>/dev/null || true; fi
    say "Added the menu entry and icons under $data"
  fi
}

main "$@"
