#!/usr/bin/env python3
# ABOUTME: Tests batch + live transcription command shapes and, if tools exist, e2e.
# ABOUTME: The e2e paths generate a real clip with ffmpeg and run whisper on it.

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent

fails = []


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


def make_clip(path, seconds):
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i",
                    f"sine=frequency=440:duration={seconds}", "-ac", "1", "-ar",
                    "16000", str(path)], check=True)


def main():
    ta = load("transcribe_audio")
    tl = load("transcribe_live")

    # --- batch: build_cmd shape ---
    cmd = ta.build_cmd(["mlx_whisper"], "/tmp/a.mp3", "/tmp/out")
    check("build_cmd has model", "mlx-community/whisper-large-v3-turbo" in cmd, cmd)
    check("build_cmd txt output", "--output-format" in cmd and "txt" in cmd, cmd)
    check("build_cmd points at the audio", "/tmp/a.mp3" in cmd, cmd)
    check("build_cmd points at the out dir", "/tmp/out" in cmd, cmd)

    # get_audio: a local file is used in place (no download)
    tmp = Path(tempfile.mkdtemp())
    existing = tmp / "already.mp3"
    existing.write_bytes(b"not really audio")
    check("get_audio returns a local file unchanged",
          ta.get_audio(str(existing), tmp) == existing)

    # --- live: completed_indices (the chunk-readiness core) ---
    check("no segments -> none complete", tl.completed_indices([], True) == [])
    check("one segment, still running -> none complete (still being written)",
          tl.completed_indices([0], True) == [])
    check("running -> all but the last are complete",
          tl.completed_indices([0, 1, 2], True) == [0, 1])
    check("stream ended -> every segment is complete",
          tl.completed_indices([0, 1, 2], False) == [0, 1, 2])
    check("completed_indices sorts its input",
          tl.completed_indices([2, 0, 1], True) == [0, 1])

    # --- live: segment_cmd shape ---
    scmd = tl.segment_cmd("http://x/live.m3u8", tmp, 30)
    check("segment_cmd segments", "segment" in scmd and "-segment_time" in scmd, scmd)
    check("segment_cmd 30s", "30" in scmd, scmd)
    check("segment_cmd 16k mono wav", "16000" in scmd and scmd[-1].endswith(".wav"), scmd)

    # --- live: platform resolution chooser (no network) ---
    check("twitch -> streamlink", tl.resolver_cmd("https://www.twitch.tv/foo")[0] == "streamlink")
    check("youtube -> yt-dlp", tl.resolver_cmd("https://youtube.com/watch?v=x")[0] == "yt-dlp")
    check("youtu.be -> yt-dlp", tl.resolver_cmd("https://youtu.be/x")[0] == "yt-dlp")
    check("x spaces -> yt-dlp", tl.resolver_cmd("https://x.com/i/spaces/1")[0] == "yt-dlp")
    check("direct m3u8 -> no resolver", tl.resolver_cmd("https://cdn/live.m3u8") is None)
    check("direct mp3 stream -> no resolver", tl.resolver_cmd("https://cdn/audio.mp3") is None)

    # --- setup.sh --check is a no-op dry run that exits 0 ---
    r = subprocess.run(["bash", str(HERE / "setup.sh"), "--check"],
                       capture_output=True, text=True)
    check("setup --check exits 0", r.returncode == 0, r.stderr[-300:])
    check("setup --check reports ffmpeg", "ffmpeg" in (r.stdout + r.stderr), r.stdout[-200:])

    # --- e2e (only if ffmpeg + a whisper runner exist) ---
    have_ff = shutil.which("ffmpeg")
    have_whisper = shutil.which("mlx_whisper") or shutil.which("uv")
    if have_ff and have_whisper:
        # batch
        clip = tmp / "clip.wav"
        make_clip(clip, 1)
        out = tmp / "t.txt"
        r = subprocess.run([sys.executable, str(HERE / "transcribe_audio.py"),
                            str(clip), str(out)], capture_output=True, text=True)
        check("e2e batch exits 0", r.returncode == 0, r.stderr[-300:])
        check("e2e batch wrote out.txt", out.is_file())
        try:
            js = json.loads(r.stdout)
            check("e2e batch prints json transcript", js.get("transcript") == str(out), r.stdout[-200:])
        except Exception as e:
            check("e2e batch prints json", False, f"{e}: {r.stdout[-200:]}")

        # live: a finite local file stands in for a stream — ffmpeg segments it,
        # the loop drains every chunk once ffmpeg exits, and writes JSON.
        longclip = tmp / "long.wav"
        make_clip(longclip, 5)
        lout = tmp / "live.txt"
        r = subprocess.run([sys.executable, str(HERE / "transcribe_live.py"),
                            str(longclip), str(lout), "--segment-seconds", "2"],
                           capture_output=True, text=True)
        check("e2e live exits 0", r.returncode == 0, r.stderr[-400:])
        check("e2e live wrote out.txt", lout.is_file())
        try:
            js = json.loads(r.stdout)
            check("e2e live prints json summary", js.get("transcript") == str(lout), r.stdout[-200:])
            check("e2e live transcribed >=1 chunk", js.get("chunks", 0) >= 1, r.stdout[-200:])
        except Exception as e:
            check("e2e live prints json", False, f"{e}: {r.stdout[-200:]}")
    else:
        print("SKIP: e2e whisper smoke (ffmpeg or whisper runner missing)")

    print()
    if fails:
        print(f"{len(fails)} FAILED: {fails}")
        sys.exit(1)
    print("ALL PASSED")


if __name__ == "__main__":
    main()
