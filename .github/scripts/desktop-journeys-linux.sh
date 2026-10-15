#!/usr/bin/env bash
# Linux user journeys. Runs the release AppImage under Xvfb with openbox and software Vulkan, and uses it
# the way a person does: the hotkey, typing into the command bar and the search box with xdotool, Enter,
# Ctrl+Z, the window manager's close, Ctrl+Q. Every journey records pass or fail, the seconds it took and
# screenshots. Verification is by files on disk, the freedesktop trash, screenshots (cropped to the app
# window and compared for blankness and brightness) and tesseract OCR of those screenshots.
#
# Status words in the report: pass; fail (the app did not do what the journey expects); warn (it worked
# but missed a stated target); error (the script broke or could not check, never an app bug); skip.
#
#   desktop-journeys-linux.sh DOWNLOAD_DIR OUT_DIR [JOURNEY_IDS_COMMA_SEPARATED]
set -uo pipefail
DL="$(cd "$1" && pwd)"; mkdir -p "$2"; OUT="$(cd "$2" && pwd)"; ONLY="${3:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
FIX="$HERE/journeys-fixtures.py"
PY="${PY:-python3}"
WORK="$HOME/journeys"; rm -rf "$WORK"; mkdir -p "$WORK"
APPIMAGE="$(ls "$DL"/*.AppImage | head -1)"; chmod +x "$APPIMAGE"
TRASH="$HOME/.local/share/Trash"

export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 -dpi 96 +extension Composite >"$OUT/xvfb.log" 2>&1 &
sleep 2
eval "$(dbus-launch --sh-syntax)"
openbox >"$OUT/openbox.log" 2>&1 &
sleep 2
export APPIMAGE_EXTRACT_AND_RUN=1 RUST_BACKTRACE=1

# ---- the journey recorder ---------------------------------------------------------------------------
RESULTS="$OUT/results.jsonl"; : >"$RESULTS"
J_ID=""; J_STATUS=pass; JDIR=""; SESS_OK=0; SESS_FILE=""; SESS_DIR=""
want() { [ -z "$ONLY" ] && return 0; case ",$ONLY," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }
check() { # check "what" ok(0|1) ["detail"]
  printf '%s\t%s\t%s\n' "$2" "$1" "${3:-}" >>"$JDIR/.checks"
  echo "  [$([ "$2" = 1 ] && echo ok || echo FAIL)] $1 ${3:-}"
  if [ "$2" != 1 ] && { [ "$J_STATUS" = pass ] || [ "$J_STATUS" = warn ]; }; then J_STATUS=fail; fi
}
ok_if() { if eval "$2"; then echo 1; else echo 0; fi; }
warn() { echo "warn: $1 ${2:-}" >>"$JDIR/.notes"; echo "  [warn] $1 ${2:-}"; [ "$J_STATUS" = pass ] && J_STATUS=warn; return 0; }
limit() { echo "script limitation: $1" >>"$JDIR/.notes"; echo "  [limit] $1"; { [ "$J_STATUS" = pass ] || [ "$J_STATUS" = warn ]; } && J_STATUS=error; return 0; }
skip() { echo "skipped: $1" >>"$JDIR/.notes"; J_STATUS=skip; }
note() { echo "$1" >>"$JDIR/.notes"; echo "  note: $1"; }
data() { printf '%s\t%s\n' "$1" "$2" >>"$JDIR/.data"; }

