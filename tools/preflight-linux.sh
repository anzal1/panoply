#!/usr/bin/env bash
# The Linux half of the preflight: catches Linux-only compile and lint errors on this Mac in minutes
# instead of in a 25-minute CI round trip.
#
#     tools/preflight-linux.sh            clippy for the whole workspace, then nextest on the crates changed
#                                         since origin/main and every crate that depends on them
#     tools/preflight-linux.sh --all      nextest on the whole workspace too
#     tools/preflight-linux.sh --heavy    also the heavy corpus and model tests (profile `ci`)
#     tools/preflight-linux.sh --smoke    also build panoply-desktop and run it under Xvfb and openbox: a
#                                         window titled Panoply appears, the bar is not mapped before the
#                                         hotkey, and a screenshot (xwd and png) is written
#     tools/preflight-linux.sh --keep     leave the VM running when the check ends (default: stop it)
#     tools/preflight-linux.sh --rebuild  rebuild the image from scratch (picks up a newer stable Rust)
#     tools/preflight-linux.sh --plan     only print what would run
#
# tools/preflight.sh calls this itself when you pass --linux or when a changed file holds Linux-only code.
#
# How it works: a colima VM (Apple Virtualization, native arm64 Linux, 4 CPUs and 4 GB of RAM, profile
# "panoply" so your own default VM is never touched) runs one container of tools/linux/Dockerfile (Ubuntu
# 24.04, the toolchain from rust-toolchain.toml, the apt packages of .github/actions/linux-libs). The repo
# is bind-mounted read-only; inside, it is synced to a work volume (rsync keeps the modification times, so
# cargo rebuilds only what you changed). Named docker volumes keep the cargo registry, the downloaded
# engines and, per checkout (keyed on a hash of its path, so branches never share build output), the
# Linux target folder and the work folder warm between runs. Everything runs at nice 15, and only one heavy check runs on the Mac at a time (tools/lib/heavy-lock.sh). When the check ends the
# VM is stopped (`colima stop`), so it costs nothing while idle, and your docker context is put back.
#
# Environment: PANOPLY_COLIMA_PROFILE (default panoply), PANOPLY_LINUX_JOBS (cargo jobs, default 4),
# PANOPLY_SMOKE_OUT (where the screenshots go, default ~/.cache/panoply/linux-smoke).
set -euo pipefail

all=0 heavy=0 smoke=0 keep=0 rebuild=0 plan_only=0
for a in "$@"; do
  case "$a" in
    --all) all=1 ;;
    --heavy) heavy=1 ;;
    --smoke) smoke=1 ;;
    --keep) keep=1 ;;
    --rebuild) rebuild=1 ;;
    --plan) plan_only=1 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | grep '^#' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $a (see --help)" >&2; exit 2 ;;
  esac
done

cd "$(git rev-parse --show-toplevel)"
# shellcheck source=lib/heavy-lock.sh
. tools/lib/heavy-lock.sh
for tool in colima docker; do
  command -v "$tool" >/dev/null || { echo "preflight-linux: $tool is not installed (brew install colima docker)" >&2; exit 3; }
done

profile="${PANOPLY_COLIMA_PROFILE:-panoply}"
ctx="colima-$profile"
[ "$profile" = default ] && ctx=colima
# 4 jobs: the VM has 4 CPUs and 3.7 GB, and 8 rustc processes (cranelift-codegen above all) were OOM-killed.
jobs="${PANOPLY_LINUX_JOBS:-4}"
# The Linux target and work folders are per checkout: cargo hashes workspace crates the same in every
# worktree, so one shared target let a build from another branch stand in for this one (and a test ran
# another branch's code). The registry, git and engine volumes hold content that is the same for all.
wt="$(printf '%s' "$PWD" | shasum | cut -c1-10)"
smoke_out="${PANOPLY_SMOKE_OUT:-$HOME/.cache/panoply/linux-smoke}"
start=$SECONDS
step() { printf '\n==> [linux] %s\n' "$*"; step_start=$SECONDS; }
done_step() { printf '    %ss\n' "$((SECONDS - step_start))"; }
d() { docker --context "$ctx" "$@"; }

# ---- what changed (the same logic as tools/preflight.sh) ----------------------------------------
base="$(git merge-base origin/main HEAD 2>/dev/null || echo HEAD)"
changed="$({ git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)"
plan="$(printf '%s\n' "$changed" | ALL="$all" python3 -I tools/ci/changed_crates.py)"
if [ -z "$plan" ] && [ "$smoke" = 0 ]; then
  echo "[linux] No Rust crate changed: nothing to check on Linux."
  exit 0
fi
if [ "$plan" = all ] || [ "$all" = 1 ]; then
  scope="--workspace --exclude gpui-base"
  echo "[linux] nextest scope: the whole workspace"
else
  scope="$plan"
  echo "[linux] nextest scope: ${plan:-none}"
fi
echo "[linux] clippy scope: the whole workspace (as in CI)"
[ "$plan_only" = 1 ] && exit 0

