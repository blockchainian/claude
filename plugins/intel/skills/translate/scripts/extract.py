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
import posixpath
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

import pikepdf

IMG_TOKEN = "⟦IMG:{}⟧"  # placeholder for a figure/equation stored as an image, kept verbatim through translation
IMG_TOKEN_RE = re.compile(r"⟦IMG:[^⟧]+⟧")
IMG_ZOOM = 4  # render extracted images at 4x (~288dpi) so small equations stay crisp; coordinates are divided back to pt

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


def image_pages(pdf_path, start, end, images_dir):
    """Text of pages [start, end] with each figure/equation image replaced by an ⟦IMG:key⟧ placeholder at its
    position. pdftohtml -xml gives text runs and extracted PNGs with pt coordinates (zoom 1); an image whose
    vertical band overlaps no text line is a block (own line), otherwise inline (placed by horizontal order).
    Returns (page strings, {key: {file, w, h, block}}) with the PNGs copied into images_dir as <key>.png."""
    if end < start:
        return [], {}
    images_dir.mkdir(parents=True, exist_ok=True)
    page_texts, mapping = [], {}
    z = IMG_ZOOM
    for page in range(start, end + 1):
        base = images_dir / f"_x{page}"
        subprocess.run(["pdftohtml", "-xml", "-zoom", str(z), "-f", str(page), "-l", str(page), str(pdf_path), str(base)],
                       capture_output=True, check=True)
        xml = base.with_suffix(".xml")
        root = ET.parse(xml).getroot() if xml.exists() else ET.Element("pdf2xml")
        texts, images = [], []
        for el in root.iter():
            if el.tag == "text":
                t = "".join(el.itertext())
                if t.strip():
                    a = el.attrib
                    texts.append({"top": float(a["top"]) / z, "left": float(a["left"]) / z,
                                  "w": float(a["width"]) / z, "h": float(a["height"]) / z, "t": re.sub(r"\s+", " ", t)})
            elif el.tag == "image":
                a = el.attrib
                images.append({"top": float(a["top"]) / z, "left": float(a["left"]) / z,
                               "w": float(a["width"]) / z, "h": float(a["height"]) / z, "src": a["src"]})
        for i, img in enumerate(images, 1):
            key = f"{page}_{i}"
            band_lo, band_hi = img["top"], img["top"] + img["h"]
            block = not any(t["top"] < band_hi and t["top"] + t["h"] > band_lo for t in texts)
            dest = images_dir / f"{key}{Path(img['src']).suffix or '.png'}"
            try:
                shutil.copyfile(img["src"], dest)
            except OSError:
                continue
            img["key"] = key
            img["block"] = block
            mapping[key] = {"file": dest.name, "w": img["w"], "h": img["h"], "block": block}
        # group text runs into lines (top within 4pt), then place inline images into their line by horizontal order
        items = sorted(texts, key=lambda e: (e["top"], e["left"]))
        lines, cur, curtop = [], [], None
        for e in items:
            if curtop is None or abs(e["top"] - curtop) <= 4:
                cur.append(e)
                curtop = e["top"] if curtop is None else curtop
            else:
                lines.append((curtop, cur))
                cur, curtop = [e], e["top"]
        if cur:
            lines.append((curtop, cur))
        rendered = []
        for top, runs in lines:
            toks = [(r["left"], r["t"]) for r in runs]
            for img in images:
                if img.get("key") is not None and not img["block"] and img["top"] < top + 10 and img["top"] + img["h"] > top - 2:
                    toks.append((img["left"], IMG_TOKEN.format(img["key"])))
                    img["key"] = None  # place an inline image on one line only
            toks.sort()
            rendered.append((top, " ".join(s for _, s in toks if s)))
        for img in images:
            if img.get("key") and img["block"]:
                rendered.append((img["top"], IMG_TOKEN.format(img["key"])))
        rendered.sort()
        page_texts.append("\n".join(s for _, s in rendered))
    for tmp in images_dir.glob("_x*"):  # drop pdftohtml's raw xml and its original-named image files
        tmp.unlink()
    return page_texts, mapping


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


