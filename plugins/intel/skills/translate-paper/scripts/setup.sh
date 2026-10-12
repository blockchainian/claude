#!/bin/bash
# ABOUTME: Idempotent setup for translate-paper: MinerU in its own venv, KaTeX for the formula check, and the
# ABOUTME: translate skill's own setup (poppler, uv, codex login, Chrome). Pass --check to report without installing.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK=0
[ "${1:-}" = "--check" ] && CHECK=1
VENV="${MINERU_VENV:-$HOME/.local/share/intel/translate-paper/mineru}"
missing=()

[ -d "$HERE/../../translate/scripts" ] || missing+=("the translate skill next to this one")
if [ ! -x "$VENV/bin/mineru-kit" ]; then
  if [ "$CHECK" -eq 1 ]; then missing+=("MinerU ($VENV)"); else
    echo "installing MinerU into $VENV ..." >&2
    mkdir -p "$(dirname "$VENV")"
    uv venv -q --python 3.12 "$VENV"
    VIRTUAL_ENV="$VENV" uv pip install -q "mineru[all]"
  fi
fi
if [ ! -d "$HERE/node_modules/katex" ]; then
  if [ "$CHECK" -eq 1 ]; then missing+=("katex (npm install in $HERE)"); else (cd "$HERE" && npm install --silent); fi
fi
command -v pdfinfo >/dev/null || missing+=("pdfinfo (brew install poppler)")
if [ "$CHECK" -eq 1 ]; then "$HERE/../../translate/scripts/setup.sh" --check; else "$HERE/../../translate/scripts/setup.sh"; fi
if [ ${#missing[@]} -gt 0 ]; then echo "missing: ${missing[*]}"; exit 1; fi
echo "translate-paper ready: MinerU $("$VENV/bin/mineru-kit" version 2>/dev/null | head -1)"
