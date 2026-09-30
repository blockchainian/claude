#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
# ABOUTME: Splits an EPUB book into sections from its OPF spine (order) and nav/ncx (titles), keeping each
# ABOUTME: section's XHTML formatting (bold, emphasis, sub/superscripts, images) for translation, into a work directory.
#
# Usage: extract.py <book.epub> [--work <dir>] [--keep-images] [--page-size WxH]
# Writes <work>/sections.json and <work>/text/<id>-<slug>.xhtml; prints one line per section.
import argparse
import json
import posixpath
import re
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

IMG_TOKEN = "⟦IMG:{}⟧"  # placeholder for a figure/equation stored as an image, kept verbatim through translation
IMG_TOKEN_RE = re.compile(r"⟦IMG:[^⟧]+⟧")

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
    if t in ("index", "notes", "endnotes", "references", "bibliography", "works cited", "further reading",
             "copyright", "title page", "half title", "also by", "about the author"):
        return "skip"
    if chapter is not None:
        return "chapter"
    return "back" if seen_chapter else "front"


PART_TITLE = re.compile(r"^\s*part\b", re.I)


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
# A book's terminal back-matter opens with one of these; once past the last chapter it and every spine file
# after it (continuations that carry no nav title of their own included) are skipped rather than translated.
BACKMATTER_START = re.compile(r"^(notes|endnotes|references|bibliography|works cited|further reading|index)\b", re.I)
# A conclusion-like closing section is real content: it opens its own section rather than folding into the last chapter.
OWN_BACK_SECTION = re.compile(r"^(conclusion|epilogue|afterword|postscript|coda)\b", re.I)


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

    def chapter_at(idref):  # the chapter number this spine file opens, or None. Must track the main loop's
        # chapter resolution below (parse_title + part-opener headings); if that changes, change this too, or
        # last_chapter_pos drifts and the back-matter latch arms on the wrong file.
        item = manifest[idref]
        if not item["media"].startswith("application/xhtml"):
            return None
        base = posixpath.basename(item["href"])
        stitle = titles.get(base) or epub_first_heading(z, item["href"]) or Path(base).stem
        chap, _ = parse_title(stitle)
        if chap is None and not (PART_STEM.match(Path(base).stem) or PART_TITLE.match(stitle)) and PART_TITLE_RE.match(stitle):
            chap = chapter_from_headings(epub_headings(z, item["href"]))[0]
        return chap

    # Arm the back-matter latch only past the last chapter, so a per-chapter "Notes" between chapters still passes.
    last_chapter_pos = max((i for i, idref in enumerate(spine) if chapter_at(idref) is not None), default=-1)
    in_backmatter = False

    for pos, idref in enumerate(spine):
        item = manifest[idref]
        if not item["media"].startswith("application/xhtml"):
            continue
        if in_backmatter:  # the book's terminal back-matter has started — skip it and everything after
            continue
        href = item["href"]
        stem = Path(posixpath.basename(href)).stem
        stitle = titles.get(posixpath.basename(href)) or epub_first_heading(z, href) or stem
        if pos > last_chapter_pos and BACKMATTER_START.match(stitle.strip()):
            in_backmatter = True
            continue
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
        # chapter like a per-chapter-file one. A Conclusion/Epilogue/Afterword is substantial standalone content,
        # so it opens its own section instead — its own untitled continuation files then append to it.
        is_own_back = kind == "back" and bool(OWN_BACK_SECTION.match(clean or stitle))
        if cur is not None and (is_subsection or (kind == "back" and not is_own_back)):
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
    ap.add_argument("--keep-images", action="store_true",
                    help="keep figures and equations that are stored as images: extract each section's text with "
                         "⟦IMG:key⟧ placeholders at the image positions and copy the images into <work>/images")
    ap.add_argument("--page-size", default="468x680",
                    help="rendered page size in pt as WxH (default 468x680, a 6.5x9.4in trade book)")
    args = ap.parse_args()
    source = Path(args.book).resolve()
    if source.suffix.lower() != ".epub":
        print(f"extract.py takes an EPUB; got {source.name}. Download the book's EPUB with the download-book skill "
              f"and pass that.", file=sys.stderr)
        sys.exit(2)
    if args.work:
        work = Path(args.work).resolve()
        work.mkdir(parents=True, exist_ok=True)
    else:
        work, _ = default_work(source)

    page_size = [round(float(x), 2) for x in args.page_size.lower().split("x")]
    extract_epub(source, work, args.keep_images, page_size)


if __name__ == "__main__":
    main()