def _ln(tag):
    """Local name of a namespaced ElementTree tag ('{ns}item' -> 'item')."""
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def epub_titles(z, nav_href, ncx_href):
    """Map each content file (basename, fragment dropped) to its TOC title, from the EPUB3 nav or EPUB2 ncx.
    The first title seen for a file wins, which is the file-level entry (sub-section entries carry #fragments)."""
    titles = {}
    base = lambda href: posixpath.basename(href.split("#", 1)[0])
    if nav_href:
        try:
            root = ET.fromstring(z.read(nav_href))
        except (KeyError, ET.ParseError):
            root = None
        if root is not None:
            for a in root.iter():
                if _ln(a.tag) == "a" and a.get("href"):
                    txt = " ".join("".join(a.itertext()).split())
                    b = base(a.get("href"))
                    if txt and b and b not in titles:
                        titles[b] = txt
    if not titles and ncx_href:
        try:
            root = ET.fromstring(z.read(ncx_href))
        except (KeyError, ET.ParseError):
            root = None
        if root is not None:
            for np in root.iter():
                if _ln(np.tag) != "navPoint":
                    continue
                label = next((e for e in np.iter() if _ln(e.tag) == "navLabel"), None)
                content = next((e for e in np.iter() if _ln(e.tag) == "content"), None)
                if label is not None and content is not None and content.get("src"):
                    txt = " ".join("".join(label.itertext()).split())
                    b = base(content.get("src"))
                    if txt and b not in titles:
                        titles[b] = txt
    return titles


def epub_headings(z, href):
    """The text of every <h1>..<h6> in a file, in order."""
    try:
        raw = z.read(href).decode("utf-8", "replace")
    except KeyError:
        return []
    return [" ".join(re.sub(r"<[^>]+>", "", m).split())
            for m in re.findall(r"<h[1-6][^>]*>(.*?)</h[1-6]>", raw, re.S | re.I)]


def epub_first_heading(z, href):
    hs = epub_headings(z, href)
    return hs[0] if hs else None


PART_TITLE_RE = re.compile(r"^\s*(?:part\s+)?[ivxlcdm]+\b", re.I)  # "I …", "II …", "Part IV …" (word boundary keeps "Introduction" out)


def chapter_from_headings(headings):
    """A chapter number + title read from a file's headings: 'CHAPTER 3' then 'Solving Problems by Searching', or a
    single 'N Title' heading. Used when the opener file's nav title is a part name, not the chapter title."""
    for i, h in enumerate(headings):
        m = re.match(r"^\s*chapter\s+(\d+)\b", h, re.I)
        if m:
            return int(m.group(1)), (headings[i + 1].strip().title() if i + 1 < len(headings) else h)
        num, clean = parse_title(h)
        if num is not None:
            return num, clean
    return None, None


_BLOCK_TAG = re.compile(r"</?(?:p|div|li|ul|ol|table|tr|td|section|figure|figcaption|h[1-6]|br|blockquote)\b[^>]*>", re.I)


def _img_inline(body, pos):
    """An <img> is inline (a symbol within a line of text) only when a word character sits right before or after
    it WITHIN THE SAME block element; a display equation alone in its own <p>/<div> is a block. Looking only up
    to the nearest block boundary avoids picking up text from a neighbouring paragraph."""
    end = body.find(">", pos) + 1
    left = re.sub(r"<[^>]+>", "", _BLOCK_TAG.split(body[max(0, pos - 240):pos])[-1]).rstrip()
    right = re.sub(r"<[^>]+>", "", _BLOCK_TAG.split(body[end:end + 240])[0]).lstrip()
    W = re.compile(r"[0-9A-Za-z一-鿿]")
    return bool(left and W.match(left[-1])) or bool(right and W.match(right[0]))


