#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9"]
# ///
# ABOUTME: Splits an English book PDF into sections (from its outline, else from its printed contents page) and
# ABOUTME: writes each section's cleaned text (heads, feet, folios stripped; paragraphs rebuilt) into a work directory.
#
# Usage: extract.py <book.pdf> [--work <dir>] [--sections <sections.json>]
# Two-up scans (two book pages per PDF page) are split into single pages first (<work>/pages.pdf).
# Writes <work>/sections.json and <work>/text/<id>-<slug>.txt; prints one line per section.
# When neither the outline nor the contents page yields sections it exits 2 and says how to hand-write them.
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
    """'1. Getting Started' / 'Chapter One: Getting Started' -> (1, 'Getting Started'); else (None, title)."""
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


PART_TITLE = re.compile(r"^\s*part\b", re.I)


def flatten_outline(pdf):
    """The outline as a flat, page-sorted list of (title, zero-based page): the root entries, the children of
    Part entries, and the children of a lone root entry. Deeper entries are subsections within a chapter."""
    out = []
    with pdf.open_outline() as outline:
        def walk(items, descend):
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
                if descend(it):
                    walk(it.children, lambda child: PART_TITLE.match(str(child.title)) is not None)
        walk(outline.root, lambda it: len(outline.root) == 1 or PART_TITLE.match(str(it.title)) is not None)
    out.sort(key=lambda x: x[1])
    return out


def build_sections(entries, page_count):
    """Outline entries [(title, zero-based page)] -> section dicts with 1-based inclusive page ranges."""
    sections = []
    seen_chapter = False
    for i, (title, page) in enumerate(entries):
        # Two consecutive bookmarks can point at the same page (e.g. a part divider and a chapter opener),
        # which would make end < start; keep every range at least one page so it stays a valid inclusive span.
        end = max(entries[i + 1][1] if i + 1 < len(entries) else page_count, page + 1)
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


# "DEFINITIONS 2-1", "LOA 4-2", "Appendix A-3": a section-numbered running foot.
RUNNING_FOOT = re.compile(r"(?:\d+\s+)?(?:[A-Z][A-Za-z0-9 ,&/'’\-–]*\s+)?(?:[A-Za-z]+\s*)?(?:[A-Z]|\d+)-\d+")


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
            if RUNNING_FOOT.fullmatch(stripped):
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
    if end < start:
        return []
    out = subprocess.run(["pdftotext", "-layout", "-f", str(start), "-l", str(end), str(pdf_path), "-"],
                         capture_output=True, text=True, check=True).stdout
    return out.split("\f")[: end - start + 1]


def split_two_up(book, out):
    """Pages wider than tall hold two book pages side by side: write a PDF with each half as its own page.
    Returns the path when anything was split, else None."""
    src = pikepdf.open(book)
    if not any((float(p.mediabox[2]) - float(p.mediabox[0])) > 1.25 * (float(p.mediabox[3]) - float(p.mediabox[1])) for p in src.pages):
        return None
    dst = pikepdf.new()
    for page in src.pages:
        x0, y0, x1, y1 = (float(v) for v in page.mediabox)
        halves = [[x0, y0, (x0 + x1) / 2, y1], [(x0 + x1) / 2, y0, x1, y1]] if (x1 - x0) > 1.25 * (y1 - y0) else [[x0, y0, x1, y1]]
        for box in halves:
            dst.pages.append(page)
            dst.pages[-1].mediabox = box
            dst.pages[-1].cropbox = box
    dst.save(out)
    return out


LABEL = r"(?:Section|Chapter|Part|Appendix|LOA|Article|Letter)\s+([A-Za-z]+|\d+)"
LEADER_LINE = re.compile(r"^\s*(?:(" + LABEL + r")\s+)?(.+?)\s*[.…·]{3,}\s*([A-Za-z]*\s*\d+(?:-\d+)?|[ivxlc]+)\s*$", re.I)
LABEL_LINE = re.compile(r"^\s*(" + LABEL + r")\s+(\S.*?)\s*$", re.I)
HEAD_WORDS = {"loa": "letter of agreement"}
INDEX_LINE = re.compile(r"\S.*?[.…·\uFFFD\u2022]{3,}\s*\d+(?:,\s*\d+)*\s*$")


def page_lines(pdf_path, page):
    return [l.strip() for l in pdftotext_pages(pdf_path, page, page)[0].splitlines() if l.strip()]


def contents_entries(pdf_path, page_count):
    """(first contents page, [(label or None, title)]) read from the printed table of contents: lines with dot
    leaders and a page number, a wrapped title joined to its leader line."""
    entries, first, pending = [], None, None
    for page in range(1, min(page_count, 20) + 1):
        lines = page_lines(pdf_path, page)
        found = [m for m in (LEADER_LINE.match(l) for l in lines) if m]
        if len(found) < 3:
            if entries:
                break
            continue
        first = first or page
        for l in lines:
            m = LEADER_LINE.match(l)
            if m:
                label, title = m.group(1), m.group(3)
                if pending and not label:
                    label, title = pending[0], pending[1] + " " + title
                pending = None
                if not re.fullmatch(r"(table of )?contents|letters? of agreement|appendices|index", title.strip(), re.I):
                    entries.append((label, title.strip(" .")))
            else:
                lm = LABEL_LINE.match(l)
                pending = (lm.group(1), lm.group(3)) if lm and "contents" not in l.lower() else None
    return first, entries


