#!/bin/bash
# ABOUTME: Idempotent auto-setup for translate-scan: the translate-epub skill's setup (poppler, uv, codex, Chrome), plus
# ABOUTME: ImageMagick for page crops, the Swift compiler that builds the macOS Vision helper and translate-paper's KaTeX.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
"$HERE/../../epub/scripts/setup.sh" "$@"

missing=()
if ! command -v magick >/dev/null 2>&1; then
  if [ "${1:-}" = "--check" ] || ! command -v brew >/dev/null 2>&1; then
    missing+=("magick (brew install imagemagick)")
  else
    echo "installing imagemagick ..." >&2
    brew install imagemagick >/dev/null
  fi
fi
command -v swiftc >/dev/null 2>&1 || missing+=("swiftc (xcode-select --install)")
PAPER="$HERE/../../paper/scripts"
if [ ! -d "$PAPER" ]; then
  missing+=("the translate-paper skill next to this one")
elif [ ! -d "$PAPER/node_modules/katex" ]; then
  # translate-paper's translator checks every formula with KaTeX.
  if [ "${1:-}" = "--check" ]; then missing+=("katex (npm install in $PAPER)"); else (cd "$PAPER" && npm install --silent); fi
fi

if [ ${#missing[@]} -gt 0 ]; then
  echo "missing: ${missing[*]}"
  exit 1
fi
echo "translate-scan: ready"
