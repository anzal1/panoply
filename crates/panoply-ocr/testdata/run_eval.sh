#!/bin/sh
# Runs every engine on every labelled item, one process per run so that peak memory
# (maximum resident set size of the whole process, models and runtime included) is per run.
# Writes JSON lines to the file given as $1. Needs a release build of the ocr_eval example:
#   cargo build --release -p panoply-ocr --example ocr_eval
# Corpus items need `python3 tests/corpus/fetch.py` first (they are in tests/corpus/cache).
set -u
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../.."
bin="$root/target/release/examples/ocr_eval"
out="$1"
: > "$out"
L="$here/labelled"
run() { # engine langs label input
  /usr/bin/time -l "$bin" --engine "$1" --langs "$2" --label "$3" "$4" 2>"$out.err" | tail -1 > "$out.one"
  rss=$(awk '/maximum resident set size/ {print $1}' "$out.err")
  python3 - "$out.one" "$rss" "$1" >> "$out" <<'PY'
import json, sys
try:
    d = json.loads(open(sys.argv[1]).read().strip().splitlines()[-1])
except Exception:
    d = {"error": "no output"}
d["engine_asked"] = sys.argv[3]
d["peak_rss_mb"] = round(int(sys.argv[2] or 0) / 1e6, 1)
print(json.dumps(d, ensure_ascii=False))
PY
}
for img in "$L"/*.png; do
  id="$(basename "$img" .png)"
  case "$id" in
    en-*) langs=en; engines="vision ppocr tesseract-plain" ;;
    hien-*) langs=hi,en; engines="vision ppocr" ;;
    hi-*) langs=hi; engines="vision ppocr tesseract-plain" ;;
    bn-*) langs=bn; engines="vision tesseract-plain tesseract" ;;
    *) continue ;;
  esac
  for e in $engines; do
    l="$langs"
    # Vision is measured the way it is used when it cannot be told the language: automatic.
    [ "$e" = vision ] && l=""
    run "$e" "$l" "$L/$id.txt" "$img"
  done
done
C="$root/tests/corpus/cache/pdf"
for item in "commons/ia-scan-public-lands.pdf#15:corpus-ia-lands-p15" "commons/ia-scan-mining-claims.pdf#9:corpus-ia-claims-p9" "pdfjs/scan-bad.pdf#1:corpus-scanbad-p1"; do
  f="${item%%:*}"; l="${item##*:}"
  for e in vision ppocr tesseract-plain; do
    run "$e" en "$L/$l.txt" "$C/$f"
  done
done
rm -f "$out.err" "$out.one"
