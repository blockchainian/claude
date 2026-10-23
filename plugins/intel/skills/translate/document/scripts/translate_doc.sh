#!/bin/bash
# ABOUTME: Translates one document built from a Markdown file or a web page into a Chinese PDF in the translate
# ABOUTME: skill's book look: split into segments, translate through codex, join back, print with headless Chrome.
#
# Usage: translate_doc.sh (--md FILE | --txt FILE | --url URL [--html FILE] [--reader]) --out PDF [--work DIR]
#                         [--title T] [--glossary FILE] [--context C] [--drop-heading TEXT ...] [--skip-bookmark TEXT ...]
# --txt takes a plain-text transcript (one sentence per line) and regroups it into paragraphs first.
# --html uses a saved copy of the page instead of fetching --url (which still resolves its relative links).
# --reader keeps only the article of a page from a site whose pages wrap it in chrome (tags, share bars, footers).
# --work defaults to $INTEL_STATE_DIR/translate-document/<out name>; its zh.json caches every translated segment, so a
# rerun only re-renders and redoes what failed.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
[ -f "$HOME/.config/intel/.env" ] && { set -a; . "$HOME/.config/intel/.env"; set +a; }

md="" txt="" url="" html="" reader=0 out="" work="" title="" glossary="" context="technical course material"
drop=() skip=()
while [ $# -gt 0 ]; do
  case "$1" in
    --md) md="$2"; shift 2 ;;
    --txt) txt="$2"; shift 2 ;;
    --url) url="$2"; shift 2 ;;
    --html) html="$2"; shift 2 ;;
    --reader) reader=1; shift ;;
    --out) out="$2"; shift 2 ;;
    --work) work="$2"; shift 2 ;;
    --title) title="$2"; shift 2 ;;
    --glossary) glossary="$2"; shift 2 ;;
    --context) context="$2"; shift 2 ;;
    --drop-heading) drop+=(--drop-heading "$2"); shift 2 ;;
    --skip-bookmark) skip+=(--skip-bookmark "$2"); shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$out" ] && { [ -n "$md" ] || [ -n "$txt" ] || [ -n "$url" ] || [ -n "$html" ]; } || {
  echo "usage: translate_doc.sh (--md FILE | --txt FILE | --url URL [--html FILE] [--reader]) --out PDF [options]" >&2; exit 2; }
name=$(basename "$out" .pdf)
work="${work:-${INTEL_STATE_DIR:-$HOME/.local/state/intel}/translate-document/$name}"
mkdir -p "$work/src" "$(dirname "$out")"
title="${title:-$name}"
opts=(--title "$title" --context "$context")
[ -n "$glossary" ] && opts+=(--glossary "$glossary")
if [ -n "$txt" ]; then
  md="$work/src/transcript.md"
  "$HERE/txt2md.py" "$txt" "$md"
fi

if [ -n "$md" ]; then
  "$HERE/md_segments.py" split "$md" "$work/segments.json"
  node "$HERE/translate_segments.mjs" "$work/segments.json" "$work/zh.json" --format md "${opts[@]}" || true
  "$HERE/md_segments.py" join "$md" "$work/zh.json" "$work/$name.md"
  "$HERE/md2pdf.py" "$work/$name.md" "$work/$name.pdf"
else
  page="$work/src/page.html"
  if [ -n "$html" ]; then
    cp "$html" "$page"
  elif [ ! -s "$page" ]; then
    code=$(curl -sL --retry 2 --max-time 60 -A "Mozilla/5.0" -o "$page" -w '%{http_code}' "$url" || true)
    if [ "$code" = 429 ] && [ -n "${ISP_PROXY_URL:-}" ]; then
      code=$(curl -sL --retry 2 --max-time 90 -A "Mozilla/5.0" -x "$ISP_PROXY_URL" -o "$page" -w '%{http_code}' "$url" || true)
    fi
    [ "$code" = 200 ] || { echo "FETCHFAIL($code) $url"; rm -f "$page"; exit 1; }
  fi
  if [ "$reader" = 1 ]; then
    node "$HERE/readable.mjs" "$page" "${url:-about:blank}" "$work/article.html"; page="$work/article.html"
  fi
  "$HERE/html_segments.py" split "$page" "$work/segments.json" "$work/skeleton.html" ${drop[@]+"${drop[@]}"}
  node "$HERE/translate_segments.mjs" "$work/segments.json" "$work/zh.json" --format html "${opts[@]}" || true
  "$HERE/html_segments.py" join "$work/skeleton.html" "$work/zh.json" "$work/$name.html"
  base=(); [ -n "$url" ] && base=(--base "$url")
  # The print waits for the page's load event, which a slow server can hold past html2pdf's deadline; a second try
  # nearly always settles.
  "$HERE/html2pdf.py" "$work/$name.html" ${base[@]+"${base[@]}"} ${skip[@]+"${skip[@]}"} ||
    "$HERE/html2pdf.py" "$work/$name.html" ${base[@]+"${base[@]}"} ${skip[@]+"${skip[@]}"}
fi
cp "$work/$name.pdf" "$out"
echo "DONE $out"
