#!/bin/bash
# Compile-check the desktop crate for Linux in Docker (colima is enough). Low CPU: two cores, one rustc at a time (the colima VM has 2 GB, wasmtime needs the room), niced.
# Usage: tools/linux-check.sh [check|clippy|test]
set -euo pipefail
cd "$(dirname "$0")/../../.."
MODE="${1:-check}"
docker run --rm --cpus 2 \
  -v "$PWD":/work:ro \
  -v panoply-linux-target:/cargo-target \
  -v panoply-linux-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/cargo-target -e CARGO_BUILD_JOBS=1 \
  -w /work rust:latest bash -c '
    set -e
    apt-get update -qq >/dev/null
    apt-get install -y -qq --no-install-recommends \
      pkg-config cmake clang libclang-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libx11-dev libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libxcb-render0-dev libxcb-shape0-dev \
      libxcb-xfixes0-dev libfontconfig-dev libfreetype-dev libssl-dev libasound2-dev libvulkan-dev \
      libdbus-1-dev libgtk-3-dev >/dev/null
    case "'"$MODE"'" in
      check)  nice -n 10 cargo check  --locked -p panoply-desktop --all-targets ;;
      clippy) nice -n 10 cargo clippy --locked -p panoply-desktop --no-deps --all-targets -- -D warnings ;;
      test)   nice -n 10 cargo test   --locked -p panoply-desktop ;;
    esac'