def epub_fragment(z, href, work, keep_images, counter):
    """Clean one XHTML file into a fragment for translation: body only, scripts/anchors removed, each <img>
    replaced by an ⟦IMG:key⟧ placeholder (block on its own line, inline within the line) and the image copied
    into <work>/images. The math and emphasis tags (strong/em/sub/sup/span) are kept for the translator. Returns
    (fragment, {key: {file, w, h, block}}, next counter)."""
    raw = z.read(href).decode("utf-8", "replace")
    m = re.search(r"<body[^>]*>(.*)</body>", raw, re.S | re.I)
    body = m.group(1) if m else raw
    body = re.sub(r"<script.*?</script>", "", body, flags=re.S | re.I)
    body = re.sub(r"<style.*?</style>", "", body, flags=re.S | re.I)
    body = re.sub(r"<a\b[^>]*>(.*?)</a>", r"\1", body, flags=re.S | re.I)
    body = re.sub(r"<a\b[^>]*?/>", "", body, flags=re.I)  # self-closing target anchors (<a id=.../>)
    imgs = {}
    hdir = posixpath.dirname(href)

    def img_repl(mm):
        nonlocal counter
        src = re.search(r'src="([^"]+)"', mm.group(0)) or re.search(r"src='([^']+)'", mm.group(0))
        if not src:
            return ""
        counter += 1
        key = f"e{counter}"
        img_href = posixpath.normpath(posixpath.join(hdir, src.group(1)))
        inline = _img_inline(body, mm.start())
        if not keep_images:
            return ""
        try:
            data = z.read(img_href)
        except KeyError:
            return ""
        ext = Path(img_href).suffix or ".png"
        (work / "images" / f"{key}{ext}").write_bytes(data)
        imgs[key] = {"file": f"{key}{ext}", "w": 0, "h": 0, "block": not inline}
        token = IMG_TOKEN.format(key)
        return token if inline else f"\n{token}\n"

    body = re.sub(r"<img\b[^>]*?/?>", img_repl, body, flags=re.I)
    # Drop EPUB styling leftovers that would distort the page and confuse the translator, keeping their text:
    # <small> (and class="small" spans) actually shrink the rendered text, spans/anchors are noise. The math and
    # emphasis tags (strong/em/sub/sup) are kept.
    body = re.sub(r"</?small\b[^>]*>", "", body, flags=re.I)
    body = re.sub(r"<span\b[^>]*>|</span>", "", body, flags=re.I)
    body = re.sub(r"<a\b[^>]*>|</a>", "", body, flags=re.I)
    body = re.sub(r"[ \t]+", " ", body)
    body = re.sub(r"\n{3,}", "\n\n", body).strip()
    return body, imgs, counter


PART_STEM = re.compile(r"^part\d*$", re.I)
EPUB_SKIP_STEM = re.compile(r"^(cover|titlepage|halftitle|title|copyright|toc|nav|ncx|index|bibliography)\d*$", re.I)


