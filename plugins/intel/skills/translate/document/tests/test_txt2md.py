#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
# ABOUTME: Tests txt2md.py on a real lecture transcript (one sentence per line): every sentence survives in order,
# ABOUTME: grouped into single-line paragraphs of bounded length.
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parent / "scripts" / "txt2md.py"
TRANSCRIPT = HERE / "fixtures" / "transcript.txt"
fails = []


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


with tempfile.TemporaryDirectory() as d:
    out = Path(d) / "t.md"
    subprocess.run([str(SCRIPT), str(TRANSCRIPT), str(out)], check=True, capture_output=True)
    paras = out.read_text().strip().split("\n\n")
    lines = [l.strip() for l in TRANSCRIPT.read_text().splitlines() if l.strip()]
    check("several paragraphs", len(paras) > 5, str(len(paras)))
    check("each paragraph is one line", all("\n" not in p for p in paras))
    longest = max(len(p.split()) for p in paras)
    check("paragraphs stay short", longest <= 250, str(longest))
    check("every sentence kept in order", " ".join(paras).split() == " ".join(lines).split())

sys.exit(1 if fails else 0)