run_journey() { # run_journey ID "name" function
  want "$1" || return 0
  echo "::group::journey $1 $2"
  J_ID="$1"; J_STATUS=pass
  JDIR="$OUT/j$1"; mkdir -p "$JDIR"; : >"$JDIR/.checks"; : >"$JDIR/.notes"; : >"$JDIR/.data"
  [ "$1" = 5 ] || stop_all
  local t0; t0=$(date +%s.%N)
  "$3" || true
  local secs; secs=$(python3 -c "print(round($(date +%s.%N)-$t0,1))")
  python3 -I - "$JDIR" "$1" "$2" "$J_STATUS" "$secs" >>"$RESULTS" <<'PY'
import json, os, sys
d, jid, name, status, secs = sys.argv[1:6]
def lines(p): return [l.rstrip("\n") for l in open(p)] if os.path.exists(p) else []
checks = []
for l in lines(d + "/.checks"):
    ok, what, *detail = l.split("\t") + [""]
    checks.append({"check": what, "ok": ok == "1", "detail": detail[0] if detail else ""})
data = {}
for l in lines(d + "/.data"):
    k, _, v = l.partition("\t"); data[k] = v
shots = sorted(f for f in os.listdir(d) if f.endswith(".png") and ".ocr" not in f)
print(json.dumps({"id": jid, "name": name, "status": status, "seconds": float(secs), "checks": checks,
                  "notes": lines(d + "/.notes"), "screenshots": shots, "data": data}))
PY
  echo "journey $1: $J_STATUS in ${secs} s"
  echo "::endgroup::"
}

# ---- windows, input, screenshots --------------------------------------------------------------------
pid_of() { pgrep -x panoply-desktop | head -1; }
alive() { [ -n "$(pid_of)" ]; }
stop_all() { pkill -x panoply-desktop 2>/dev/null; sleep 1; pkill -9 -x panoply-desktop 2>/dev/null; return 0; }
win_main() { xdotool search --onlyvisible --name '^Panoply$' 2>/dev/null | head -1; }
# The bar is a 720x460 pop-up that the window manager does not manage: find it in the window tree.
bar_line() { xwininfo -root -tree 2>/dev/null | grep ' 720x460+' | head -1; }
bar_mapped() { [ -n "$(bar_line)" ]; }
bar_id() { bar_line | awk '{print $1}'; }
geom_of() { # main | bar  -> WxH+X+Y of the window on the root
  local id
  if [ "$1" = bar ]; then id="$(bar_id)"; else id="$(win_main)"; fi
  [ -n "$id" ] || return 1
  xwininfo -id "$id" 2>/dev/null | awk '/Absolute upper-left X/{x=$4} /Absolute upper-left Y/{y=$4} /Width:/{w=$2} /Height:/{h=$2} END{print w "x" h "+" x "+" y}'
}
shot() { # shot NAME [main|bar]   -> prints the png path
  local p="$JDIR/$1.png" g=""
  import -window root "$p" 2>/dev/null
  [ -n "${2:-}" ] && g="$(geom_of "$2")" && [ -n "$g" ] && convert "$p" -crop "$g" +repage "$p" 2>/dev/null
  echo "$p"
}
brightness() { convert "$1" -colorspace Gray -format '%[fx:mean]' info: 2>/dev/null; }
colours() { convert "$1" -format '%k' info: 2>/dev/null; }
ocr() { # ocr PNG -> text (also written next to the picture)
  local p="$1" q="${1%.png}.ocr.png" neg=""
  python3 -c "import sys; sys.exit(0 if float('$(brightness "$p")') < 0.4 else 1)" 2>/dev/null && neg="-negate"
  convert "$p" -resize 200% -colorspace Gray $neg "$q" 2>/dev/null
  tesseract "$q" stdout --psm 11 2>/dev/null | tr -s '\n' ' ' | tee "${p%.png}.txt"
  rm -f "$q"
}
ocr_find() { # ocr_find PNG WORD -> "x y" (centre, in the picture's own pixels) or nothing
  local p="$1" q="${1%.png}.ocr.png" neg=""
  python3 -c "import sys; sys.exit(0 if float('$(brightness "$p")') < 0.4 else 1)" 2>/dev/null && neg="-negate"
  convert "$p" -resize 200% -colorspace Gray $neg "$q" 2>/dev/null
  tesseract "$q" stdout --psm 11 tsv 2>/dev/null | python3 -I -c "
import sys
w = sys.argv[1].lower()
for l in sys.stdin:
    f = l.rstrip('\n').split('\t')
    if len(f) >= 12 and f[11].lower().startswith(w):
        print(int((int(f[6]) + int(f[8]) / 2) / 2), int((int(f[7]) + int(f[9]) / 2) / 2)); break
" "$2"
  rm -f "$q"
}
wait_text() { # wait_text NAME main|bar REGEX SECONDS -> sets WT_OK (0|1), WT_TEXT, WT_PNG
  local t0 i=0 p
  t0=$(date +%s); WT_OK=0; WT_TEXT=""
  while :; do
    i=$((i + 1)); p="$(shot "$1-$i" "$2")"; WT_PNG="$p"; WT_TEXT="$(ocr "$p")"
    if echo "$WT_TEXT" | grep -Eiq "$3"; then WT_OK=1; return 0; fi
    [ "$i" -ge 2 ] && [ $(( $(date +%s) - t0 )) -ge "$4" ] && return 0
    sleep 0.7
  done
}
key() { xdotool key --clearmodifiers "$@"; sleep 0.15; }
type_text() { xdotool type --delay 70 -- "$1"; sleep 1.5; }
hotkey() { xdotool key --clearmodifiers ctrl+alt+space; }
focus_main() { local id; id="$(win_main)"; [ -n "$id" ] && { xdotool windowactivate --sync "$id" 2>/dev/null || xdotool windowfocus "$id" 2>/dev/null; }; sleep 0.4; return 0; }
one_line() { echo "$1" | tr -s '[:space:]' ' ' | cut -c1-400; }

