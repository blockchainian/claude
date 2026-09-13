#!/usr/bin/env bash
# ABOUTME: Idempotent auto-setup for digest: installs only what's missing.
# ABOUTME: uv (runs the trafilatura article extractor) + yt-dlp (YouTube subs).

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

# uv runs trafilatura in an ephemeral env (article main-content extraction);
# yt-dlp pulls YouTube subtitles.
note_or_install uv
note_or_install yt-dlp

echo "digest setup: present=[${present[*]:-}] missing=[${missing[*]:-}] installed=[${installed[*]:-}]"
# trafilatura is not installed globally: fetch_source.py runs it via `uv run
# --with trafilatura`, so uv is the only thing needed and it caches the package.
