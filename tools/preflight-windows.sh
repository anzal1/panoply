#!/usr/bin/env bash
# Catch Windows-only compile and lint errors from an Apple-silicon Mac, without a VM and without CI.
#
#     tools/preflight-windows.sh            clippy for the x86_64-pc-windows-msvc target, for the crates
#                                           changed since origin/main and every crate that depends on them
#     tools/preflight-windows.sh --all      the whole workspace
#     tools/preflight-windows.sh --build    also link target/.../panoply-desktop.exe (it is never run)
#     tools/preflight-windows.sh --plan     only print what would run
#     tools/preflight-windows.sh --setup    only check the tools and fetch Microsoft's CRT and SDK, then stop
#
# What it runs, the Windows half of the CI clippy job (docs/CI.md):
#   cargo xwin clippy --target x86_64-pc-windows-msvc <crates> --all-targets -- -D warnings
# cargo-xwin (https://github.com/rust-cross/cargo-xwin) cross compiles with clang-cl and lld-link, and
# downloads the Windows headers and import libraries (the MSVC CRT and the Windows SDK) into
# ~/Library/Caches/cargo-xwin. Those files are Microsoft's, used under Microsoft's license for local
# development builds only: they are never committed and never shipped. A binary built here is for
# looking at, not for releasing. Releases come from CI.
#
# One time install (macOS):
#   brew install llvm lld              clang-cl, llvm-lib and lld-link (Apple's clang has no clang-cl)
#   cargo install cargo-xwin --locked
#   rustup target add x86_64-pc-windows-msvc
#
# The whole workspace is used instead of the changed crates when Cargo.lock or a Cargo.toml in the root
# changed, or when --all is given: the rule of tools/preflight.sh (tools/ci/changed_crates.py). The run is at low
# priority (nice -n 15, 8 build jobs), holds the machine wide heavy build lock (only one heavy build at
# a time), and builds into <worktree>-win.noindex next to the worktree, one per worktree (cargo hashes
# workspace crates the same in every worktree, so a shared folder mixes branches). The .noindex name keeps
# Spotlight out.
# Clippy only checks, so it never links or runs a Windows program. The first run compiles every
# dependency for Windows and is slow; later runs only recheck what changed.
set -euo pipefail

if [ -z "${PANOPLY_WINPRE_NICE:-}" ]; then
  PANOPLY_WINPRE_NICE=1 exec nice -n 15 "$0" "$@"
fi

all=0
build=0
plan_only=0
setup_only=0
for a in "$@"; do
  case "$a" in
    --all) all=1 ;;
    --build) build=1 ;;
    --plan) plan_only=1 ;;
    --setup) setup_only=1 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $a (see --help)" >&2; exit 2 ;;
  esac
done

cd "$(git rev-parse --show-toplevel)"

TARGET=x86_64-pc-windows-msvc

# ---- tools ---------------------------------------------------------------------------------------
# Homebrew's llvm and lld are keg only, so put them on the path for this script.
for d in /opt/homebrew/opt/llvm/bin /opt/homebrew/opt/lld/bin /usr/local/opt/llvm/bin /usr/local/opt/lld/bin; do
  [ -d "$d" ] && PATH="$d:$PATH"
done
export PATH

missing=""
for t in clang-cl llvm-lib lld-link cargo-xwin; do
  command -v "$t" > /dev/null 2>&1 || missing="$missing $t"
done
if [ -n "$missing" ]; then
  echo "missing:$missing" >&2
  echo "  brew install llvm lld && cargo install cargo-xwin --locked" >&2
  exit 1
fi
if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "missing the Rust target: rustup target add $TARGET" >&2
  exit 1
fi