launch_app() { # launch_app [args...]
  mkdir -p "$JDIR/data"
  PANOPLY_DATA_DIR="$JDIR/data" "$APPIMAGE" "$@" >"$JDIR/stdout.txt" 2>"$JDIR/stderr.txt" &
  APP_T0=$(date +%s.%N)
}
wait_main() { # wait_main SECONDS -> prints window id, or nothing
  local i w
  for i in $(seq 1 $(( $1 * 10 ))); do
    w="$(win_main)"; [ -n "$w" ] && { echo "$w"; return 0; }
    # The AppImage unpacks itself first, so "not running yet" is normal for the first seconds.
    [ "$i" -gt 100 ] && ! alive && return 1
    sleep 0.1
  done
  return 1
}
find_new() { # find_new DIR GLOB SECONDS -> prints the first matching file, ignoring partial files
  local t0 f; t0=$(date +%s)
  while :; do
    f="$(find "$1" -maxdepth 1 -type f -name "$2" ! -name '*.part*' ! -name '.*' 2>/dev/null | head -1)"
    [ -n "$f" ] && { sleep 0.4; echo "$f"; return 0; }
    [ $(( $(date +%s) - t0 )) -ge "$3" ] && return 1
    sleep 0.5
  done
}
fixtures() { mkdir -p "$1"; cp "$WORK/fixtures/"* "$1/"; }
filesize() { stat -c %s "$1"; }

"$PY" -I "$FIX" make "$WORK/fixtures" >"$OUT/fixtures.txt" 2>&1 && FX_OK=1 || FX_OK=0
cat "$OUT/fixtures.txt"
fc-list : family | sort -u >"$OUT/fonts.txt"
echo "tesseract: $(tesseract --version 2>&1 | head -1)" | tee "$OUT/ocr-engine.txt"

# ================================================================================================
j_1() { # First launch
  launch_app
  local t0 w="" win_ms="" paint_ms="" p stat k
  t0=$(date +%s%3N)
  local deadline=$(( $(date +%s) + 20 ))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    [ -z "$w" ] || alive || break
    if [ -z "$w" ]; then w="$(win_main)"; [ -n "$w" ] && win_ms=$(( $(date +%s%3N) - t0 )); fi
    if [ -n "$w" ]; then
      p="$(shot first-paint main)"; k="$(colours "$p")"
      if [ "${k:-0}" -gt 12 ]; then paint_ms=$(( $(date +%s%3N) - t0 )); break; fi
    fi
    sleep 0.1
  done
  data ms_to_window "${win_ms:-none}"; data ms_to_first_painted_frame "${paint_ms:-none}"
  check "the process is alive" "$(ok_if x 'alive')"
  check "a window titled Panoply appears" "$(ok_if x '[ -n "$w" ]')" "after ${win_ms:-?} ms"
  if [ -n "$w" ]; then
    data window "$(xdotool getwindowgeometry "$w" | tr '\n' ' ')"
    if [ -n "$paint_ms" ] && [ "$paint_ms" -le 3000 ]; then check "first painted frame within 3 s" 1 "$paint_ms ms"
    elif [ -n "$paint_ms" ] && [ "$paint_ms" -le 10000 ]; then warn "first painted frame took longer than 3 s" "$paint_ms ms (software rendering on a runner, so a script note and not a verdict)"
    else check "the window is not blank" 0 "unique colours: ${k:-?}"; fi
    sleep 2
    wait_text home main 'choose files|drop|search|Panoply' 8
    check "home renders (OCR finds its words)" "$WT_OK" "$(one_line "$WT_TEXT")"
    note "Accessibility (AT-SPI) tree not probed on Linux: no script for it yet"
  fi
  sleep 3
  check "still alive after 6 s" "$(ok_if x 'alive')"
  data stderr "$(head -c 600 "$JDIR/stderr.txt" | tr '\n' ' ')"
  stop_all
}

