#!/bin/bash
# ABOUTME: Idempotent auto-setup for translate-pdf: runs the translate skill's setup (poppler, uv, codex, Chrome),
# ABOUTME: then installs readable.mjs's Node packages (Readability, jsdom) beside it when missing.

set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

"$HERE/../../translate/scripts/setup.sh" "$@"

if [ -d "$HERE/node_modules/@mozilla/readability" ] && [ -d "$HERE/node_modules/jsdom" ]; then
  echo "present: readability, jsdom"
elif [ "${1:-}" = "--check" ]; then
  echo "missing: readability, jsdom (run setup.sh)"; exit 1
else
  command -v npm >/dev/null 2>&1 || { echo "missing: npm (install Node.js)"; exit 1; }
  npm install --silent --prefix "$HERE" >/dev/null && echo "installed: readability, jsdom"
fi
