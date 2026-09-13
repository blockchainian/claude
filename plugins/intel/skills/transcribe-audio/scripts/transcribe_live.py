#!/usr/bin/env python3
# ABOUTME: Transcribes a live audio stream incrementally with whisper.
# ABOUTME: ffmpeg segments the stream; each finished chunk is transcribed and appended.

import argparse
import json
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import urlparse

import transcribe_audio as base

SEG_RE = re.compile(r"seg(\d+)\.wav$")


def resolver_cmd(url):
    """The command that prints a playable URL for a platform live, or None if
    the URL is already a direct, ffmpeg-readable stream."""
    host = urlparse(url).netloc.lower()
    if "twitch.tv" in host:
        return ["streamlink", "--stream-url", url, "best"]
    if "youtube.com" in host or "youtu.be" in host:
        return ["yt-dlp", "-g", url]
    if "x.com" in host or "twitter.com" in host:
        return ["yt-dlp", "-g", url]
    return None


def resolve_stream(url):
    cmd = resolver_cmd(url)
    if cmd is None:
        return url
    tool = cmd[0]
    if shutil.which(tool) is None:
        raise SystemExit(f"{tool} is needed to resolve {url} — run setup.sh")
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0 or not res.stdout.strip():
        raise SystemExit(f"could not resolve a stream from {url}: "
                         f"{res.stderr.strip()[-300:]}")
    return res.stdout.strip().splitlines()[0]


def segment_cmd(src, seg_dir, seconds):
    """ffmpeg reading the stream and writing fixed-length 16k mono wav chunks."""
    return ["ffmpeg", "-nostdin", "-loglevel", "error", "-i", str(src),
            "-ac", "1", "-ar", "16000", "-f", "segment",
            "-segment_time", str(seconds), "-reset_timestamps", "1",
            str(Path(seg_dir) / "seg%05d.wav")]


def completed_indices(indices, running):
    """Which segments are safe to transcribe. While ffmpeg runs, the highest
    index is still being written, so only the ones below it are done; once the
    stream has ended every segment is complete."""
    idxs = sorted(indices)
    if not idxs:
        return []
    return idxs[:-1] if running else idxs


def segment_indices(seg_dir):
    out = []
    for p in Path(seg_dir).glob("seg*.wav"):
        m = SEG_RE.search(p.name)
        if m:
            out.append(int(m.group(1)))
    return out


def transcribe_segment(prefix, wav, work):
    out_dir = Path(work) / ("out_" + wav.stem)
    out_dir.mkdir(exist_ok=True)
    subprocess.run(base.build_cmd(prefix, wav, out_dir), check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    produced = sorted(out_dir.glob("*.txt"))
    return produced[0].read_text(encoding="utf-8", errors="replace") if produced else ""


def run_live(src, out_txt, seconds=30, max_minutes=None, on_text=None):
    if shutil.which("ffmpeg") is None:
        raise SystemExit("ffmpeg not found — run setup.sh")
    prefix = base.whisper_prefix()
    stream = resolve_stream(src)

    work = Path(tempfile.mkdtemp())
    seg_dir = work / "seg"
    seg_dir.mkdir()
    out_txt = Path(out_txt)
    out_txt.parent.mkdir(parents=True, exist_ok=True)
    out_txt.write_text("", encoding="utf-8")

    ff = subprocess.Popen(segment_cmd(stream, seg_dir, seconds),
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    stop = {"flag": False}
    prev = signal.getsignal(signal.SIGINT)
    signal.signal(signal.SIGINT, lambda *_: stop.__setitem__("flag", True))
    deadline = time.time() + max_minutes * 60 if max_minutes else None

    done = set()
    chunks = 0

    def drain():
        nonlocal chunks
        running = ff.poll() is None
        for i in completed_indices(segment_indices(seg_dir), running):
            if i in done:
                continue
            text = transcribe_segment(prefix, seg_dir / f"seg{i:05d}.wav", work).strip()
            done.add(i)
            chunks += 1
            if text:
                with out_txt.open("a", encoding="utf-8") as f:
                    f.write(text + "\n")
                if on_text:
                    on_text(text)

    try:
        while ff.poll() is None:
            if stop["flag"] or (deadline and time.time() > deadline):
                ff.send_signal(signal.SIGINT)  # let ffmpeg finalize the open chunk
                break
            drain()
            time.sleep(1)
        try:
            ff.wait(timeout=30)
        except subprocess.TimeoutExpired:
            ff.terminate()
        drain()  # final pass: the stream ended, so the last chunk is now complete
    finally:
        signal.signal(signal.SIGINT, prev)
        if ff.poll() is None:
            ff.terminate()

    text_all = out_txt.read_text(encoding="utf-8", errors="replace")
    words = len(text_all.split())
    return {"transcript": str(out_txt), "words": words, "thin": words < 1500,
            "source": src, "chunks": chunks}


def main():
    ap = argparse.ArgumentParser(description="Transcribe a live audio stream.")
    ap.add_argument("source", help="stream URL (direct or Twitch/YouTube/X) or file")
    ap.add_argument("out_txt", help="transcript file, appended as chunks land")
    ap.add_argument("--segment-seconds", type=int, default=30,
                    help="chunk length; whisper's window is 30s (default)")
    ap.add_argument("--max-minutes", type=float, default=None,
                    help="stop after this long (default: until the stream ends)")
    a = ap.parse_args()
    summary = run_live(a.source, a.out_txt, a.segment_seconds, a.max_minutes,
                       on_text=lambda t: print(t, file=sys.stderr, flush=True))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