# ---- environment ---------------------------------------------------------------------------------
repo_root="$(git rev-parse --show-toplevel)"
export CARGO_TARGET_DIR="${PANOPLY_WIN_TARGET_DIR:-${repo_root}-win.noindex}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}"
export CARGO_INCREMENTAL=0 PANOPLY_YES=1 CARGO_TERM_COLOR=always
# The Windows SDK and CRT cache of cargo-xwin. Keep it where cargo-xwin puts it by default, outside the
# repository, so nothing of Microsoft's can end up in a commit.
export XWIN_CACHE_DIR="${XWIN_CACHE_DIR:-$HOME/Library/Caches/cargo-xwin}"
# C dependencies built by clang-cl (cc-rs): mozjpeg-sys never defines HAVE_INTRIN_H, so under clang-cl
# _BitScanForward64 is undeclared; libwebp-sys compiles its SSE4.1 files without the SSE4.1 flag, which
# clang-cl does not pass on by itself. Both are set here, not in the code. Local checking builds only.
# cargo-xwin owns CFLAGS_x86_64_pc_windows_msvc and overwrites it; CL_FLAGS is its extra flags variable.
export CL_FLAGS="${CL_FLAGS:+$CL_FLAGS }-DHAVE_INTRIN_H -msse4.1"
mkdir -p "$CARGO_TARGET_DIR"

# ---- the heavy build lock ------------------------------------------------------------------------
# Only one heavy check at a time on this Mac (tools/lib/heavy-lock.sh). When tools/preflight.sh started
# this script it already holds the lock and acquiring is a no-op.
# shellcheck source=lib/heavy-lock.sh
. tools/lib/heavy-lock.sh
trap heavy_lock_release EXIT

start=$SECONDS
step() { printf '\n==> %s\n' "$*"; step_start=$SECONDS; }
done_step() { printf '    %ss\n' "$((SECONDS - step_start))"; }

# ---- what changed --------------------------------------------------------------------------------
base="$(git merge-base origin/main HEAD 2> /dev/null || echo HEAD)"
changed="$({ git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)"
[ -z "$changed" ] && echo "nothing has changed since origin/main"
# The same crate selection as tools/preflight.sh and tools/preflight-linux.sh (tools/ci/changed_crates.py).
plan="$(printf '%s\n' "$changed" | ALL="$all" python3 -I tools/ci/changed_crates.py)"

if [ -z "$plan" ] && [ "$build" = 0 ] && [ "$setup_only" = 0 ]; then
  echo "No Rust crate changed: nothing for Windows to check."
  exit 0
fi

if [ "$plan" = all ]; then
  scope="--workspace --exclude gpui-base"
  echo "scope: the whole workspace"
else
  scope="$plan"
  echo "scope: ${plan:-none}"
fi

if [ "$plan_only" = 1 ]; then
  echo "target dir: $CARGO_TARGET_DIR"
  [ "$build" = 1 ] && echo "then: cargo xwin build -p panoply-desktop --profile ci"
  exit 0
fi

if [ "$setup_only" = 1 ]; then
  step "fetching Microsoft's CRT and Windows SDK into $XWIN_CACHE_DIR"
  # Any cargo-xwin command downloads them first. Checking a tiny crate is the cheapest.
  # shellcheck disable=SC2086
  cargo xwin check --target "$TARGET" -p panoply-manifest
  done_step
  exit 0
fi

heavy_lock_acquire

# ---- the checks ----------------------------------------------------------------------------------
if [ -n "$plan" ]; then
  step "cargo xwin clippy --target $TARGET (-D warnings)"
  # shellcheck disable=SC2086
  cargo xwin clippy --target "$TARGET" $scope --all-targets -- -D warnings
  done_step
fi

if [ "$build" = 1 ]; then
  step "cargo xwin build panoply-desktop.exe (profile ci, not run)"
  cargo xwin build --target "$TARGET" -p panoply-desktop --bin panoply-desktop --profile ci
  done_step
  ls -l "$CARGO_TARGET_DIR/$TARGET/ci/panoply-desktop.exe"
fi

printf '\nwindows preflight passed in %ss\n' "$((SECONDS - start))"
