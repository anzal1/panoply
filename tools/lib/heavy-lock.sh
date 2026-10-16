# shellcheck shell=bash
# A machine-wide lock with a small number of slots (PANOPLY_HEAVY_SLOTS, default 2), so at most that many
# heavy checks (a build, clippy, a test run, a VM) run at once across every worktree and every agent on
# this Mac. Each holder gets CARGO_BUILD_JOBS=6, so two holders use at most 12 of the 18 cores. Source it,
# then:
#
#     heavy_lock_acquire     waits ("waiting for another check") while another check holds the lock
#     heavy_lock_release     call it from the EXIT trap
#
# The lock is a directory made with mkdir (atomic) holding the holder's PID. A lock whose PID is gone is
# stale and is taken over. A child script started by a holder (preflight.sh runs preflight-linux.sh) sees
# PANOPLY_HEAVY_LOCK_HELD and does not wait for its own parent.
#
# PANOPLY_HEAVY_LOCK moves the lock directory (tests only). tools/preflight-windows.sh sources this too.

HEAVY_LOCK_BASE="${PANOPLY_HEAVY_LOCK:-/Users/anzalhussainabidi/personal/.panoply-heavy.lock}"
HEAVY_LOCK_SLOTS="${PANOPLY_HEAVY_SLOTS:-2}"
HEAVY_LOCK_DIR="$HEAVY_LOCK_BASE"
HEAVY_LOCK_OWNER=""

# Slot 1 is the base path (older copies of this script only know that one), slot n is base.n.
heavy_lock_slot() { if [ "$1" = 1 ]; then echo "$HEAVY_LOCK_BASE"; else echo "$HEAVY_LOCK_BASE.$1"; fi; }

# Try every slot once. Clears a slot whose holder is gone. Returns 0 with HEAVY_LOCK_DIR set on success.
heavy_lock_try() {
  local i dir holder
  for i in $(seq 1 "$HEAVY_LOCK_SLOTS"); do
    dir="$(heavy_lock_slot "$i")"
    if mkdir "$dir" 2>/dev/null; then
      HEAVY_LOCK_DIR="$dir"
      return 0
    fi
    holder="$(cat "$dir/pid" 2>/dev/null || true)"
    if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
      echo "heavy lock: holder $holder is gone, taking over a stale lock" >&2
      if [ "$(cat "$dir/pid" 2>/dev/null || true)" = "$holder" ]; then
        mv "$dir" "$dir.stale.$$" 2>/dev/null && rm -rf "$dir.stale.$$"
      fi
    elif [ -z "$holder" ] && [ -d "$dir" ]; then
      # Made a moment ago and no PID yet, or abandoned empty. Clear it if it stays empty for 10 s.
      if [ -n "$(find "$dir" -maxdepth 0 -mmin +0.17 2>/dev/null)" ]; then rm -rf "$dir"; fi
    fi
  done
  return 1
}

heavy_lock_acquire() {
  local announced=0 waited_since=$SECONDS i
  # Inherited from a parent that holds a slot: nothing to do.
  if [ -n "${PANOPLY_HEAVY_LOCK_HELD:-}" ] && kill -0 "$PANOPLY_HEAVY_LOCK_HELD" 2>/dev/null; then
    for i in $(seq 1 "$HEAVY_LOCK_SLOTS"); do
      if [ "$(cat "$(heavy_lock_slot "$i")/pid" 2>/dev/null || true)" = "$PANOPLY_HEAVY_LOCK_HELD" ]; then
        return 0
      fi
    done
  fi
  mkdir -p "$(dirname "$HEAVY_LOCK_BASE")"
  until heavy_lock_try; do
    if [ "$announced" = 0 ] || [ $((SECONDS - waited_since)) -ge 60 ]; then
      printf 'waiting for another check (%s slots busy)\n' "$HEAVY_LOCK_SLOTS" >&2
      announced=1
      waited_since=$SECONDS
    fi
    sleep 5
  done
  printf '%s\n' "$$" >"$HEAVY_LOCK_DIR/pid"
  printf '%s\n' "$(basename "${0:-check}") in $PWD since $(date +%H:%M:%S)" >"$HEAVY_LOCK_DIR/what"
  HEAVY_LOCK_OWNER=$$
  export PANOPLY_HEAVY_LOCK_HELD=$$
  # Two holders at once: keep each to 6 compile jobs.
  export CARGO_BUILD_JOBS=6
}

heavy_lock_release() {
  if [ -n "$HEAVY_LOCK_OWNER" ] && [ "$(cat "$HEAVY_LOCK_DIR/pid" 2>/dev/null || true)" = "$HEAVY_LOCK_OWNER" ]; then
    rm -rf "$HEAVY_LOCK_DIR"
  fi
  HEAVY_LOCK_OWNER=""
}
