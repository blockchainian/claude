#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Splits a PDF into chapters (its bookmarks) or pages for per-chapter highlights, and typesets the
# ABOUTME: written highlights into a PDF in the translate skill's book format at the source's page size.
#
# Usage: pdf_highlights.py split <file.pdf | url> [--by page]
#        pdf_highlights.py render <work dir> [--out <highlights.pdf>] [--bg iterm|#rrggbb] [--fg #rrggbb|iterm] [--font-size 9.25]
# split writes <work>/chapters.json and one text file per chapter, and prints the JSON. render typesets every
# chapter whose highlights Markdown exists to <source>-highlights.pdf and writes the store-ready <work>/draft.md.
import argparse
import html
import importlib.util
import json
import re
import sys
import tempfile
from pathlib import Path

import pikepdf

HERE = Path(__file__).parent


def load(path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


fs = load(HERE / "fetch_source.py")
rd = load(HERE.parent.parent / "translate" / "scripts" / "render.py")  # the book format: css, Chrome print, background

MARKER_RE = re.compile(r"⟦[^⟧]*⟧")  # the invisible page markers a translate-built PDF carries
# A scanned (image-only) PDF extracts to nothing or a few stray characters, while a real one-page note can be
# under 200: refuse only below this total.
MIN_CHARS = 50


def bookmark_page(pdf, item, index):
    """The zero-based page a bookmark points at, or None when it cannot be resolved."""
    try:
        dest = item.destination
        if dest is None and item.action is not None:
            dest = item.action.get("/D")
        if isinstance(dest, (pikepdf.String, pikepdf.Name)):  # a named destination
            if isinstance(dest, pikepdf.Name):
                dest = pdf.Root.Dests[str(dest)]
            else:
                dest = pikepdf.NameTree(pdf.Root.Names.Dests)[str(dest)]
        if isinstance(dest, pikepdf.Dictionary):
            dest = dest.get("/D")
        return index.get(dest[0].objgen)
    except Exception:
        return None


def bookmarks(pdf):
    """(title, zero-based page) for the PDF's chapter-level bookmarks, in page order, one per page. When the
    top level holds fewer than three (e.g. only "Part I / Part II"), their children are chapters too."""
    index = {p.objgen: i for i, p in enumerate(pdf.pages)}
    with pdf.open_outline() as outline:
        items = list(outline.root)
        if len(items) < 3:
            items = [x for it in items for x in [it, *it.children]]
        found = [(str(it.title).strip(), bookmark_page(pdf, it, index)) for it in items]
    seen, out = set(), []
    for title, page in sorted((f for f in found if f[1] is not None), key=lambda f: f[1]):
        if page not in seen:
            seen.add(page)
            out.append((title, page))
    return out


def split(source, by):
    """Split the PDF into chapters (bookmarks) or pages; writes chapters.json + text/<id>.txt, returns the meta."""
    slug = fs.slugify(source)
    work = fs.WORK / slug
    (work / "text").mkdir(parents=True, exist_ok=True)
    local = Path(source).expanduser()
    pdf_path = local.resolve() if local.is_file() else fs.fetch_bytes(source, work / "source.pdf")
    texts = [MARKER_RE.sub("", t).strip() for t in rd.page_texts(pdf_path)]
    pdf = pikepdf.open(pdf_path)
    texts = (texts + [""] * len(pdf.pages))[:len(pdf.pages)]
    if sum(len(re.sub(r"\s", "", t)) for t in texts) < MIN_CHARS:
        sys.exit(f"{pdf_path.name} has almost no text: it is image-only (scanned) and there is no OCR here")

    marks = [] if by == "page" else bookmarks(pdf)
    unit = "chapter" if len(marks) > 1 else "page"
    if unit == "page":
        marks = [(f"Page {i + 1}", i) for i in range(len(texts))]
    elif marks[0][1] > 0:
        marks.insert(0, ("Front matter", 0))

    chapters = []
    for n, (title, first) in enumerate(marks):
        last = marks[n + 1][1] - 1 if n + 1 < len(marks) else len(texts) - 1
        text = "\n\n".join(t for t in texts[first:last + 1] if t)
        cid = f"{n + 1:0{max(2, len(str(len(marks))))}d}"
        (work / "text" / f"{cid}.txt").write_text(text + "\n", encoding="utf-8")
        chapters.append({"id": cid, "title": title, "pages": [first + 1, last + 1],
                         "chars": len(re.sub(r"\s", "", text)), "text": f"text/{cid}.txt", "highlights": f"md/{cid}.md"})

    box = [float(v) for v in pdf.pages[0].mediabox]
    meta = {"title": str(pdf.docinfo.get("/Title", "")).strip() or pdf_path.stem, "slug": slug,
            "source": str(pdf_path) if local.is_file() else source, "pdf": str(pdf_path), "work": str(work),
            "unit": unit, "page_size": [round(box[2] - box[0], 2), round(box[3] - box[1], 2)], "chapters": chapters}
    (work / "chapters.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2), encoding="utf-8")
    return meta


def default_out(pdf_path, work):
    """<source>-highlights.pdf next to the source, or in the work dir when the source's folder is not writable
    (or the source was downloaded into the work dir)."""
    target = pdf_path.with_name(pdf_path.stem + "-highlights.pdf")
    try:
        probe = target.parent / ".digest-write-test"
        probe.touch()
        probe.unlink()
        return target
    except OSError:
        return work / target.name


def write_draft(work, meta, ready):
    """The whole book's highlights as one store-ready draft: each chapter a '## ' section, its own headings
    one level down."""
    parts = ["---", f"title: {meta['title']}", f"url: {meta['source']}", f"slug: {meta['slug']}", "---", "",
             f"# {meta['title']}", ""]
    for title, md_text in ready:
        lines = md_text.strip().splitlines()
        if lines and lines[0].startswith("# "):
            lines = lines[1:]
        body = re.sub(r"^(#+) ", r"#\1 ", "\n".join(lines).strip(), flags=re.M)
        parts += [f"## {title}", "", body, ""]
    (work / "draft.md").write_text("\n".join(parts), encoding="utf-8")


def render(work, opt):
    meta = json.loads((work / "chapters.json").read_text(encoding="utf-8"))
    bg, fg = rd.page_colors(opt)
    sections, drafts = [], []
    for c in meta["chapters"]:
        md_path = work / c["highlights"]
        if not md_path.exists():
            continue
        md_text = md_path.read_text(encoding="utf-8")
        title, body = rd.md_to_html(md_text)
        sections.append(({"id": c["id"], "kind": "chapter"}, title or c["title"], body))
        drafts.append((title or c["title"], md_text))
    if not sections:
        sys.exit(f"nothing to render: no highlights in {work / 'md'}")

    style = rd.css(meta["page_size"], bg, fg, [(s["id"], t, s["kind"]) for s, t, _ in sections],
                   opt.font_size, rd.brighten(fg, opt.bold_factor))
    tmp = Path(tempfile.mkdtemp(prefix="highlights-"))
    html_path, typeset = tmp / "highlights.html", tmp / "highlights.pdf"
    html_path.write_text(
        f'<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>{html.escape(meta["title"])}</title>'
        f'{rd.KATEX_HEAD}<style>{style}</style></head><body>{"".join(rd.section_html(*s) for s in sections)}</body></html>',
        encoding="utf-8")
    rd.print_pdf(rd.chrome_binary(), html_path, typeset)
    openers = rd.page_map(typeset)
    missing = [s["id"] for s, _, _ in sections if f"S{s['id']}" not in openers]
    if missing:
        sys.exit(f"markers not found for chapters {missing}; the render is broken")

    pdf = pikepdf.open(typeset)
    opener_pages = set(openers.values())
    for i, page in enumerate(pdf.pages):
        rd.paint_background(pdf, page, bg, meta["page_size"][1] * 0.082 if i in opener_pages else 0)
    with pdf.open_outline() as outline:
        for s, title, _ in sections:
            outline.root.append(pikepdf.OutlineItem(title, openers[f"S{s['id']}"]))
    pdf.docinfo["/Title"] = meta["title"]
    out = Path(opt.out) if opt.out else default_out(Path(meta["pdf"]), work)
    pdf.save(out)
    write_draft(work, meta, drafts)
    print(f"{out} ({len(pdf.pages)} pages, {len(sections)} of {len(meta['chapters'])} {meta['unit']}s)")
    print(f"draft: {work / 'draft.md'}")
    rd.report_katex_errors(rd.katex_errors(typeset, openers))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sp = sub.add_parser("split")
    sp.add_argument("source", help="a PDF file path or URL")
    sp.add_argument("--by", choices=["page"], help="one section per page even when the PDF has bookmarks")
    rp = sub.add_parser("render")
    rp.add_argument("work")
    rp.add_argument("--out")
    rd.add_style_args(rp)
    opt = ap.parse_args()
    if opt.cmd == "split":
        print(json.dumps(split(opt.source, opt.by), ensure_ascii=False, indent=2))
    else:
        render(Path(opt.work).resolve(), opt)


if __name__ == "__main__":
    main()
