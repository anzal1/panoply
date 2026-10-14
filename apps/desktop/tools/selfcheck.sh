#!/bin/bash
# Headless run: builds the release profile (no LTO, 16 codegen units, so it is quick), then opens the
# real windows off screen at x = -20000 (never ordered front, never key), saves PNGs into screens/ and
# writes screens/selfcheck-report.json with the budget numbers. Nothing is shown on screen.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_STRIP=false
nice -n 10 cargo build --release -j4 -p panoply-desktop --features snapshot
nice -n 10 perl -e 'alarm 300; exec @ARGV' ./target/release/panoply-desktop --selfcheck --out screens
