#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
# ABOUTME: Turns a plain-text transcript (one sentence or caption per line) into Markdown paragraphs for md_segments.py.
# ABOUTME: Lines are joined into single-line paragraphs of about 150 words, so each is translated and set as prose.
#
# Usage: txt2md.py <src.txt> <dest.md>
# A blank line in the source always ends a paragraph; otherwise a paragraph closes at the first line end past 150 words.
import sys
from pathlib import Path

WORDS = 150


def paragraphs(text):
    para, words = [], 0
    for line in text.splitlines():
        line = line.strip()
        if line:
            para.append(line)
            words += len(line.split())
        if para and (not line or words >= WORDS):
            yield " ".join(para)
            para, words = [], 0
    if para:
        yield " ".join(para)


src, dest = sys.argv[1:3]
Path(dest).write_text("\n\n".join(paragraphs(Path(src).read_text())) + "\n")
