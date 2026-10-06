#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow>=10"]
# ///
# ABOUTME: Lints the translated Markdown in <work>/translated/ for translator-output defects the render guard misses or
# ABOUTME: reports late: lost or invented image tokens, unclosed inline tags, markup or images inside math, unconverted math.
"""Usage: lint_md.py <work dir> [section id ...]  -- one line per problem (kind, file:line, detail), then a summary.

Run it after translating and before rendering. Fix each problem in the section's Markdown, or retranslate the
section (translate.mjs --force --only <id> --effort medium) when a section is full of "unconverted math": the
translator skipped the LaTeX conversion for it.
"""
import collections
import json
import re
import sys
from pathlib import Path

IMG_RE = re.compile(r"⟦IMG:([^⟧]+)⟧")
MATH_RE = re.compile(r"\\\((.*?)\\\)|\\\[(.*?)\\\]", re.S)
CODE_SPAN_RE = re.compile(r"`[^`\n]*`")
INLINE_TAGS = ("code", "sub", "sup", "em", "strong", "i", "b")
MD_IN_MATH_RE = re.compile(r"(?<![{^_\\])\*{1,2}[A-Za-z][^*{}]*?\*{1,2}")  # *x* or **x** inside \( \); not A^{*}
UNICODE_SCRIPT_RE = re.compile(r"[²³¹⁰-₟]")  # ² ³ ¹ ⁰-⁹ ₀-₉ ⁿ …
# A math variable the translator left as text: an italic single letter, or HTML sub/superscripts.
UNCONVERTED_RE = re.compile(r"(?<![*\\\w])\*[A-Za-z]\*(?!\*)|<(em|i)>[A-Za-z]</\1>|<su[bp]>")


def line_of(text, pos):
    return text.count("\n", 0, pos) + 1


def blank(m):
    """Replace a match with spaces of the same length, keeping offsets (and so line numbers) intact."""
    return re.sub(r"[^\n]", " ", m.group(0))


SYMBOL_MAX_WIDTH = 100  # px: an image this narrow is a symbol (√5, ∅) even when the extractor saw it as a block


def symbol_image(work, entry):
    """True when the image is a single symbol rather than a figure or a display equation."""
    from PIL import Image
    try:
        with Image.open(work / "images" / entry["file"]) as im:
            return im.width < SYMBOL_MAX_WIDTH
    except (OSError, KeyError):
        return False


def lint_section(name, src, md, images, is_symbol):
    problems = []
    add = lambda kind, pos, detail: problems.append((kind, f"{name}:{line_of(md, pos)}", detail))

    src_imgs, md_imgs = set(IMG_RE.findall(src)), IMG_RE.findall(md)
    # A block image (a figure, a display equation) must survive; an inline symbol image may rightly have become
    # LaTeX (the translator is told to write an image-only symbol such as √5 or ⌊ as LaTeX), and so may a narrow
    # symbol the extractor took for a block because it sat on its own source line.
    for key in sorted(k for k in src_imgs - set(md_imgs) if images.get(k, {}).get("block", True) and not is_symbol(k)):
        problems.append(("image", name, f"missing ⟦IMG:{key}⟧ (a block image in the source, not in the translation)"))
    for m in IMG_RE.finditer(md):
        if m.group(1) not in src_imgs:
            add("image", m.start(), f"unknown ⟦IMG:{m.group(1)}⟧ (not in the source)")

    for opener, closer in (("\\(", "\\)"), ("\\[", "\\]")):
        if md.count(opener) != md.count(closer):
            add("math", 0, f"unbalanced {opener} {closer}: {md.count(opener)} open, {md.count(closer)} close")

    for m in MATH_RE.finditer(md):
        body = m.group(1) if m.group(1) is not None else m.group(2)
        snippet = m.group(0)[:50].replace("\n", " ")
        if MD_IN_MATH_RE.search(body):
            add("math", m.start(), f"markdown in math: {snippet}")
        if UNICODE_SCRIPT_RE.search(body):
            add("math", m.start(), f"unicode sub/superscript in math: {snippet}")
        if IMG_RE.search(body):
            add("math", m.start(), f"image in math: {snippet}")

    prose = CODE_SPAN_RE.sub(blank, MATH_RE.sub(blank, md))
    for tag in INLINE_TAGS:
        opens = [m.start() for m in re.finditer(rf"<{tag}\b[^>]*>", prose)]
        closes = len(re.findall(rf"</{tag}>", prose))
        if len(opens) > closes:
            add("tag", opens[closes] if closes < len(opens) else 0, f"unclosed <{tag}>: {len(opens)} open, {closes} close")
    unconverted = list(UNCONVERTED_RE.finditer(prose))
    if unconverted:
        add("math", unconverted[0].start(),
            f"unconverted math: {len(unconverted)} variable(s) left as text, e.g. {unconverted[0].group(0)!r}")
    return problems


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    work = Path(sys.argv[1])
    only = set(sys.argv[2:])
    sections = json.loads((work / "sections.json").read_text())["sections"]
    images_path = work / "images.json"
    images = json.loads(images_path.read_text()) if images_path.exists() else {}
    problems = []
    for s in sections:
        if not s.get("file") or (only and s["id"] not in only):
            continue
        md_path = work / "translated" / (Path(s["file"]).stem + ".md")
        if md_path.exists():
            problems += lint_section(md_path.name, (work / s["file"]).read_text(), md_path.read_text(), images,
                                      lambda k: k in images and symbol_image(work, images[k]))
    for kind, where, detail in problems:
        print(f"{kind:<6} {where:<52} {detail}")
    counts = collections.Counter(kind for kind, _, _ in problems)
    print(f"\n{len(problems)} problems: " + ", ".join(f"{k} {counts[k]}" for k in ("image", "tag", "math")))


if __name__ == "__main__":
    main()
