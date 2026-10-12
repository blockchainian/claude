#!/usr/bin/env python3
# ABOUTME: Puts back into each translation what must match the source verbatim: its Markdown tables (the translator
# ABOUTME: sometimes shifts a cell; data tables stay in English) and its title's section number ("3", not "3.").
#
# Usage: restore_verbatim.py <work>
import json
import re
import sys
from pathlib import Path

TABLE_RE = re.compile(r"(?:^\|.*\|[ \t]*\n?)+", re.M)
NUMBER_RE = re.compile(r"^# ((?:\d+|[A-Z])(?:\.\d+)*\.?)\s+")


def restore_number(source, text):
    """The translation's title with the source title's section number written exactly as in the source."""
    src = NUMBER_RE.match(source)
    if not src:
        return text
    return NUMBER_RE.sub(lambda _: f"# {src.group(1)} ", text, count=1) if NUMBER_RE.match(text) else text


def main():
    work = Path(sys.argv[1])
    meta = json.loads((work / "sections.json").read_text())
    bad = 0
    for s in meta["sections"]:
        if not s.get("file"):
            continue
        out = work / "translated" / (Path(s["file"]).stem + ".md")
        if not out.exists():
            continue
        source = (work / s["file"]).read_text()
        src = TABLE_RE.findall(source)
        text = restore_number(source, out.read_text())
        got = TABLE_RE.findall(text)
        if len(src) != len(got):
            print(f"{s['id']}: {len(src)} tables in the source, {len(got)} in the translation; tables left as is")
            bad += 1
            src = got
        tables = iter(src)
        fixed = TABLE_RE.sub(lambda _: next(tables), text)
        if fixed != out.read_text():
            out.write_text(fixed)
            print(f"{s['id']}: restored")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