j_2() { # Hotkey, type compress, Enter
  launch_app --background
  sleep 4
  check "the process is alive in the background" "$(ok_if x 'alive')"
  check "no window before the hotkey" "$(ok_if x '! bar_mapped')"
  hotkey; sleep 2.5
  check "the command bar appears on Ctrl+Alt+Space" "$(ok_if x 'bar_mapped')"
  if bar_mapped; then
    data bar "$(bar_line | tr -s ' ')"
    data focus_after_hotkey "$(xdotool getwindowfocus 2>&1) bar=$(bar_id)"
    local bid; bid="$(bar_id)"; xdotool windowfocus "$bid" 2>/dev/null; sleep 0.3
    type_text "compress"
    wait_text bar-typed bar 'compress' 6
    data rows_after_typing "$(one_line "$WT_TEXT")"
    check "typed text and result rows are visible" "$WT_OK" "$(one_line "$WT_TEXT")"
    key Return; sleep 2
    wait_text bar-after-enter bar 'Choose files|Quality|Run|Options' 8
    data panel_text "$(one_line "$WT_TEXT")"
    local which; which="$(echo "$WT_TEXT" | grep -Eio 'Compress (image|PDF|video)' | head -1)"
    check "a Compress panel opens (its title and controls)" "$(ok_if x '[ "$WT_OK" = 1 ] && [ -n "$which" ]')" "$(one_line "$WT_TEXT")"
    data panel_title "${which:-none}"
    [ "$which" = "Compress image" ] || [ -z "$which" ] || note "the word compress matches three tools (image, PDF, video); Enter opened: $which"
    check "the bar window is still on screen" "$(ok_if x 'bar_mapped')"
    key Escape; sleep 1
  fi
  stop_all
}

