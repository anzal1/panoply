#!/usr/bin/env bash
# Linux desktop smoke test. Runs the release AppImage under Xvfb with the openbox window manager and
# software Vulkan, and records screenshots and facts.
set -uo pipefail
DL="$(cd "$1" && pwd)"; mkdir -p "$2"; OUT="$(cd "$2" && pwd)"
APPIMAGE="$(ls "$DL"/*.AppImage | head -1)"; chmod +x "$APPIMAGE"

export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 -dpi 96 >"$OUT/xvfb.log" 2>&1 &
sleep 2
eval "$(dbus-launch --sh-syntax)"
openbox >"$OUT/openbox.log" 2>&1 &
sleep 2
vulkaninfo --summary >"$OUT/vulkaninfo.txt" 2>&1 || true

# Fonts the system offers (the app should not need any of them to draw its own text).
fc-list : family | sort -u >"$OUT/fonts.txt"

export APPIMAGE_EXTRACT_AND_RUN=1
export RUST_BACKTRACE=1
failed=()
runs=()

pid_of() { pgrep -x panoply-desktop | head -1; }
# The command bar is a 720x460 pop-up the window manager does not manage, so it shows up in the window tree.
bar_mapped() { xwininfo -root -tree 2>/dev/null | grep -c ' 720x460+' || true; }
window_id() { xdotool search --onlyvisible --name '^Panoply$' 2>/dev/null | head -1; }

