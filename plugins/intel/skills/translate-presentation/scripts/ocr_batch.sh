#!/bin/bash
# ABOUTME: Translates every picture-only deck that has an OCR box file and no output yet: ocr_slides.py extract,
# translate_slides.mjs, ocr_slides.py render, then copy to <out root>/<course>/<name>.pdf. Box files are
# <boxes root>/<course>/<name>.json; decks are <source root>/<course>/<name>.pdf. At most two decks run at once.
# Usage: ocr_batch.sh <boxes root> <source root> <work root> <out root> [jobs ≤ 2]
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
BOXES="$1"; SRC="$2"; WORK="$3"; OUT="$4"; JOBS="${5:-2}"
[ "$JOBS" -gt 2 ] && JOBS=2
LOGS="$WORK/logs"
mkdir -p "$LOGS"

one() {
  local boxes="$1" course name src w log status
  course="$(basename "$(dirname "$boxes")")"; name="$(basename "$boxes" .json)"
  src="$SRC/$course/$name.pdf"; w="$WORK/$course/$name"; log="$LOGS/$course--$name.log"
  [ -f "$OUT/$course/$name.pdf" ] && { echo "SKIP  $course/$name (exists)"; return; }
  [ -f "$src" ] || { echo "FAILED no source $course/$name"; return; }
  mkdir -p "$w"
  [ -f "$w/blocks.json" ] || "$HERE/ocr_slides.py" extract "$src" "$boxes" "$w" >"$log" 2>&1 || { echo "FAILED extract $course/$name"; return; }
  status=OK
  "$HERE/translate_slides.mjs" "$w" --title "MIT $course, $name" >>"$log" 2>&1 || status=PARTIAL
  [ -f "$w/zh.json" ] || { echo "FAILED translate $course/$name"; return; }
  "$HERE/ocr_slides.py" render "$src" "$w" "$w/out.pdf" >>"$log" 2>&1 || { echo "FAILED render $course/$name"; return; }
  mkdir -p "$OUT/$course" && cp "$w/out.pdf" "$OUT/$course/$name.pdf"
  echo "$status $course/$name"
}
export -f one
export HERE SRC WORK OUT LOGS

find "$BOXES" -name '*.json' -print0 | xargs -0 -P "$JOBS" -I{} bash -c 'one "$@"' _ {}
