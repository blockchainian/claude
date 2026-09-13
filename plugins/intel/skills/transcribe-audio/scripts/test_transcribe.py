#!/usr/bin/env python3
# ABOUTME: Tests the command shape of transcribe_audio and, if tools exist, e2e.
# ABOUTME: The e2e path generates a real clip with ffmpeg and runs whisper on it.

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


def main():
    ta = load("transcribe_audio")

    # build_cmd shape
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

    # end-to-end whisper smoke test (only if ffmpeg + a whisper runner exist)
    have_ff = shutil.which("ffmpeg")
    have_whisper = shutil.which("mlx_whisper") or shutil.which("uv")
    if have_ff and have_whisper:
        clip = tmp / "clip.wav"
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i",
                        "sine=frequency=440:duration=1", "-ac", "1", "-ar",
                        "16000", str(clip)], check=True)
        out = tmp / "t.txt"
        r = subprocess.run([sys.executable, str(HERE / "transcribe_audio.py"),
                            str(clip), str(out)], capture_output=True, text=True)
        check("e2e transcribe exits 0", r.returncode == 0, r.stderr[-300:])
        check("e2e wrote out.txt", out.is_file())
        try:
            js = json.loads(r.stdout)
            check("e2e prints json transcript", js.get("transcript") == str(out), r.stdout[-200:])
        except Exception as e:
            check("e2e prints json", False, f"{e}: {r.stdout[-200:]}")
    else:
        print("SKIP: e2e whisper smoke (ffmpeg or whisper runner missing)")

    print()
    if fails:
        print(f"{len(fails)} FAILED: {fails}")
        sys.exit(1)
    print("ALL PASSED")


if __name__ == "__main__":
    main()
