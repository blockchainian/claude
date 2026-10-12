#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
# ABOUTME: Splits a Markdown document into translatable blocks and joins their translations back.
# ABOUTME: Fenced code and Mermaid are never sent; heading markers are restored after translation.
#
# Usage: md_segments.py split <src.md> <segments.json>
#        md_segments.py join <src.md> <translations.json> <dest.md>
# A block is a run of non-blank lines; fenced code (Mermaid included) is never translated. Each segment lists what its
# translation must keep verbatim: inline code spans and link targets. A heading's marker is stripped before
# translation and restored after, so it cannot be lost.
import hashlib, json, re, sys
from pathlib import Path

FENCE_RE = re.compile(r"^(`{3,}|~{3,})")
HEADING_RE = re.compile(r"^(#{1,6} )(.*)$")
KEEP_RE = re.compile(r"(`+)[^\n]*?\1|\]\(([^)\s]+)")


def blocks(text):
    """Yields (kind, text): "code" for a fenced block, "text" for a run of non-blank lines, "blank" for blank lines."""
    lines, i = text.split("\n"), 0
    while i < len(lines):
        m = FENCE_RE.match(lines[i])
        if m:
            j = i + 1
            while j < len(lines) and not lines[j].startswith(m.group(1)):
                j += 1
            yield "code", "\n".join(lines[i:j + 1]); i = j + 1
        elif not lines[i].strip():
            yield "blank", lines[i]; i += 1
        else:
            j = i
            while j < len(lines) and lines[j].strip() and not FENCE_RE.match(lines[j]):
                j += 1
            yield "text", "\n".join(lines[i:j]); i = j


def seg_id(text):
    """A block's id is a hash of its text, so saved translations stay valid when the source or the splitting changes."""
    return "b" + hashlib.sha1(text.encode()).hexdigest()[:12]


def segment(block):
    """Returns (prefix, translatable text) for a text block."""
    m = HEADING_RE.match(block)
    return (m.group(1), m.group(2)) if m and "\n" not in block else ("", block)


def split(src, out):
    segs = {}
    for kind, block in blocks(Path(src).read_text()):
        if kind == "text":
            text = segment(block)[1]
            keep = [m.group(0) if m.group(1) else m.group(2) for m in KEEP_RE.finditer(text)]
            segs[seg_id(text)] = {"id": seg_id(text), "text": text, "keep": keep}
    segs = list(segs.values())
    Path(out).write_text(json.dumps(segs, ensure_ascii=False, indent=1))
    print(f"{len(segs)} segments")


def join(src, translations, dest):
    zh = json.loads(Path(translations).read_text())
    parts = []
    for kind, block in blocks(Path(src).read_text()):
        prefix, text = segment(block) if kind == "text" else ("", block)
        parts.append(prefix + zh.get(seg_id(text), text) if kind == "text" else block)
    Path(dest).write_text("\n".join(parts))


if __name__ == "__main__":
    {"split": split, "join": join}[sys.argv[1]](*sys.argv[2:])
