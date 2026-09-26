#!/usr/bin/env bash
# ABOUTME: Idempotent auto-setup for translate: installs only what's missing.
# ABOUTME: poppler (pdftotext/pdftoppm), uv (runs the pikepdf scripts); reports codex and Chrome.

set -euo pipefail

CHECK=0
[ "${1:-}" = "--check" ] && CHECK=1

have() { command -v "$1" >/dev/null 2>&1; }

present=()
missing=()
installed=()

BREW=1
have brew || BREW=0

note_or_install() {
  local tool="$1" formula="${2:-$1}"
  if have "$tool"; then
    present+=("$tool")
    return
  fi
  missing+=("$tool")
  if [ "$CHECK" -eq 1 ]; then
    return
  fi
  if [ "$BREW" -eq 0 ]; then
    echo "cannot install $tool: Homebrew missing (https://brew.sh)" >&2
    return
  fi
  echo "installing $tool ..." >&2
  brew install "$formula" >/dev/null && installed+=("$tool")
}

# pdftotext/pdftoppm split and sample the source PDF; uv runs the scripts with pikepdf, markdown and pillow.
note_or_install pdftotext poppler
note_or_install uv

# codex signs in with the user's ChatGPT plan (gpt-6-luna); Chrome typesets the PDF. Neither is brew-installed here.
if have codex && [ -f "${CODEX_HOME:-$HOME/.codex}/auth.json" ]; then
  present+=("codex")
else
  missing+=("codex (npm i -g @openai/codex, then codex login)")
fi
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
if [ -x "$CHROME" ] || have google-chrome || have chromium; then
  present+=("chrome")
else
  missing+=("chrome (set CHROME=/path/to/chrome)")
fi

echo "present: ${present[*]:-none}"
[ ${#installed[@]} -gt 0 ] && echo "installed: ${installed[*]}"
if [ ${#missing[@]} -gt 0 ]; then
  echo "missing: ${missing[*]}"
  [ "$CHECK" -eq 1 ] && exit 1
fi
exit 0
