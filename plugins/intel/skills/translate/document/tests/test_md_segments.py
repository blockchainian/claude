#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
# ABOUTME: Tests md_segments.py on a real course note: what is sent for translation, and that join puts translations
# ABOUTME: back with headings, code and Mermaid fences intact.
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parent / "scripts" / "md_segments.py"
NOTES = HERE / "fixtures" / "notes.md"
fails = []


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


with tempfile.TemporaryDirectory() as d:
    d = Path(d)
    subprocess.run([str(SCRIPT), "split", str(NOTES), str(d / "segs.json")], check=True, capture_output=True)
    segs = json.loads((d / "segs.json").read_text())
    texts = [s["text"] for s in segs]
    check("no fenced code or Mermaid is sent", not any("```" in t or "block-beta" in t for t in texts))
    check("heading markers are stripped", "Defending Buffer Overflows" in texts and not any(t.startswith("#") for t in texts))
    check("ids are content hashes, unique", len({s["id"] for s in segs}) == len(segs) and all(len(s["id"]) == 13 for s in segs))
    link = next(s for s in segs if "](https://blog.zimperium.com" in s["text"])
    check("link targets are kept", "https://blog.zimperium.com/whatsapp-buffer-overflow-vulnerability-under-the-scope/" in link["keep"])
    canary = next(s for s in segs if "`RET`" in s["text"])
    check("inline code is kept", "`RET`" in canary["keep"])

    # Joining the source's own text reproduces the file byte for byte.
    (d / "same.json").write_text(json.dumps({s["id"]: s["text"] for s in segs}))
    subprocess.run([str(SCRIPT), "join", str(NOTES), str(d / "same.json"), str(d / "same.md")], check=True)
    check("identity join reproduces the source", (d / "same.md").read_text() == NOTES.read_text())

    # A translated heading gets its marker back; untranslated blocks stay as they were.
    title = next(s for s in segs if s["text"] == "Defending Buffer Overflows")
    (d / "zh.json").write_text(json.dumps({title["id"]: "防御缓冲区溢出"}, ensure_ascii=False))
    subprocess.run([str(SCRIPT), "join", str(NOTES), str(d / "zh.json"), str(d / "zh.md")], check=True)
    out = (d / "zh.md").read_text()
    check("heading marker restored", out.startswith("# 防御缓冲区溢出\n"))
    check("Mermaid fence untouched", out.count("```mermaid") == NOTES.read_text().count("```mermaid"))

print(f"{len(fails)} failed" if fails else "all passed")
sys.exit(1 if fails else 0)
