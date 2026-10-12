#!/bin/bash
# ABOUTME: Idempotent auto-setup for translate-scan: the translate-epub skill's setup (poppler, uv, codex, Chrome), plus
# ABOUTME: ImageMagick for page crops and the Swift compiler that builds the macOS Vision helper.

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
[ -d "$HERE/../../paper/scripts" ] || missing+=("the translate-paper skill next to this one")

if [ ${#missing[@]} -gt 0 ]; then
  echo "missing: ${missing[*]}"
  exit 1
fi
echo "translate-scan: ready"