for scenario in default; do
  rm -rf "$HOME/.local/share/panoply/"*window* 2>/dev/null || true
  if [ -n "${SMOKE_STRACE:-}" ]; then
    strace -f -tt -s 200 -e trace=connect,execve,openat,write -o "$OUT/strace.txt" "$APPIMAGE" >"$OUT/stdout.txt" 2>"$OUT/stderr.txt" &
  else
    "$APPIMAGE" >"$OUT/stdout.txt" 2>"$OUT/stderr.txt" &
  fi
  launcher=$!
  # Poll for the window instead of a fixed sleep, and record how long it took.
  t0=$(date +%s.%N); shown=""
  for i in $(seq 1 120); do
    shown="$(window_id || true)"; [ -n "$shown" ] && break
    sleep 0.5
    [ "$i" = 24 ] && ps -eo pid,ppid,stat,rss,etime,args | grep -i panoply | grep -v grep >"$OUT/ps.txt"
  done
  t_window=$(python3 -c "print(round($(date +%s.%N)-$t0,1))")
  sleep 3
  [ -n "$(pid_of || true)" ] && { cat "/proc/$(pid_of)/status" >"$OUT/proc-status.txt"; ls -l "/proc/$(pid_of)/fd" >"$OUT/proc-fd.txt" 2>&1; for t in /proc/$(pid_of)/task/*; do echo "$(cat $t/comm) $(cat $t/wchan 2>/dev/null)"; done >"$OUT/proc-threads.txt"; }
  pid="$(pid_of || true)"
  alive=false; [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && alive=true
  wid="$(window_id || true)"
  title=""; geom=""; frame=""; motif=""; rss=0
  if [ -n "$wid" ]; then
    title="$(xdotool getwindowname "$wid")"
    geom="$(xdotool getwindowgeometry "$wid" | tr '\n' ' ')"
    frame="$(xprop -id "$wid" _NET_FRAME_EXTENTS 2>/dev/null)"
    motif="$(xprop -id "$wid" _MOTIF_WM_HINTS 2>/dev/null)"
    xprop -id "$wid" >"$OUT/xprop-app-window.txt" 2>&1 || true
  fi
  xwininfo -root -tree >"$OUT/xwininfo-tree.txt" 2>&1 || true
  wmctrl -lG >"$OUT/wmctrl.txt" 2>&1 || true
  [ -n "$pid" ] && rss=$(ps -o rss= -p "$pid" | tr -d ' ')
  mem_mb=$(( ${rss:-0} / 1024 ))
  import -window root "$OUT/linux-app-screen.png"
  bar_at_start=$(bar_mapped)
  if [ -n "$wid" ]; then import -window "$wid" "$OUT/linux-app-window.png" 2>/dev/null || true; fi

  # The command bar, through the global hotkey: it must show, then hide on Escape, then show and hide
  # again with the hotkey itself.
  xdotool key --clearmodifiers ctrl+alt+space
  sleep 2
  import -window root "$OUT/linux-bar-screen.png"
  bar_after_hotkey=$(bar_mapped)
  wmctrl -lG >"$OUT/wmctrl-after-hotkey.txt" 2>&1 || true
  xwininfo -root -tree >"$OUT/xwininfo-tree-after-hotkey.txt" 2>&1 || true
  alive_after_hotkey=false; [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && alive_after_hotkey=true
  xdotool key --clearmodifiers Escape
  sleep 1
  import -window root "$OUT/linux-bar-after-escape.png"
  bar_after_escape=$(bar_mapped)
  xdotool key --clearmodifiers ctrl+alt+space
  sleep 2
  bar_second_show=$(bar_mapped)
  xdotool key --clearmodifiers ctrl+alt+space
  sleep 1
  import -window root "$OUT/linux-bar-after-toggle.png"
  bar_after_toggle=$(bar_mapped)

  # Close the window through the window manager, like the close button.
  exited=false; code=""
  [ -n "$wid" ] && wmctrl -i -c "$(printf '0x%x' "$wid")"
  for i in $(seq 1 15); do
    if [ -z "$(pid_of || true)" ]; then exited=true; break; fi
    sleep 1
  done
  if $exited; then wait "$launcher"; code=$?; else pkill -x panoply-desktop || true; wait "$launcher" 2>/dev/null || true; fi

  [ "$alive" = true ] || failed+=("process not alive after the window wait")
  [ -n "$wid" ] || failed+=("no visible window titled Panoply")
  [ "${bar_at_start:-0}" = 0 ] || failed+=("command bar on screen before the hotkey")
  [ "${bar_after_hotkey:-0}" != 0 ] || failed+=("command bar did not show on the hotkey")
  [ "${bar_after_escape:-0}" = 0 ] || failed+=("command bar still on screen after Escape")
  [ "${bar_second_show:-0}" != 0 ] || failed+=("command bar did not show on the second hotkey")
  [ "${bar_after_toggle:-0}" = 0 ] || failed+=("command bar still on screen after the hotkey toggled it off")
  if $exited && [ "$code" != 0 ]; then failed+=("exit code $code after close"); fi
  runs+=("$(python3 -I - "$scenario" "$alive" "$wid" "$title" "$geom" "$frame" "$motif" "$mem_mb" "$alive_after_hotkey" "$exited" "$code" "$t_window" "$bar_at_start" "$bar_after_hotkey" "$bar_after_escape" "$bar_second_show" "$bar_after_toggle" <<'PY'
import json, sys
k = ["scenario","alive","window_id","title","geometry","frame_extents","motif_hints","memory_mb","alive_after_hotkey","exited_after_close","exit_code","seconds_to_window","bar_at_start","bar_after_hotkey","bar_after_escape","bar_second_show","bar_after_toggle"]
d = dict(zip(k, sys.argv[1:]))
for b in ("alive","alive_after_hotkey","exited_after_close"): d[b] = d[b] == "true"
d["memory_mb"] = int(d["memory_mb"]); d["seconds_to_window"] = float(d["seconds_to_window"])
for b in ("bar_at_start","bar_after_hotkey","bar_after_escape","bar_second_show","bar_after_toggle"): d[b] = int(d[b])
print(json.dumps(d))
PY
)")
done

python3 -I - "$OUT" "${failed[@]+"${failed[@]}"}" <<PY
import json, sys, subprocess
out = sys.argv[1]
runs = [json.loads(l) for l in '''$(printf '%s\n' "${runs[@]}")'''.strip().splitlines()]
rep = {"os": subprocess.run(["lsb_release","-ds"],capture_output=True,text=True).stdout.strip(),
       "screen": "1920x1080", "appimage": "$(basename "$APPIMAGE")", "runs": runs, "failed": sys.argv[2:]}
open(out + "/report.json", "w").write(json.dumps(rep, indent=2))
print(json.dumps(rep, indent=2))
PY
tail -20 "$OUT/stderr.txt" || true
if [ "${#failed[@]}" -gt 0 ]; then echo "::error::${failed[*]}"; exit 1; fi