def label_number(label):
    n = label.split()[-1].lower()
    return int(n) if n.isdigit() else NUMBER_WORDS.get(n)


def locate_sections(pdf_path, page_count, first_page, entries):
    """Find each contents entry's start page by its heading (label or title in the first lines of a page),
    scanning forward from the previous find. Returns [(title, zero-based page)] plus the entries not found."""
    # A heading sits in the first lines of its page: match only against the start of the page's text.
    all_lines = {p: page_lines(pdf_path, p) for p in range(first_page, page_count + 1)}
    heads = {p: re.sub(r"^(\d+|the)", "", _norm(" ".join(all_lines[p][:5])))[:48] for p in all_lines}
    found, missing, pos = [], [], first_page
    for label, title in entries:
        keys, opening = [_norm(title)[:24]], []
        if label:
            word, n = label.split()[0].lower(), label_number(label)
            keys += [_norm(label)] + [_norm(f"{word} {w}") for w, k in NUMBER_WORDS.items() if n is not None and k == n]
            # a heading phrase such as "Letter of Agreement" must open the page, or it matches a closing formula
            opening = [_norm(HEAD_WORDS[word])] if word in HEAD_WORDS else []
        if opening:
            keys = []
        hit = next((p for p in range(pos + 1, page_count + 1)
                    if any(k and k in heads[p] for k in keys) or any(heads[p].startswith(k) for k in opening)), None)
        if hit is None:
            missing.append(f"{label or ''} {title}".strip())
            continue
        pos = hit
        n = label_number(label) if label else None
        word = label.split()[0].lower() if label else ""
        name = f"{n}. {title}" if word in ("chapter", "section") and n else f"{label}: {title}" if label else title
        found.append((name, hit - 1))
    # An index after the last section: a heading "Index", or pages of leader lines ("Term ....... 12, 40").
    def index_like(p):
        return heads[p].startswith("index") or sum(1 for l in all_lines[p] if INDEX_LINE.search(l)) >= 5
    index = next((p for p in range(pos + 1, page_count + 1) if index_like(p)), None)
    if index:
        found.append(("Index", index - 1))
    return found, missing


def default_work(book):
    """<book dir>/.translate/<slug>, or ~/Documents/translate/<slug> when the book's folder is not writable
    (macOS keeps this process out of some folders); the book is copied there so every later step can read it."""
    work = book.parent / ".translate" / slugify(book.stem)
    try:
        work.mkdir(parents=True, exist_ok=True)
        return work, book
    except (PermissionError, OSError):
        work = Path.home() / "Documents" / "translate" / slugify(book.stem)
        work.mkdir(parents=True, exist_ok=True)
        copy = work / book.name
        if not copy.exists():
            copy.write_bytes(book.read_bytes())
        print(f"{book.parent} is not writable: working in {work} on a copy of the book", file=sys.stderr)
        return work, copy


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("book")
    ap.add_argument("--work")
    ap.add_argument("--sections", help="hand-written sections.json ([{title,start}] or full) to use instead of the outline")
    args = ap.parse_args()
    source = Path(args.book).resolve()
    if args.work:
        work, book = Path(args.work).resolve(), source
        work.mkdir(parents=True, exist_ok=True)
    else:
        work, book = default_work(source)
    (work / "text").mkdir(exist_ok=True)
    split = split_two_up(book, work / "pages.pdf")
    if split:
        print(f"two-up pages split into {split}", file=sys.stderr)
        book = split

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
        first, listed = contents_entries(book, page_count)
        located, missing = locate_sections(book, page_count, first, listed) if listed else ([], [])
        if listed and len(located) >= max(2, 0.6 * len(listed)):
            entries = [("COVER", 0), ("CONTENTS", first - 1)] + located
            print(f"no outline: {len(listed) - len(missing)} of {len(listed)} contents entries located by their headings"
                  + (f"; not found: {', '.join(missing)}" if missing else ""), file=sys.stderr)
        else:
            print(f"{book.name} has no usable outline and its contents page yields {len(located)} of {len(listed)} sections. "
                  f"Write {work / 'sections.json'} by hand from the printed table of contents as "
                  f"[{{\"title\": \"COVER\", \"start\": 1}}, {{\"title\": \"Preface\", \"start\": 10}}, ...] (1-based pages of "
                  f"{book}, in order) and rerun with --sections {work / 'sections.json'}", file=sys.stderr)
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

    meta = {"book": str(book), "source": str(source), "title": title, "author": author, "pages": page_count,
            "page_size": page_size, "sections": sections}
    (work / "sections.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    for s in sections:
        extra = f"{s['words']} words -> {s['file']}" if "file" in s else "(not translated)"
        print(f"{s['id']} {s['kind']:8} p{s['start']}-{s['end']:<4} {s['outline_title']}: {extra}")
    print(f"work dir: {work}")


if __name__ == "__main__":
    main()
