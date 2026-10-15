# shellcheck shell=bash
# A machine-wide lock so only one heavy check (a build, clippy, a test run, a VM) runs at a time across
# every worktree and every agent on this Mac. Source it, then:
#
#     heavy_lock_acquire     waits ("waiting for another check") while another check holds the lock
#     heavy_lock_release     call it from the EXIT trap
#
# The lock is a directory made with mkdir (atomic) holding the holder's PID. A lock whose PID is gone is
# stale and is taken over. A child script started by a holder (preflight.sh runs preflight-linux.sh) sees
# PANOPLY_HEAVY_LOCK_HELD and does not wait for its own parent.
#
# PANOPLY_HEAVY_LOCK moves the lock directory (tests only). tools/preflight-windows.sh sources this too.

HEAVY_LOCK_DIR="${PANOPLY_HEAVY_LOCK:-/Users/anzalhussainabidi/personal/.panoply-heavy.lock}"
HEAVY_LOCK_OWNER=""

heavy_lock_acquire() {
  local dir="$HEAVY_LOCK_DIR" holder="" announced=0 empty=0 waited_since=$SECONDS
  # Inherited from a parent that holds it: nothing to do.
  if [ -n "${PANOPLY_HEAVY_LOCK_HELD:-}" ] && [ "$(cat "$dir/pid" 2>/dev/null || true)" = "$PANOPLY_HEAVY_LOCK_HELD" ] \
    && kill -0 "$PANOPLY_HEAVY_LOCK_HELD" 2>/dev/null; then
    return 0
  fi
  mkdir -p "$(dirname "$dir")"
  while ! mkdir "$dir" 2>/dev/null; do
    holder="$(cat "$dir/pid" 2>/dev/null || true)"
    if [ -z "$holder" ]; then
      # The holder made the directory a moment ago and has not written its PID yet. Give it 10 s.
      empty=$((empty + 1))
      if [ "$empty" -gt 10 ]; then
        rm -rf "$dir"
        empty=0
      fi
      sleep 1
      continue
    fi
    empty=0
    if ! kill -0 "$holder" 2>/dev/null; then
      echo "heavy lock: holder $holder is gone, taking over a stale lock" >&2
      # Move it aside first (atomic), and only when it still names the dead PID.
      if [ "$(cat "$dir/pid" 2>/dev/null || true)" = "$holder" ]; then
        mv "$dir" "$dir.stale.$$" 2>/dev/null && rm -rf "$dir.stale.$$"
      fi
      continue
    fi
    if [ "$announced" = 0 ] || [ $((SECONDS - waited_since)) -ge 60 ]; then
      printf 'waiting for another check (held by pid %s: %s)\n' "$holder" "$(cat "$dir/what" 2>/dev/null || echo unknown)" >&2
      announced=1
      waited_since=$SECONDS
    fi
    sleep 5
  done
  printf '%s\n' "$$" >"$dir/pid"
  printf '%s\n' "$(basename "${0:-check}") in $PWD since $(date +%H:%M:%S)" >"$dir/what"
  HEAVY_LOCK_OWNER=$$
  export PANOPLY_HEAVY_LOCK_HELD=$$
}

heavy_lock_release() {
  if [ -n "$HEAVY_LOCK_OWNER" ] && [ "$(cat "$HEAVY_LOCK_DIR/pid" 2>/dev/null || true)" = "$HEAVY_LOCK_OWNER" ]; then
    rm -rf "$HEAVY_LOCK_DIR"
  fi
  HEAVY_LOCK_OWNER=""
}