def extract_epub(source, work, keep_images, page_size):
    """Split an EPUB into sections from its OPF spine (order) and nav/ncx (titles), keeping the XHTML formatting
    that PDF extraction destroys (bold vectors, sub/superscripts, prose emphasis). Part-divider files are folded
    into the head of the next chapter. Writes the same work-dir layout as the PDF path, with source_kind=epub."""
    z = zipfile.ZipFile(source)
    cont = ET.fromstring(z.read("META-INF/container.xml"))
    opf_path = next(el.get("full-path") for el in cont.iter() if _ln(el.tag) == "rootfile")
    opf_dir = posixpath.dirname(opf_path)
    opf = ET.fromstring(z.read(opf_path))
    resolve = lambda href: posixpath.normpath(posixpath.join(opf_dir, href))
    manifest, cover_id, title, author, nav_href, ncx_href = {}, None, source.stem, "", None, None
    for el in opf.iter():
        ln, a = _ln(el.tag), el.attrib
        if ln == "item":
            manifest[a["id"]] = {"href": resolve(a["href"]), "media": a.get("media-type", ""), "props": a.get("properties", "")}
            if "nav" in a.get("properties", ""):
                nav_href = resolve(a["href"])
            if "cover-image" in a.get("properties", ""):  # EPUB3 cover
                cover_id = a["id"]
            if a.get("media-type") == "application/x-dtbncx+xml":
                ncx_href = resolve(a["href"])
        elif ln == "meta" and a.get("name") == "cover":  # EPUB2 cover
            cover_id = a.get("content")
        elif ln == "title" and (el.text or "").strip() and title == source.stem:
            title = el.text.strip()
        elif ln == "creator" and (el.text or "").strip() and not author:
            author = el.text.strip()
    spine = [el.get("idref") for el in opf.iter() if _ln(el.tag) == "itemref" and el.get("idref") in manifest]
    titles = epub_titles(z, nav_href, ncx_href)

    (work / "text").mkdir(exist_ok=True)
    if keep_images:
        (work / "images").mkdir(exist_ok=True)
    sections, images, pending_prefix, seen_chapter, counter, idx = [], {}, "", False, 0, 0
    cur = None  # the chapter/section being accumulated; subsections and trailing notes append to it

    def words_of(frag):
        return len(re.sub(r"<[^>]+>", " ", IMG_TOKEN_RE.sub(" ", frag)).split())

    def flush():
        nonlocal cur, idx
        if cur is None:
            return
        idx += 1
        sid = f"{idx:02d}"
        sfile = f"text/{sid}-{slugify(cur['title'])}.xhtml"
        (work / sfile).write_text(cur["frag"])
        sections.append({"id": sid, "title": cur["title"], "outline_title": cur["outline_title"],
                         "chapter": cur["chapter"], "label": cur["label"], "kind": cur["kind"],
                         "start": 0, "end": 0, "file": sfile, "words": words_of(cur["frag"])})
        cur = None

    for idref in spine:
        item = manifest[idref]
        if not item["media"].startswith("application/xhtml"):
            continue
        href = item["href"]
        stem = Path(posixpath.basename(href)).stem
        stitle = titles.get(posixpath.basename(href)) or epub_first_heading(z, href) or stem
        if re.fullmatch(r"[ivxlcdm]+|\d+", stitle.strip(), re.I):  # a bare page number/roman: a series-ad or filler page
            continue
        chapter, clean = parse_title(stitle)
        is_subsection = bool(re.match(r"^\s*\d+(?:\.\d+)+", stitle))  # 1.1, 2.10 … a subsection, not a chapter
        is_part = bool(PART_STEM.match(stem)) or bool(re.match(r"^\s*part\b", stitle, re.I))
        if EPUB_SKIP_STEM.match(stem):
            continue
        if chapter is None and not is_part and PART_TITLE_RE.match(stitle):
            # First chapter of a part: this opener file's nav title is the PART name, so the chapter number and
            # title live in its headings ("CHAPTER 3" / "Solving Problems by Searching"). See translate-roman parts.
            hnum, htitle = chapter_from_headings(epub_headings(z, href))
            if hnum is not None:
                chapter, clean, is_subsection = hnum, htitle, False
        frag, sec_imgs, counter = epub_fragment(z, href, work, keep_images, counter)
        if is_part and chapter is None:  # a part divider folds into the head of the next chapter
            pending_prefix += frag + "\n\n"
            images.update(sec_imgs)
            continue
        kind = classify(stitle, chapter, seen_chapter)
        if kind in ("cover", "contents", "skip"):
            continue
        # A subsection (1.1 …) or a chapter's trailing notes (its Bibliographical Remarks etc.) belong under the
        # current chapter, becoming ## sub-headings, so a per-subsection-file EPUB groups into one section per
        # chapter like a per-chapter-file one.
        if cur is not None and (is_subsection or kind == "back"):
            cur["frag"] += "\n\n" + frag
            images.update(sec_imgs)
            continue
        flush()  # a new top-level section starts: emit the accumulated one
        if kind == "chapter":
            seen_chapter = True
        images.update(sec_imgs)
        body_frag = (pending_prefix + frag) if kind == "chapter" and pending_prefix else frag
        pending_prefix = "" if kind == "chapter" else pending_prefix
        cur = {"title": clean, "outline_title": stitle, "chapter": chapter,
               "label": f"第{chinese_number(chapter)}章" if chapter else None, "kind": kind, "frag": body_frag}
    flush()

    nums = [s["chapter"] for s in sections if s.get("chapter")]
    if nums:  # a gap or duplicate means an opener was not recognised — the general safety net for a new EPUB layout
        gaps = [n for n in range(min(nums), max(nums) + 1) if n not in nums]
        dupes = sorted({n for n in nums if nums.count(n) > 1})
        if gaps:
            print(f"warn: chapter numbers skip {gaps} — an opener may be mislabelled", file=sys.stderr)
        if dupes:
            print(f"warn: chapter numbers repeat {dupes} — subsections may not be grouping", file=sys.stderr)

    cover_file = None
    if cover_id and cover_id in manifest:
        ch = manifest[cover_id]
        img_href = ch["href"]
        if not ch["media"].startswith("image"):
            craw = z.read(ch["href"]).decode("utf-8", "replace")
            mm = re.search(r'<img[^>]+src=["\']([^"\']+)["\']', craw)
            img_href = posixpath.normpath(posixpath.join(posixpath.dirname(ch["href"]), mm.group(1))) if mm else None
        if img_href:
            try:
                ext = Path(img_href).suffix or ".jpg"
                (work / f"cover{ext}").write_bytes(z.read(img_href))
                cover_file = f"cover{ext}"
            except KeyError:
                cover_file = None

    meta = {"source": str(source), "source_kind": "epub", "title": title, "author": author,
            "page_size": page_size, "cover_image": cover_file, "sections": sections}
    (work / "sections.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    if keep_images:
        (work / "images.json").write_text(json.dumps(images, ensure_ascii=False, indent=1))
        print(f"kept {len(images)} images in {work / 'images'}", file=sys.stderr)
    for s in sections:
        print(f"{s['id']} {s['kind']:8} {s['outline_title']}: {s['words']} words -> {s['file']}")
    print(f"cover: {cover_file}; page size: {page_size[0]}x{page_size[1]}pt", file=sys.stderr)
    print(f"work dir: {work}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("book")
    ap.add_argument("--work")
    ap.add_argument("--sections", help="hand-written sections.json ([{title,start}] or full) to use instead of the outline")
    ap.add_argument("--keep-images", action="store_true",
                    help="keep figures and equations that are stored as images: extract each section's text with "
                         "⟦IMG:key⟧ placeholders at the image positions and copy the images into <work>/images")
    ap.add_argument("--page-size", default="468x680",
                    help="EPUB only: rendered page size in pt as WxH (default 468x680, a 6.5x9.4in trade book)")
    args = ap.parse_args()
    source = Path(args.book).resolve()
    if args.work:
        work = Path(args.work).resolve()
        work.mkdir(parents=True, exist_ok=True)
    else:
        work, _ = default_work(source)

    if source.suffix.lower() == ".epub":
        page_size = [round(float(x), 2) for x in args.page_size.lower().split("x")]
        extract_epub(source, work, args.keep_images, page_size)
        return

    book = source
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
    images = {}
    for s in sections:
        if s["kind"] in ("cover", "contents", "skip"):
            continue
        heads = [s["title"], s["outline_title"], short_title, title]
        if s["chapter"]:
            heads += [f"chapter {s['chapter']}"] + [f"chapter {w}" for w, n in NUMBER_WORDS.items() if n == s["chapter"]]
        if args.keep_images:
            pages, section_images = image_pages(book, s["start"], s["end"], work / "images")
            images.update(section_images)
        else:
            pages = pdftotext_pages(book, s["start"], s["end"])
        paragraphs = clean_pages(pages, heads)
        s["file"] = f"text/{s['id']}-{slugify(s['title'])}.txt"
        s["words"] = sum(len(p.split()) for p in paragraphs if not IMG_TOKEN_RE.fullmatch(p.strip()))
        (work / s["file"]).write_text("\n\n".join(paragraphs) + "\n")

    if args.keep_images:
        (work / "images.json").write_text(json.dumps(images, ensure_ascii=False, indent=1))
        print(f"kept {len(images)} images in {work / 'images'}", file=sys.stderr)

    meta = {"book": str(book), "source": str(source), "title": title, "author": author, "pages": page_count,
            "page_size": page_size, "sections": sections}
    (work / "sections.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    for s in sections:
        extra = f"{s['words']} words -> {s['file']}" if "file" in s else "(not translated)"
        print(f"{s['id']} {s['kind']:8} p{s['start']}-{s['end']:<4} {s['outline_title']}: {extra}")
    print(f"work dir: {work}")


if __name__ == "__main__":
    main()
