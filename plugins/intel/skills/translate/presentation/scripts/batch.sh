#!/bin/bash
# ABOUTME: Translates every deck in a list file (TSV, path in the last column) that has no output yet: extract,
# translate, render, then copy to <out root>/<course>/<name>.pdf. At most two decks run at once, which keeps the
# machine's memory safe; one status line per deck.
# Usage: batch.sh <list.tsv> <work root> <out root> [jobs ≤ 2]   (skips *-all-animations.pdf)
#        REDO=1 batch.sh ...   re-runs listed decks that already have an output (a resumed translation fills only
#                              the texts still missing, then the deck is rendered again)
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
LIST="$1"; WORK="$2"; OUT="$3"; JOBS="${4:-2}"
[ "$JOBS" -gt 2 ] && JOBS=2
LOGS="$WORK/logs"
mkdir -p "$LOGS"

one() {
  local src="$1" course name w log status
  course="$(basename "$(dirname "$src")")"; name="$(basename "$src" .pdf)"
  w="$WORK/$course/$name"; log="$LOGS/$course--$name.log"
  [ -z "${REDO:-}" ] && [ -f "$OUT/$course/$name.pdf" ] && { echo "SKIP  $course/$name (exists)"; return; }
  mkdir -p "$w"
  [ -f "$w/blocks.json" ] || "$HERE/slides.py" extract "$src" "$w" >"$log" 2>&1 || { echo "FAILED extract $course/$name"; return; }
  status=OK
  "$HERE/translate_slides.mjs" "$w" --title "MIT $course, $name" >>"$log" 2>&1 || status=PARTIAL
  [ -f "$w/zh.json" ] || { echo "FAILED translate $course/$name"; return; }
  "$HERE/slides.py" render "$src" "$w" "$w/out.pdf" >>"$log" 2>&1 || { echo "FAILED render $course/$name"; return; }
  mkdir -p "$OUT/$course" && cp "$w/out.pdf" "$OUT/$course/$name.pdf"
  echo "$status $course/$name ($(grep -c 'still low contrast' "$log") pages low contrast)"
}
export -f one
export HERE WORK OUT LOGS REDO

awk -F'\t' '{print $NF}' "$LIST" | grep -v -- '-all-animations\.pdf$' | tr '\n' '\0' |
  xargs -0 -P "$JOBS" -I{} bash -c 'one "$@"' _ {}