# ---- the VM -------------------------------------------------------------------------------------
# `colima start` makes its own docker context the current one; remember yours and put it back.
prev_ctx="$(docker context show 2>/dev/null || echo default)"
cleanup() {
  status=$?
  trap - EXIT
  vm_mine=0
  if [ -n "${vm_lock:-}" ] && [ "$(cat "$vm_lock/pid" 2>/dev/null || true)" = "$$" ]; then vm_mine=1; fi
  if [ "$vm_mine" = 0 ]; then
    :  # never got the VM, so never stop it
  elif [ "$keep" = 0 ]; then
    colima stop "$profile" >/dev/null 2>&1 && echo "[linux] VM stopped (colima stop $profile)" || true
  else
    echo "[linux] VM left running (--keep); stop it with: colima stop $profile"
  fi
  docker context use "$prev_ctx" >/dev/null 2>&1 || true
  if [ "$vm_mine" = 1 ]; then rm -rf "$vm_lock"; fi
  heavy_lock_release
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Only one heavy check at a time on this Mac (tools/lib/heavy-lock.sh); waits when another holds it.
heavy_lock_acquire
# The VM is shared by every worktree. Only one Linux check uses it at a time, and only the holder stops
# it, so one agent's cleanup never kills another agent's run.
vm_lock="${PANOPLY_LINUX_VM_LOCK:-/Users/anzalhussainabidi/personal/.panoply-linux-vm.lock}"
vm_announced=0
until mkdir "$vm_lock" 2>/dev/null; do
  vm_holder="$(cat "$vm_lock/pid" 2>/dev/null || true)"
  if [ -n "$vm_holder" ] && ! kill -0 "$vm_holder" 2>/dev/null; then
    rm -rf "$vm_lock"
    continue
  fi
  if [ "$vm_announced" = 0 ]; then echo "[linux] waiting for another Linux check to finish with the VM" >&2; vm_announced=1; fi
  sleep 5
done
printf '%s\n' "$$" >"$vm_lock/pid"

if ! colima status "$profile" >/dev/null 2>&1; then
  step "starting the VM (4 CPUs, 4 GB, no windows)"
  colima start "$profile" --cpu 4 --memory 4 --disk 60 --vm-type vz --mount-type virtiofs >/dev/null 2>&1 \
    || { echo "[linux] colima start failed; run: colima start $profile --cpu 4 --memory 4 --vm-type vz --mount-type virtiofs" >&2; exit 4; }
  docker context use "$prev_ctx" >/dev/null 2>&1 || true
  done_step
fi

# ---- the image: rebuilt only when the Dockerfile or rust-toolchain.toml change --------------------
tag="panoply-linux-check:$(cat tools/linux/Dockerfile rust-toolchain.toml | shasum | cut -c1-12)"
if [ "$rebuild" = 1 ] || ! d image inspect "$tag" >/dev/null 2>&1; then
  step "building the image $tag (a few minutes, once)"
  ctxdir="$(mktemp -d)"
  cp tools/linux/Dockerfile rust-toolchain.toml "$ctxdir/"
  extra=""
  [ "$rebuild" = 1 ] && extra="--no-cache --pull"
  # shellcheck disable=SC2086
  d build $extra -q -t "$tag" "$ctxdir" >/dev/null
  rm -rf "$ctxdir"
  done_step
fi

# ---- the test corpus: the cache of this checkout, or of the first worktree --------------------------
# Only the kinds CI fetches (pdf and image, see ci.yml): the video, audio and office files a Mac may have
# generated need tools the container does not have, and CI never sees them.
corpus=""
if [ -e tests/corpus/cache ]; then
  corpus="$(cd tests/corpus/cache && pwd -P)"
else
  first="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
  [ -d "$first/tests/corpus/cache" ] && corpus="$first/tests/corpus/cache"
fi
corpus_args=()
if [ -n "$corpus" ]; then
  for kind in pdf image; do
    [ -d "$corpus/$kind" ] && corpus_args+=(-v "$corpus/$kind":/corpus/$kind:ro)
  done
fi
[ "${#corpus_args[@]}" -gt 0 ] || echo "[linux] no corpus cache found: corpus tests will fail as they do in CI without one"

profile_nx=ci-core
[ "$heavy" = 1 ] && profile_nx=ci
mkdir -p "$smoke_out"

step "running in the container"
set +e
d run --rm --cpus 4 --memory 3700m --memory-swap 3700m \
  -v "$PWD":/src:ro \
  "${corpus_args[@]}" \
  -v panoply-linux-registry:/usr/local/cargo/registry \
  -v panoply-linux-git:/usr/local/cargo/git \
  -v "panoply-linux-target-$wt":/cache/target \
  -v panoply-linux-engines:/cache/engines \
  -v "panoply-linux-work-$wt":/work \
  -v "$smoke_out":/out \
  -e SCOPE="$scope" -e NEXTEST_PROFILE="$profile_nx" -e SMOKE="$smoke" -e JOBS="$jobs" -e PLAN="$plan" \
  "$tag" bash /src/tools/linux/inner.sh
code=$?
set -e

printf '\n[linux] preflight-linux %s in %ss (wall time, VM start and image build included)\n' \
  "$([ "$code" = 0 ] && echo passed || echo "FAILED (exit $code)")" "$((SECONDS - start))"
[ "$smoke" = 1 ] && echo "[linux] smoke screenshots and report: $smoke_out"
exit "$code"
