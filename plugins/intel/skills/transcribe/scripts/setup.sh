#!/usr/bin/env bash
# ABOUTME: Idempotent auto-setup for transcribe: installs only what's missing.
# ABOUTME: ffmpeg + streamlink + yt-dlp + a whisper runner; --check only reports.

set -euo pipefail

CHECK=0
[ "${1:-}" = "--check" ] && CHECK=1

have() { command -v "$1" >/dev/null 2>&1; }

present=()
missing=()
installed=()

# Homebrew is the install vehicle on macOS.
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

# ffmpeg pulls and segments the audio; streamlink/yt-dlp resolve platform lives.
note_or_install ffmpeg
note_or_install streamlink
note_or_install yt-dlp

# Whisper runner: mlx_whisper on PATH, or uv to run it in an ephemeral env.
if have mlx_whisper || have uv; then
  present+=("whisper-runner")
else
  missing+=("whisper-runner")
  if [ "$CHECK" -eq 0 ]; then
    if [ "$BREW" -eq 1 ]; then
      echo "installing uv (runs mlx-whisper) ..." >&2
      brew install uv >/dev/null && installed+=("uv")
    else
      echo "cannot install a whisper runner: Homebrew missing" >&2
    fi
  fi
fi

echo "transcribe setup: present=[${present[*]:-}] missing=[${missing[*]:-}] installed=[${installed[*]:-}]"
# The whisper model (whisper-large-v3-turbo, ~1.5GB) is not fetched here: mlx-whisper
# downloads it on the first transcription and caches it, so it self-installs once.
echo "note: the whisper model downloads on first transcription, then is cached." >&2
