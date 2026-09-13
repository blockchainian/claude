#!/usr/bin/env python3
# ABOUTME: Downloads an audio file or URL and transcribes it locally with mlx-whisper.
# ABOUTME: Reusable audio-to-text step; any skill that has audio and needs its words.

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import urlparse

MODEL = "mlx-community/whisper-large-v3-turbo"
UA = ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/126 Safari/537.36")


def whisper_prefix():
    """The command that runs mlx_whisper, installed or via uv's ephemeral env."""
    if shutil.which("mlx_whisper"):
        return ["mlx_whisper"]
    if shutil.which("uv"):
        return ["uv", "run", "--with", "mlx-whisper", "mlx_whisper"]
    raise SystemExit("mlx-whisper not available: install uv, or "
                     "`pip install mlx-whisper` (Apple Silicon only)")


def build_cmd(prefix, audio, out_dir):
    return prefix + [str(audio), "--model", MODEL, "--output-dir", str(out_dir),
                     "--output-format", "txt", "--verbose", "False"]


def get_audio(src, work):
    """A local file is used in place; anything else is downloaded."""
    local = Path(src)
    if local.is_file():
        return local
    suffix = Path(urlparse(src).path).suffix or ".audio"
    dest = work / f"download{suffix}"
    res = subprocess.run(["curl", "-sL", "--max-time", "600", "-A", UA,
                          "-o", str(dest), src], capture_output=True, text=True)
    if res.returncode != 0 or not dest.exists() or dest.stat().st_size == 0:
        raise SystemExit(f"download failed for {src}: {res.stderr.strip()[-300:]}")
    return dest


def main():
    if len(sys.argv) < 3:
        raise SystemExit("usage: transcribe_audio.py <audio-url-or-file> <out.txt>")
    src, out_txt = sys.argv[1], Path(sys.argv[2])
    work = Path(tempfile.mkdtemp())
    audio = get_audio(src, work)
    # Keep our stdout clean for the JSON; whisper's chatter goes to stderr.
    subprocess.run(build_cmd(whisper_prefix(), audio, work), check=True,
                   stdout=sys.stderr)
    produced = sorted(work.glob("*.txt"))
    if not produced:
        raise SystemExit("transcription produced no .txt output")
    text = produced[0].read_text(encoding="utf-8", errors="replace")
    out_txt.parent.mkdir(parents=True, exist_ok=True)
    out_txt.write_text(text, encoding="utf-8")
    words = len(text.split())
    print(json.dumps({"transcript": str(out_txt), "words": words,
                      "thin": words < 1500, "source": src}, indent=2))


if __name__ == "__main__":
    main()