j_3() { # Open a PNG by command line, "under 50 kb"
  [ "$FX_OK" = 1 ] || { limit "fixtures could not be made (python or pip failed)"; return; }
  local d="$WORK/j3"; fixtures "$d"
  local png="$d/testpic.png" h0 s0; h0="$(sha256sum "$png" | cut -d' ' -f1)"; s0="$(filesize "$png")"
  data input_bytes "$s0"
  launch_app "$png"
  local w; w="$(wait_main 25)"
  check "the app window opens with the file argument" "$(ok_if x '[ -n "$w" ]')"
  [ -n "$w" ] || { stop_all; return; }
  wait_text file-open main 'testpic' 10
  check "the file is shown (its name is on screen)" "$WT_OK" "$(one_line "$WT_TEXT")"
  focus_main
  key ctrl+f; sleep 0.4
  type_text "under 50 kb"
  wait_text query main 'under 50' 6
  data rows_after_typing "$(one_line "$WT_TEXT")"
  check "the words typed reach the search box" "$WT_OK"
  key Return
  local out="" presses=1 i
  for i in 1 2 3 4; do
    out="$(find_new "$d" 'testpic*(*' 5)" && break
    shot "wait-$i" main >/dev/null
    [ "$i" -lt 4 ] && { key Return; presses=$((presses + 1)); }
  done
  [ -n "$out" ] || out="$(find_new "$d" 'testpic*(*' 15)"
  data enter_presses "$presses"
  wait_text result-card main 'KB|MB|smaller|Undo|Done|saved' 8
  data result_card "$(one_line "$WT_TEXT")"
  check "an output file appeared next to the input" "$(ok_if x '[ -n "$out" ]')"
  if [ -n "$out" ]; then
    local sz; sz="$(filesize "$out")"
    data output "$(basename "$out")"; data output_bytes "$sz"
    check "the output is under 50 KB" "$(ok_if x '[ "$sz" -lt 51200 ]')" "$sz bytes"
    check "the output is named (compressed)" "$(ok_if x 'case "$(basename "$out")" in *"(compressed"*) true ;; *) false ;; esac')" "$(basename "$out")"
  fi
  check "the original is untouched" "$(ok_if x '[ "$(sha256sum "$png" | cut -d" " -f1)" = "$h0" ]')"
  SESS_OK=$([ -n "$out" ] && echo 1 || echo 0); SESS_FILE="$out"; SESS_DIR="$d"
}

j_5() { # Undo: trash, not deleted
  if [ "$SESS_OK" != 1 ]; then skip "journey 3 produced no output to undo"; stop_all; return; fi
  local out="$SESS_FILE" name; name="$(basename "$SESS_FILE")"
  data file "$name"
  focus_main
  key ctrl+z; sleep 3
  local how="Ctrl+Z"
  if [ -e "$out" ]; then
    local p xy; p="$(shot result-before-click main)"; xy="$(ocr_find "$p" Undo)"
    if [ -n "$xy" ]; then
      local g x0 y0; g="$(geom_of main)"; x0="${g#*+}"; x0="${x0%%+*}"; y0="${g##*+}"
      xdotool mousemove $(( x0 + ${xy% *} )) $(( y0 + ${xy#* } )) click 1; sleep 3
      how="clicked the Undo button (Ctrl+Z did nothing)"
      note "Ctrl+Z did not undo the result; the Undo button did, if the file is gone"
    else note "Ctrl+Z did nothing and OCR could not find an Undo button"; fi
  fi
  data undo_by "$how"
  shot after-undo main >/dev/null
  check "the output file is gone from the folder" "$(ok_if x '[ ! -e "$out" ]')"
  check "the original is still there" "$(ok_if x '[ -e "$SESS_DIR/testpic.png" ]')"
  local inTrash=0; [ -e "$TRASH/files/$name" ] && inTrash=1
  [ "$inTrash" = 1 ] || { ls "$TRASH/files" 2>/dev/null | grep -qF "${name%.*}" && inTrash=1; }
  data trash_listing "$(ls "$TRASH/files" 2>&1 | head -5 | tr '\n' ' ')"
  check "the freedesktop trash holds the output (it was not deleted for good)" "$inTrash" "$TRASH/files/$name"
  stop_all
}

merge_flow() { # sets nothing; used by 4a
  :
}
j_4a() {
  [ "$FX_OK" = 1 ] || { limit "fixtures could not be made"; return; }
  local d="$WORK/j4a"; fixtures "$d"
  launch_app "$d/a.pdf" "$d/b.pdf"
  local w; w="$(wait_main 25)"
  check "the app window opens with two files" "$(ok_if x '[ -n "$w" ]')"
  [ -n "$w" ] || { stop_all; return; }
  wait_text files main 'a\.pdf|b\.pdf|merge|PDF' 8
  focus_main; key ctrl+f; sleep 0.4
  type_text "merge pdf"
  wait_text query main 'merge' 6
  data rows_after_typing "$(one_line "$WT_TEXT")"
  key Return
  local out="" i
  for i in 1 2 3; do
    out="$(find_new "$d" '*(merged*.pdf' 6)" && break
    shot "wait-$i" main >/dev/null; [ "$i" -lt 3 ] && key Return
  done
  shot result main >/dev/null
  check "a merged PDF appeared next to the inputs" "$(ok_if x '[ -n "$out" ]')"
  if [ -n "$out" ]; then
    local n; n="$("$PY" -I "$FIX" pages "$out" 2>&1)"
    data output "$(basename "$out")"; data pages "$n"
    check "the merged PDF has 5 pages (2 + 3)" "$(ok_if x '[ "$n" = 5 ]')" "pages=$n"
  fi
  stop_all
}

j_4b() {
  [ "$FX_OK" = 1 ] || { limit "fixtures could not be made"; return; }
  local d="$WORK/j4b"; fixtures "$d"
  local scan="$d/scan.pdf" before; before="$("$PY" -I "$FIX" text "$scan" 2>&1)"
  check "the input scan has no text layer to begin with" "$(ok_if x '[ -z "$before" ]')" "$before"
  launch_app "$scan"
  local w; w="$(wait_main 25)"
  check "the app window opens with the scan" "$(ok_if x '[ -n "$w" ]')"
  [ -n "$w" ] || { stop_all; return; }
  wait_text files main 'scan' 8
  focus_main; key ctrl+f; sleep 0.4
  type_text "ocr"
  wait_text query main 'ocr|text|search' 6
  data rows_after_typing "$(one_line "$WT_TEXT")"
  key Return
  local out="" presses=1 i t0; t0=$(date +%s)
  # Return runs the options step, then OCR asks to download its models and the runtime, one consent sheet each: say yes (Return) while an options or consent sheet shows.
  for i in $(seq 1 14); do
    out="$(find_new "$d" 'scan*(*.pdf' 6)" && break
    wait_text "wait-$i" main 'Download the|Not now|Options' 1
    if [ "$WT_OK" = 1 ]; then key Return; presses=$((presses + 1)); fi
    [ $(( $(date +%s) - t0 )) -ge 200 ] && break
  done
  [ -n "$out" ] || out="$(find_new "$d" 'scan*(*.pdf' 60)"
  data enter_presses "$presses"
  shot result main >/dev/null
  check "a searchable PDF appeared next to the scan" "$(ok_if x '[ -n "$out" ]')"
  if [ -n "$out" ]; then
    local t words; t="$("$PY" -I "$FIX" text "$out" 2>&1)"
    data output "$(basename "$out")"; data extracted_text "$(one_line "$t")"
    words="$("$PY" -I "$FIX" words "$out" 'invoice,4812,total,350,thank,business' 2>&1)"; local rc=$?
    check "the text can be extracted (at least two of the words on the page)" "$(ok_if x '[ "$rc" = 0 ]')" "found: $words"
    command -v pdftotext >/dev/null && data pdftotext "$(pdftotext "$out" - 2>/dev/null | tr '\n' ' ' | cut -c1-200)"
  fi
  stop_all
}

j_3b() { # The same words on a grainy photo saved as a PNG
  [ "$FX_OK" = 1 ] || { limit "fixtures could not be made (python or pip failed)"; return; }
  local d="$WORK/j3b"; fixtures "$d"
  data input_bytes "$(filesize "$d/photo.png")"
  launch_app "$d/photo.png"
  local w; w="$(wait_main 25)"
  check "the app window opens with the file argument" "$(ok_if x '[ -n "$w" ]')"
  [ -n "$w" ] || { stop_all; return; }
  wait_text file-open main 'photo' 10
  focus_main; key ctrl+f; sleep 0.4
  type_text "under 50 kb"
  wait_text query main 'under 50' 8
  data rows_after_typing "$(one_line "$WT_TEXT")"
  key Return
  local out="" i
  for i in 1 2 3 4; do
    out="$(find_new "$d" 'photo*(*' 6)" && break
    [ "$i" -lt 4 ] && key Return   # the options step, if the tool waited for Return
  done
  wait_text result-card main 'KB|MB|smaller|Undo|Done|saved|did not work' 8
  data result_card "$(one_line "$WT_TEXT")"
  check "an output file appeared next to the input" "$(ok_if x '[ -n "$out" ]')" "$(one_line "$WT_TEXT")"
  if [ -n "$out" ]; then
    local sz; sz="$(filesize "$out")"; data output "$(basename "$out")"; data output_bytes "$sz"
    check "the output is under 50 KB" "$(ok_if x '[ "$sz" -lt 51200 ]')" "$sz bytes"
  fi
  stop_all
}

j_6() { # Close the window; Quit
  launch_app
  local w; w="$(wait_main 25)"
  check "the window opens" "$(ok_if x '[ -n "$w" ]')"
  [ -n "$w" ] || { stop_all; return; }
  sleep 2
  wmctrl -i -c "$(printf '0x%x' "$w")"
  sleep 4
  local after=0; alive && after=1
  data process_after_window_close "$([ "$after" = 1 ] && echo 'still running' || echo 'exited')"
  data window_after_close "$([ -n "$(win_main)" ] && echo yes || echo no)"
  shot closed >/dev/null
  if [ "$after" = 1 ]; then
    note "Linux has no notification-area icon: after the window closes the process stays (it keeps the hotkey) and the person has no visible way to quit it"
    hotkey; sleep 2
    check "the hotkey still opens the bar after the window closed" "$(ok_if x 'bar_mapped')"
    shot bar-after-close >/dev/null
    key Escape; sleep 1
    warn "the app keeps running with no window and no tray icon on Linux" "(see KNOWN-LIMITATIONS: no quit path is visible)"
  else
    check "closing the window ends the app" 1
  fi
  stop_all
  # Ctrl+Q with the window open
  launch_app
  w="$(wait_main 25)"
  if [ -n "$w" ]; then
    sleep 2; focus_main; key ctrl+q
    local gone=0 i; for i in $(seq 1 20); do alive || { gone=1; break; }; sleep 0.5; done
    check "Ctrl+Q quits the app" "$gone"
    [ "$gone" = 1 ] || shot ctrl-q main >/dev/null
  else check "the window opens a second time" 0; fi
  stop_all
}

j_7() { # Themes and scaling
  local theme
  for theme in light dark; do
    PANOPLY_APPEARANCE="$theme" launch_app
    local w; w="$(wait_main 25)"
    check "the window opens in $theme mode" "$(ok_if x '[ -n "$w" ]')"
    if [ -n "$w" ]; then
      sleep 3
      local p b ok t; p="$(shot "theme-$theme" main)"; b="$(brightness "$p")"
      data "brightness_$theme" "$b"
      if [ "$theme" = light ]; then ok="$(python3 -c "print(1 if $b > 0.6 else 0)")"; else ok="$(python3 -c "print(1 if $b < 0.35 else 0)")"; fi
      check "the window follows the $theme theme (PANOPLY_APPEARANCE=$theme)" "$ok" "mean brightness $b"
      t="$(ocr "$p")"; check "text is readable in $theme mode (OCR finds words)" "$(ok_if x '[ ${#t} -gt 12 ]')" "$(one_line "$t")"
    fi
    stop_all
  done
  note "Linux theme is forced with PANOPLY_APPEARANCE: the runner has no desktop setting for the app to follow, so following the system theme is not tested"
  # Scale: Xft.dpi, which GTK and Qt apps (and winit/GPUI on X11) read.
  local base_w="" dpi
  for dpi in 96 120 144; do
    echo "Xft.dpi: $dpi" | xrdb -merge 2>/dev/null
    launch_app
    local w; w="$(wait_main 25)"
    if [ -n "$w" ]; then
      sleep 3
      local g p t; g="$(geom_of main)"; p="$(shot "scale-$dpi" main)"; t="$(ocr "$p")"
      local ww="${g%%x*}"; [ "$dpi" = 96 ] && base_w="$ww"
      data "window_at_xft_dpi_$dpi" "$g"
      check "the window opens at Xft.dpi $dpi" 1 "$g"
      check "text is readable at Xft.dpi $dpi" "$(ok_if x '[ ${#t} -gt 12 ]')" "$(one_line "$t")"
      if [ "$dpi" != 96 ] && [ -n "$base_w" ]; then
        note "Xft.dpi $dpi: window $ww px wide against $base_w px at 96 (ratio $(python3 -c "print(round($ww/$base_w,2))"), a scaling app gives about $(python3 -c "print($dpi/96)"))"
      fi
    else check "the window opens at Xft.dpi $dpi" 0; fi
    stop_all
  done
  echo "Xft.dpi: 96" | xrdb -merge 2>/dev/null
}

j_8() { # Idle memory and CPU
  launch_app
  local w; w="$(wait_main 25)"
  check "the window opens" "$(ok_if x '[ -n "$w" ]')"
  [ -n "$w" ] || { stop_all; return; }
  local pid; pid="$(pid_of)"
  ticks() { awk '{print $14+$15}' "/proc/$pid/stat" 2>/dev/null; }
  sleep 30
  local c30 t30 c60 t60; c30="$(ticks)"; t30=$(date +%s.%N)
  sleep 30
  c60="$(ticks)"; t60=$(date +%s.%N)
  local hz; hz="$(getconf CLK_TCK)"
  local pct rss threads
  pct="$(python3 -c "print(round((${c60:-0}-${c30:-0})/$hz/($t60-$t30)*100,2))")"
  rss="$(awk '/VmRSS/{printf "%.1f", $2/1024}' "/proc/$pid/status")"
  threads="$(awk '/Threads/{print $2}' "/proc/$pid/status")"
  data idle_cpu_percent_of_one_core_30_to_60s "$pct"; data rss_mb "$rss"; data threads "$threads"
  shot idle main >/dev/null
  if python3 -c "import sys; sys.exit(0 if $pct <= 1.0 else 1)"; then check "idle CPU is at most 1% of a core between 30 s and 60 s" 1 "$pct%"
  elif python3 -c "import sys; sys.exit(0 if $pct <= 5.0 else 1)"; then warn "idle CPU is above 1% of a core" "$pct% (the design target is 0%)"
  else check "idle CPU is at most 5% of a core" 0 "$pct%"; fi
  if python3 -c "import sys; sys.exit(0 if $rss <= 100 else 1)"; then check "resident memory is at most 100 MB (the idle budget)" 1 "$rss MB"
  elif python3 -c "import sys; sys.exit(0 if $rss <= 250 else 1)"; then warn "resident memory is above the 100 MB idle budget" "$rss MB (software Vulkan on a runner; UI-AUDIT row 65 says compare on a GPU machine)"
  else check "resident memory is at most 250 MB" 0 "$rss MB"; fi
  stop_all
}

run_journey 1 'First launch: window, home, no crash or blank window within 3 s' j_1
run_journey 2 'Hotkey, type "compress", Enter: a Compress panel opens' j_2
run_journey 3 'Open a PNG by command line, run "under 50 kb": an output under 50 KB named (compressed)' j_3
run_journey 5 'Undo on a result: the file goes to the trash, not deleted' j_5
run_journey 4a 'Merge two PDFs: one output with 5 pages' j_4a
run_journey 4b 'OCR a scanned PDF: the text becomes extractable' j_4b
run_journey 3b 'A photo saved as PNG, "under 50 kb": an output under 50 KB, in whatever format works' j_3b
run_journey 6 'Close the window, Quit (Ctrl+Q); no tray icon on Linux' j_6
run_journey 7 'Light and dark theme, 125% and 150% scaling (Xft.dpi 120 and 144)' j_7
run_journey 8 'Memory and idle CPU after 60 s idle' j_8
stop_all

python3 -I - "$OUT" <<'PY'
import json, subprocess, sys
out = sys.argv[1]
rows = [json.loads(l) for l in open(out + "/results.jsonl") if l.strip()]
rep = {"os": subprocess.run(["lsb_release", "-ds"], capture_output=True, text=True).stdout.strip(),
       "screen": "1920x1080", "ocr_engine": "tesseract", "appimage": "AppImage", "journeys": rows}
open(out + "/journeys.json", "w").write(json.dumps(rep, indent=2))
md = ["### Linux user journeys", "", "| # | Journey | Result | Seconds |", "|---|---|---|---:|"]
for r in rows:
    md.append(f"| {r['id']} | {r['name']} | {r['status']} | {r['seconds']} |")
for r in rows:
    bad = [c for c in r["checks"] if not c["ok"]]
    if bad or r["notes"]:
        md += ["", f"**{r['id']}**"]
        md += [f"- FAILED: {c['check']} {c['detail']}" for c in bad]
        md += [f"- {n}" for n in r["notes"]]
text = "\n".join(md)
print(text)
import os
if os.environ.get("GITHUB_STEP_SUMMARY"):
    open(os.environ["GITHUB_STEP_SUMMARY"], "a").write(text + "\n")
PY
# Failing journeys are findings, not a broken workflow.
exit 0
