#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9"]
# ///
# ABOUTME: Splits an English book PDF into sections from its outline and writes each section's cleaned
# ABOUTME: text (running heads and folios stripped, paragraphs rebuilt) into the book's work directory.
#
# Usage: extract.py <book.pdf> [--work <dir>] [--sections <sections.json>]
# Writes <work>/sections.json and <work>/text/<id>-<slug>.txt; prints one line per section.
# Without a usable outline it exits 2 and tells the caller how to hand-write sections.json.
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import pikepdf

NUMBER_WORDS = {w: i for i, w in enumerate(
    ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
     "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty"])}
for tens, base in (("twenty", 20), ("thirty", 30), ("forty", 40)):
    NUMBER_WORDS[tens] = base
    for w, i in list(NUMBER_WORDS.items())[1:10]:
        NUMBER_WORDS[f"{tens}-{w}"] = base + i
        NUMBER_WORDS[f"{tens} {w}"] = base + i

CN_DIGITS = "零一二三四五六七八九"


def chinese_number(n):
    """1 -> 一, 10 -> 十, 24 -> 二十四, 105 -> 一百零五."""
    if n < 10:
        return CN_DIGITS[n]
    if n < 20:
        return "十" + (CN_DIGITS[n % 10] if n % 10 else "")
    if n < 100:
        return CN_DIGITS[n // 10] + "十" + (CN_DIGITS[n % 10] if n % 10 else "")
    rest = n % 100
    return CN_DIGITS[n // 100] + "百" + ("零" + chinese_number(rest) if 0 < rest < 10 else chinese_number(rest) if rest else "")


def slugify(title):
    s = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
    return s[:40] or "section"


def parse_title(title):
    """'1. Traction Channels' / 'Chapter One: Traction Channels' -> (1, 'Traction Channels'); else (None, title)."""
    t = title.strip()
    m = re.match(r"^(\d+)[.:\s]+\s*(.+)$", t)
    if m:
        return int(m.group(1)), m.group(2).strip()
    m = re.match(r"^chapter\s+([a-z\- ]+?)[.:\s]+(.+)$", t, re.I)
    if m and m.group(1).strip().lower() in NUMBER_WORDS:
        return NUMBER_WORDS[m.group(1).strip().lower()], m.group(2).strip()
    m = re.match(r"^chapter\s+(\d+)[.:\s]+(.+)$", t, re.I)
    if m:
        return int(m.group(1)), m.group(2).strip()
    return None, t


def classify(title, chapter, seen_chapter):
    t = title.strip().lower()
    if re.search(r"\bcover\b", t):
        return "cover"
    if t in ("contents", "table of contents"):
        return "contents"
    if t in ("index", "notes", "endnotes", "copyright", "title page", "half title", "also by", "about the author"):
        return "skip"
    if chapter is not None:
        return "chapter"
    return "back" if seen_chapter else "front"


def flatten_outline(pdf):
    """The outline as a flat, page-sorted list of (title, zero-based page)."""
    out = []
    with pdf.open_outline() as outline:
        def walk(items):
            for it in items:
                page = None
                dest = it.destination
                try:
                    if dest is None and it.action is not None and "/D" in it.action:
                        dest = it.action.D
                    if dest is not None:
                        target = dest[0] if isinstance(dest, pikepdf.Array) else dest
                        page = pikepdf.Page(target).index if isinstance(target, pikepdf.Dictionary) else int(target)
                except Exception:
                    page = None
                if page is not None:
                    out.append((str(it.title), page))
                walk(it.children)
        walk(outline.root)
    out.sort(key=lambda x: x[1])
    return out


def build_sections(entries, page_count):
    """Outline entries [(title, zero-based page)] -> section dicts with 1-based inclusive page ranges."""
    sections = []
    seen_chapter = False
    for i, (title, page) in enumerate(entries):
        end = entries[i + 1][1] if i + 1 < len(entries) else page_count
        chapter, clean = parse_title(title)
        kind = classify(title, chapter, seen_chapter)
        if kind == "chapter":
            seen_chapter = True
        sections.append({"id": f"{i + 1:02d}", "title": clean, "outline_title": title, "chapter": chapter,
                         "label": f"第{chinese_number(chapter)}章" if chapter else None, "kind": kind,
                         "start": page + 1, "end": end})
    return sections


def _norm(s):
    return re.sub(r"[^a-z0-9]+", "", s.lower())


KEEP_HYPHEN = set(NUMBER_WORDS) | {"self", "non", "co", "pre", "re", "anti", "multi", "semi", "well", "long",
                                     "short", "high", "low", "full", "half", "mid", "post", "cross", "e", "x"}


def join_hyphen(prev, word):
    """'rea-' + 'sons' -> 'reasons'; 'twenty-' + 'five' keeps its hyphen."""
    stem = prev[:-1]
    if stem.rsplit(" ", 1)[-1].lower() in KEEP_HYPHEN:
        return prev + word
    return stem + word


def clean_pages(pages, running_heads):
    """pdftotext -layout pages -> paragraphs. Drops running heads and folios, rebuilds paragraphs from
    indentation and blank lines, marks indented blocks as quotes, and re-joins words the line breaks hyphenated."""
    heads = {_norm(h) for h in running_heads if _norm(h)}
    paragraphs = []
    current = []  # (text, indented)

    def flush():
        nonlocal current
        if current:
            text = " ".join(t for t, _ in current)
            if len(current) > 1 and all(ind for _, ind in current):
                text = "> " + text
            paragraphs.append(text)
        current = []

    def add_line(text, indented, new_para):
        if current and current[-1][0].endswith("-") and text[:1].islower():
            first, _, rest = text.partition(" ")
            current[-1] = (join_hyphen(current[-1][0], first), current[-1][1])
            if rest:
                current.append((rest, indented))
            return
        if new_para:
            flush()
        current.append((text, indented))

    for page in pages:
        kept = []
        for raw in page.split("\n"):
            line = raw.rstrip()
            if not line.strip():
                kept.append(None)
                continue
            stripped = line.strip()
            norm = _norm(stripped)
            if not norm or norm in heads:
                continue
            if re.fullmatch(r"[ivxlcdm]+|\d+|[|=\-—_.\s]*\d+[|=\-—_.\s]*", stripped, re.I):
                continue
            if not re.search(r"[A-Za-z]{2}", stripped):
                continue
            kept.append(line)
        body = [k for k in kept if k is not None]
        if not body:
            continue
        indent_min = min(len(k) - len(k.lstrip(" ")) for k in body)
        blank_before = False
        prev_indent = None
        for k in kept:
            if k is None:
                blank_before = True
                continue
            indent = len(k) - len(k.lstrip(" "))
            indented = indent >= indent_min + 3
            same_block = indented and prev_indent is not None and abs(indent - prev_indent) <= 2  # a blank line inside a quote
            deeper = indented and (prev_indent is None or indent > prev_indent + 2)
            dedent = indented and prev_indent is not None and indent < prev_indent - 2  # a paragraph after a quote
            new_para = deeper or dedent or (blank_before and prev_indent is not None and not same_block)
            add_line(k.strip(), indented, new_para)
            blank_before = False
            prev_indent = indent
    flush()
    return [re.sub(r"\s+", " ", p).strip() for p in paragraphs if p.strip()]


def pdftotext_pages(pdf_path, start, end):
    out = subprocess.run(["pdftotext", "-layout", "-f", str(start), "-l", str(end), str(pdf_path), "-"],
                         capture_output=True, text=True, check=True).stdout
    return out.split("\f")[: end - start + 1]


def default_work(book):
    return book.parent / ".translate" / slugify(book.stem)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("book")
    ap.add_argument("--work")
    ap.add_argument("--sections", help="hand-written sections.json ([{title,start}] or full) to use instead of the outline")
    args = ap.parse_args()
    book = Path(args.book).resolve()
    work = Path(args.work).resolve() if args.work else default_work(book)
    work.mkdir(parents=True, exist_ok=True)
    (work / "text").mkdir(exist_ok=True)

    pdf = pikepdf.open(book)
    page_count = len(pdf.pages)
    info = pdf.docinfo
    title = str(info.get("/Title", book.stem)).strip()
    author = str(info.get("/Author", "")).strip()
    box = [float(x) for x in pdf.pages[0].mediabox]
    page_size = [round(box[2] - box[0], 2), round(box[3] - box[1], 2)]

    if args.sections:
        given = json.loads(Path(args.sections).read_text())
        entries = [(s["title"], int(s["start"]) - 1) for s in (given["sections"] if isinstance(given, dict) else given)]
    else:
        entries = flatten_outline(pdf)
    if len(entries) < 2:
        print(f"{book.name} has no usable outline. Write {work / 'sections.json'} by hand from the printed table of "
              f"contents as [{{\"title\": \"Preface\", \"start\": 10}}, ...] (1-based PDF pages, in order) and rerun with "
              f"--sections {work / 'sections.json'}", file=sys.stderr)
        sys.exit(2)

    sections = build_sections(entries, page_count)
    short_title = re.split(r"[:\-–—(]", title)[0].strip()
    for s in sections:
        if s["kind"] in ("cover", "contents", "skip"):
            continue
        heads = [s["title"], s["outline_title"], short_title, title]
        if s["chapter"]:
            heads += [f"chapter {s['chapter']}"] + [f"chapter {w}" for w, n in NUMBER_WORDS.items() if n == s["chapter"]]
        pages = pdftotext_pages(book, s["start"], s["end"])
        paragraphs = clean_pages(pages, heads)
        s["file"] = f"text/{s['id']}-{slugify(s['title'])}.txt"
        s["words"] = sum(len(p.split()) for p in paragraphs)
        (work / s["file"]).write_text("\n\n".join(paragraphs) + "\n")

    meta = {"book": str(book), "title": title, "author": author, "pages": page_count, "page_size": page_size,
            "sections": sections}
    (work / "sections.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    for s in sections:
        extra = f"{s['words']} words -> {s['file']}" if "file" in s else "(not translated)"
        print(f"{s['id']} {s['kind']:8} p{s['start']}-{s['end']:<4} {s['outline_title']}: {extra}")
    print(f"work dir: {work}")


if __name__ == "__main__":
    main()
