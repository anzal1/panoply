#!/usr/bin/env bash
# Headless Linux desktop smoke, inside the container of tools/preflight-linux.sh (--smoke). The same idea as
# .github/scripts/desktop-smoke-linux.sh (which tests the released AppImage on a runner), but for a build of
# the working tree: Xvfb, the openbox window manager and software Vulkan.
#
#     smoke.sh <panoply-desktop binary> <output folder>
#
# Checks: the process stays alive, a window titled Panoply becomes visible, the command bar (720x460 pop-up)
# is NOT mapped before the hotkey, and the window closes cleanly through the window manager. Writes an xwd
# screenshot (and a png of it) and report.json. Whether the bar shows after the hotkey is recorded, not
# enforced: a synthetic key press under Xvfb is not a promise.
set -uo pipefail
BIN="$1"; mkdir -p "$2"; OUT="$(cd "$2" && pwd)"
BAR_W="${PANOPLY_BAR_W:-720}"; BAR_H="${PANOPLY_BAR_H:-460}"
rm -f "$OUT"/*.xwd "$OUT"/*.png "$OUT"/*.log "$OUT"/report.json
failed=()

export DISPLAY=:99 RUST_BACKTRACE=1
export PANOPLY_DATA_DIR="$(mktemp -d)" HOME="$(mktemp -d)"
Xvfb :99 -screen 0 1920x1080x24 -dpi 96 +extension Composite >"$OUT/xvfb.log" 2>&1 &
for _ in $(seq 1 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
eval "$(dbus-launch --sh-syntax)"
openbox >"$OUT/openbox.log" 2>&1 &
for _ in $(seq 1 50); do wmctrl -m >/dev/null 2>&1 && break; sleep 0.2; done
wmctrl -m >/dev/null 2>&1 || failed+=("the window manager did not start")

mapped_bar() {  # prints the id of a visible window of the bar's size, if any
  local w g
  for w in $(xdotool search --onlyvisible --pid "$1" 2>/dev/null); do
    g="$(xdotool getwindowgeometry "$w" 2>/dev/null | awk '/Geometry/{print $2}')"
    [ "$g" = "${BAR_W}x${BAR_H}" ] && { echo "$w"; return; }
  done
}

"$BIN" >"$OUT/stdout.log" 2>"$OUT/stderr.log" &
pid=$!
wid=""; t0=$SECONDS
for _ in $(seq 1 120); do
  wid="$(xdotool search --onlyvisible --name '^Panoply$' 2>/dev/null | head -1)"
  [ -n "$wid" ] && break
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.5
done
t_window=$((SECONDS - t0))
sleep 2  # settle: the bar is created hidden at start, give it the chance to show itself wrongly
alive=false; kill -0 "$pid" 2>/dev/null && alive=true
[ "$alive" = true ] || failed+=("the process is not alive after the window wait")
[ -n "$wid" ] || failed+=("no visible window titled Panoply")

bar_before="$(mapped_bar "$pid")"
[ -z "$bar_before" ] || failed+=("the command bar is mapped before the hotkey (window $bar_before)")

xwd -root -silent -out "$OUT/before-hotkey.xwd" 2>/dev/null || failed+=("xwd screenshot failed")
convert "xwd:$OUT/before-hotkey.xwd" "$OUT/before-hotkey.png" 2>/dev/null || true

xdotool key --clearmodifiers ctrl+alt+space
sleep 2
bar_after="$(mapped_bar "$pid")"
xwd -root -silent -out "$OUT/after-hotkey.xwd" 2>/dev/null || true
convert "xwd:$OUT/after-hotkey.xwd" "$OUT/after-hotkey.png" 2>/dev/null || true

exited=false; code=""
if [ -n "$wid" ]; then wmctrl -i -c "$(printf '0x%x' "$wid")"; fi
for _ in $(seq 1 15); do
  kill -0 "$pid" 2>/dev/null || { exited=true; break; }
  sleep 1
done
if $exited; then wait "$pid"; code=$?; else kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; failed+=("the app did not exit after its window was closed"); fi
[ -z "$code" ] || [ "$code" = 0 ] || failed+=("exit code $code after close")
grep -q "panicked" "$OUT/stderr.log" && failed+=("the app panicked (see stderr.log)")

python3 -I - "$OUT" "$alive" "${wid:-}" "$t_window" "${bar_before:-}" "${bar_after:-}" "${failed[@]+"${failed[@]}"}" <<'PY'
import json, sys
out, alive, wid, t, before, after, *failed = sys.argv[1:]
rep = {"alive": alive == "true", "window_id": wid or None, "seconds_to_window": int(t),
       "bar_mapped_before_hotkey": bool(before), "bar_mapped_after_hotkey": bool(after), "failed": failed}
open(out + "/report.json", "w").write(json.dumps(rep, indent=2))
print(json.dumps(rep, indent=2))
PY
echo "stderr tail:"; tail -8 "$OUT/stderr.log"
if [ "${#failed[@]}" -gt 0 ]; then echo "smoke FAILED: ${failed[*]}"; exit 1; fi
echo "smoke passed"
