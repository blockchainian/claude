#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Typesets the translated Markdown sections into a PDF in the source book's format (page size, colors,
# ABOUTME: running heads, folios, contents page) with headless Chrome, then adds the original cover and bookmarks.
#
# Usage: render.py <work dir> [--out <book-zh.pdf>] [--only 04] [--title <中文书名>] [--bg iterm|source|#rrggbb --fg #606e6a|iterm|source] [--font-size 9.25]
# Without --only: the whole book (cover + 目录 + every translated section) to --out (default <book>-zh.pdf next
# to the source). With --only: one section to <work>/pdf/<id>-<slug>.pdf for a quick look, no cover or contents.
import argparse
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

import markdown
import pikepdf
from PIL import Image

ROMAN = ["", "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv", "xvi",
         "xvii", "xviii", "xix", "xx"]
MARK_TOC = "⟦TOC⟧"

# KaTeX typesets the inline \(..\) / display \[..\] LaTeX in the browser before Chrome prints (see print_pdf's
# --virtual-time-budget). Loaded from the CDN; headless Chrome has network access.
KATEX_HEAD = (
    '<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">'
    '<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>'
    '<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js" '
    'onload="renderMathInElement(document.body,{delimiters:['
    "{left:'\\\\[',right:'\\\\]',display:true},{left:'\\\\(',right:'\\\\)',display:false}"
    '],throwOnError:false});"></script>'
)


def brighten(hexcolor, factor):
    """A #rrggbb color scaled per-channel and clamped: used to make bold ink a step brighter than the body."""
    return "#%02x%02x%02x" % tuple(min(255, round(c * factor)) for c in _hex(hexcolor))


def chrome_binary():
    for c in [os.environ.get("CHROME"), "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
              shutil.which("google-chrome"), shutil.which("chromium"), shutil.which("chromium-browser")]:
        if c and Path(c).exists():
            return c
    sys.exit("Chrome not found: set CHROME=/path/to/chrome")


def sample_colors(book, page):
    """Background = the most common color of a body page; text = the most common color far from it."""
    with tempfile.TemporaryDirectory() as d:
        subprocess.run(["pdftoppm", "-r", "30", "-f", str(page), "-l", str(page), "-png", str(book), f"{d}/p"], check=True)
        png = next(Path(d).glob("p*.png"))
        im = Image.open(png).convert("RGB")
        counts = Counter(zip(*(im.getchannel(i).tobytes() for i in range(3))))
    bg = counts.most_common(1)[0][0]
    lum = lambda c: 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
    far = [(c, n) for c, n in counts.items() if abs(lum(c) - lum(bg)) > 100]
    fg = max(far, key=lambda x: x[1])[0] if far else ((20, 20, 20) if lum(bg) > 128 else (225, 221, 213))
    return "#%02x%02x%02x" % bg, "#%02x%02x%02x" % fg


def iterm_colors():
    """The iTerm2 default profile's dark-mode background and foreground, as #rrggbb; a near-black page and the
    default text gray when iTerm2 is not installed."""
    import plistlib
    plist = Path.home() / "Library/Preferences/com.googlecode.iterm2.plist"
    if not plist.exists():
        return "#000409", "#606e6a"
    prefs = plistlib.load(plist.open("rb"))
    guid = prefs.get("Default Bookmark Guid")
    profile = next((b for b in prefs.get("New Bookmarks", []) if b.get("Guid") == guid), None) or prefs["New Bookmarks"][0]
    def hx(key):
        c = profile.get(key + " (Dark)") if profile.get("Use Separate Colors for Light and Dark Mode") else None
        c = c or profile[key]
        return "#%02x%02x%02x" % tuple(round(c[k] * 255) for k in ("Red Component", "Green Component", "Blue Component"))
    return hx("Background Color"), hx("Foreground Color")


MATH_RE = re.compile(r"\\\[.*?\\\]|\\\(.*?\\\)", re.S)


def md_to_html(md_text):
    """Section Markdown -> body HTML: the first '# ' line is the title (returned separately). Inline LaTeX
    (\\(..\\), \\[..\\]) is stashed before Markdown runs, because Markdown would treat the _ and * inside it as
    emphasis and corrupt it; the raw math is restored (HTML-escaped) for KaTeX to typeset in the browser."""
    lines = md_text.strip().splitlines()
    title = lines[0][2:].strip() if lines and lines[0].startswith("# ") else ""
    body = "\n".join(lines[1:] if title else lines)
    body = re.sub(r"^# ", "## ", body, flags=re.M)
    vault = []

    def stash(m):
        vault.append(m.group(0))
        return f"@@MATH{len(vault) - 1}@@"

    body = MATH_RE.sub(stash, body)
    out = markdown.markdown(body, extensions=["smarty"], output_format="html")
    for i, raw in enumerate(vault):
        out = out.replace(f"@@MATH{i}@@", html.escape(raw))
    return title, out


IMG_TOKEN_RE = re.compile(r"⟦IMG:([^⟧]+)⟧")


def is_line_art(im):
    """True when an image is black-on-white line art (an equation or a line diagram) rather than a colour
    figure: a downsampled copy has almost no saturated pixels."""
    from PIL import Image
    small = im.convert("RGB")
    small.thumbnail((64, 64))
    sat = small.convert("HSV").getchannel("S").getdata()
    px = list(sat)
    return sum(1 for s in px if s > 40) / max(1, len(px)) < 0.03


def _hex(c):
    return (int(c[1:3], 16), int(c[3:5], 16), int(c[5:7], 16))


def recolor_line_art(src, fg, bg, dest):
    """Recolour black-on-white line art to foreground ink on an OPAQUE page-coloured background, so an equation
    blends into the dark page in the body colour with no white plate. Opaque (no alpha) is deliberate: a
    transparent plate renders inconsistently across PDF viewers — some do not composite it over the page and the
    ink disappears. The grey is upscaled (Lanczos) and sharpened first so low-resolution equations degrade
    gracefully when zoomed. Returns True when it was line art (and dest written), False for a colour figure."""
    from PIL import Image, ImageFilter
    im = Image.open(src)
    if not is_line_art(im):
        return False
    grey = im.convert("L")
    # Upscale small equations (which need it) but not large diagrams (which would bloat the file); cap the size.
    if max(grey.size) < 700:
        grey = grey.resize((grey.width * 2, grey.height * 2), Image.LANCZOS)
    if max(grey.size) > 1600:
        grey.thumbnail((1600, 1600), Image.LANCZOS)
    grey = grey.filter(ImageFilter.UnsharpMask(radius=2, percent=130, threshold=2))
    ink = grey.point(lambda v: 255 - v)  # dark ink -> full weight, white paper -> none
    out = Image.composite(Image.new("RGB", grey.size, _hex(fg)), Image.new("RGB", grey.size, _hex(bg)), ink)
    out.save(dest, optimize=True)
    return True


def place_images(body_html, work, images, fg, bg, eq_scale=None):
    """Replace ⟦IMG:key⟧ placeholders with the extracted images. Line art (equations, diagrams) is recoloured
    to foreground ink on the opaque page colour so it blends into the dark page; a colour figure keeps a white
    plate (inverting a photo would ruin it). Block images become their own centred figure; inline ones sit in
    the line. With eq_scale set (EPUB build), every block figure is sized to its intrinsic width times eq_scale,
    so equations render at one consistent scale instead of at each raw crop's pixel size."""
    def repl(m):
        meta = images.get(m.group(1))
        if not meta:
            return ""
        src = work / "images" / meta["file"]
        rc = src.with_suffix(".rc.png")
        try:
            line = recolor_line_art(src, fg, bg, rc)
        except Exception:
            line = False
        uri = (rc if line else src).resolve().as_uri()
        plate = "" if line else " plate"
        if meta.get("block"):
            style = ""
            if eq_scale:
                try:
                    style = f' style="width:{Image.open(src).size[0] * eq_scale:.1f}pt;max-width:100%"'
                except Exception:
                    style = ""
            return f'</p><figure class="fig{plate}"><img{style} src="{uri}"></figure><p>'
        return f'<img class="infig{plate}" src="{uri}">'
    return IMG_TOKEN_RE.sub(repl, body_html)


def css(page_size, bg, fg, heads, font_size=9.25, bold=None):
    w, h = page_size
    bold = bold or fg
    # Chinese bold reads as bold only in a gothic (黑体) face; Songti's bold is nearly indistinguishable.
    hei = 'Baskerville, "PingFang SC", "Heiti SC", "Hiragino Sans GB", sans-serif'
    top, side, bottom = round(h * 0.082, 1), round(w * 0.135, 1), round(h * 0.068, 1)
    margin_font = 'font-family: Baskerville, "Songti SC", serif;'
    folio = lambda kind: "counter(page, lower-roman)" if kind == "front" else "counter(page)"
    named = "\n".join(
        f'@page s{sid} {{ @top-center {{ content: "{html.escape(t)}"; font-size: 7pt; letter-spacing: 2pt; color: {fg}; {margin_font} }}'
        f' @bottom-center {{ content: {folio(kind)}; font-size: 8pt; color: {fg}; {margin_font} }} }}'
        for sid, t, kind in heads)
    return f"""
:root {{ --bg: {bg}; --fg: {fg}; }}
@page {{ size: {w}pt {h}pt; margin: {top}pt {side}pt {bottom}pt; background: var(--bg);
        @bottom-center {{ content: counter(page, lower-roman); font-size: 8pt; color: {fg}; {margin_font} }} }}
{named}
html {{ background: var(--bg); }}
body {{ margin: 0; color: var(--fg); font-family: Baskerville, "Songti SC", serif; font-size: {font_size}pt; line-height: 1.8; }}
strong, b {{ font-family: {hei}; font-weight: 700; color: {bold}; }}
.katex {{ color: {fg}; font-size: 1em; }}
section {{ break-before: page; }}
.opener {{ padding-top: {round(h * 0.2)}pt; text-align: center; margin-bottom: {round(h * 0.07)}pt; }}
.opener .label {{ font-family: {hei}; font-weight: 700; color: {bold}; font-size: 9pt; letter-spacing: 3pt; margin-bottom: 14pt; }}
.opener h1 {{ font-family: {hei}; font-weight: 700; color: {bold}; font-size: 22pt; letter-spacing: 2pt; margin: 0; line-height: 1.5; }}
.mk {{ font-size: 1pt; color: transparent; letter-spacing: 0; white-space: nowrap; font-family: Baskerville, "Songti SC", serif; }}
p {{ margin: 0; text-indent: 2em; text-align: justify; }}
.body-text > p:first-of-type {{ text-indent: 0; }}
.body-text > p:first-of-type::first-letter {{ float: left; font-size: 2.6em; line-height: 0.85; padding: 3pt 4pt 0 0; }}
h2 {{ font-family: {hei}; font-size: 11pt; font-weight: 700; color: {bold}; margin: 18pt 0 6pt; break-after: avoid; }}
h3 {{ font-family: {hei}; font-size: 10.5pt; font-weight: 700; color: {bold}; margin: 12pt 0 4pt; break-after: avoid; }}
blockquote {{ margin: 8pt 2em; font-style: italic; }}
blockquote p {{ text-indent: 0; }}
ul, ol {{ margin: 4pt 0 4pt 2em; padding: 0; }}
li {{ margin: 2pt 0; }}
em {{ font-style: italic; }}
.fig {{ margin: 10pt auto; text-align: center; break-inside: avoid; }}
.fig img {{ max-width: 100%; height: auto; }}
.fig.plate img {{ background: #fff; padding: 4pt 6pt; border-radius: 3pt; }}
.infig {{ max-height: 1.4em; width: auto; vertical-align: middle; }}
.infig.plate {{ background: #fff; padding: 0 2pt; border-radius: 2pt; }}
.contents .opener {{ margin-bottom: {round(h * 0.05)}pt; }}
.toc {{ font-size: 9.5pt; }}
.toc .e {{ display: flex; align-items: baseline; margin: 0 0 9pt; }}
.toc .lbl {{ width: 6.5em; letter-spacing: 1pt; font-size: 8.5pt; flex: none; }}
.toc .t {{ flex: 1; }}
.toc .pg {{ margin-left: 1em; flex: none; }}
.toc .front .lbl, .toc .back .lbl {{ visibility: hidden; }}
"""


def section_html(s, title, body):
    cls = ["body" if s["kind"] != "front" else "front"]
    label = f'<div class="label">{html.escape(s["label"])}</div>' if s.get("label") else ""
    return (f'<section class="{" ".join(cls)}" style="page: s{s["id"]}" id="s{s["id"]}">'
            f'<div class="opener">{label}<h1>{html.escape(title)}<span class="mk">⟦S{s["id"]}⟧</span></h1></div>'
            f'<div class="body-text">{body}</div></section>')


def contents_html(entries, folios):
    rows = []
    for s, title in entries:
        pg = folios.get(s["id"], "000")
        rows.append(f'<div class="e {s["kind"]}"><span class="lbl">{html.escape(s.get("label") or "")}</span>'
                    f'<span class="t">{html.escape(title)}<span class="mk">⟦T{s["id"]}⟧</span></span><span class="pg">{pg}</span></div>')
    return (f'<section class="contents front" id="toc"><div class="opener"><h1>目录<span class="mk">{MARK_TOC}</span></h1></div>'
            f'<div class="toc">{"".join(rows)}</div></section>')


def contents_rows(pdf_path):
    """Contents-row marker id -> (zero-based page, y center in PDF points from the top, page width, page height)."""
    xml = subprocess.run(["pdftotext", "-bbox-layout", str(pdf_path), "-"], capture_output=True, text=True, check=True).stdout
    rows = {}
    for i, page in enumerate(re.findall(r"<page (.*?)</page>", xml, re.S)):
        head = re.match(r'width="([\d.]+)" height="([\d.]+)"', page)
        for m in re.finditer(r'<word xMin="[\d.]+" yMin="([\d.]+)" xMax="[\d.]+" yMax="([\d.]+)">⟦\s*T\s*([\d\s]+)\s*⟧</word>', page):
            rows[re.sub(r"\s+", "", m.group(3))] = (i, (float(m.group(1)) + float(m.group(2))) / 2, float(head.group(1)), float(head.group(2)))
    return rows


def link_contents(pdf, front_pdf, offset, pages, ready, row_height):
    """A Link annotation over every 目录 row, jumping to that section's opener page."""
    rows = contents_rows(front_pdf)
    for s, _, _ in ready:
        if s["id"] not in rows:
            continue
        page_index, y, w, h = rows[s["id"]]
        page = pdf.pages[page_index + offset]
        target = pdf.pages[pages[f"S{s['id']}"] + offset]
        rect = [w * 0.1, h - y - row_height / 2, w * 0.9, h - y + row_height / 2]
        annot = pikepdf.Dictionary(Type=pikepdf.Name.Annot, Subtype=pikepdf.Name.Link, Rect=rect, Border=[0, 0, 0],
                                   Dest=[target.obj, pikepdf.Name.Fit])
        if "/Annots" not in page:
            page.Annots = pikepdf.Array()
        page.Annots.append(pdf.make_indirect(annot))


def page_texts(pdf_path):
    out = subprocess.run(["pdftotext", "-layout", str(pdf_path), "-"], capture_output=True, text=True, check=True).stdout
    return out.split("\f")


def page_map(pdf_path):
    """Marker -> zero-based page index."""
    found = {}
    for i, text in enumerate(page_texts(pdf_path)):
        for m in re.findall(r"⟦\s*((?:S\s*[\d\s]+)|(?:T\s*O\s*C))\s*⟧", text):
            found.setdefault(re.sub(r"\s+", "", m), i)
    return found


def roman_folio(page_index):
    return ROMAN[page_index + 1] if page_index + 1 < len(ROMAN) else str(page_index + 1)


def folio_for(page_index, first_body_index):
    if first_body_index is None or page_index < first_body_index:
        return roman_folio(page_index)
    return str(page_index - first_body_index + 1)


def print_pdf(chrome, html_path, pdf_path):
    # --virtual-time-budget lets KaTeX (loaded from the CDN) finish typesetting the math before the snapshot;
    # without it the page prints with raw \(...\) source.
    subprocess.run([chrome, "--headless=new", "--disable-gpu", "--no-pdf-header-footer",
                    "--virtual-time-budget=15000",
                    f"--print-to-pdf={pdf_path}", f"file://{html_path}"], check=True, capture_output=True)


def render(work, opt):
    meta = json.loads((work / "sections.json").read_text())
    book = Path(meta["book"]) if meta.get("book") else None
    sections = [s for s in meta["sections"] if s.get("file")]
    if opt.only:
        sections = [s for s in sections if s["id"] == opt.only] or sys.exit(f"no section {opt.only}")
    images_path = work / "images.json"
    images = json.loads(images_path.read_text()) if images_path.exists() else {}

    first_chapter = next((s for s in meta["sections"] if s["kind"] == "chapter"), meta["sections"][0])
    if "source" in (opt.bg, opt.fg):
        if not book:
            sys.exit("--bg/--fg 'source' samples the source PDF, which an EPUB build has none of; pass explicit #rrggbb")
        sampled = sample_colors(book, min(first_chapter["start"] + 1, meta["pages"]))
    else:
        sampled = None
    term = iterm_colors() if "iterm" in (opt.bg, opt.fg) else None
    resolve = lambda v, i: sampled[i] if v == "source" else term[i] if v == "iterm" else v
    bg, fg = resolve(opt.bg, 0), resolve(opt.fg, 1)

    ready = []
    for s in sections:
        md_path = work / "md" / (Path(s["file"]).stem + ".md")
        if md_path.exists():
            title_zh, body = md_to_html(md_path.read_text())
            if images:
                eq_scale = getattr(opt, "eq_scale", 0.6) if meta.get("source_kind") == "epub" else None
                body = place_images(body, work, images, fg, bg, eq_scale)
            ready.append((s, title_zh, body))
        else:
            print(f"skip {s['id']} {s['title']}: not translated yet", file=sys.stderr)
    if not ready:
        sys.exit("nothing to render")
    bold = brighten(fg, getattr(opt, "bold_factor", 1.25))
    style = css(meta["page_size"], bg, fg, [(s["id"], t, s["kind"]) for s, t, _ in ready], opt.font_size, bold)
    title = opt.title or meta["title"]
    chrome = chrome_binary()
    tmp = Path(tempfile.mkdtemp(prefix="render-"))

    def document(parts):
        return (f'<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>{html.escape(title)}</title>'
                f'{KATEX_HEAD}<style>{style}</style></head><body>{"".join(parts)}</body></html>')

    def typeset(name, parts):
        """Chrome-print one HTML document; returns (pdf path, marker -> page index within it)."""
        html_path, pdf_path = tmp / f"{name}.html", tmp / f"{name}.pdf"
        html_path.write_text(document(parts))
        print_pdf(chrome, html_path, pdf_path)
        return pdf_path, page_map(pdf_path)

    if opt.only:
        pdf_path, pages = typeset("section", [section_html(*r) for r in ready])
        out = work / "pdf" / (Path(ready[0][0]["file"]).stem + ".pdf")
        out.parent.mkdir(exist_ok=True)
        shutil.copyfile(pdf_path, out)
        print(f"{out} ({len(page_texts(pdf_path)) - 1} pages)")
        return

    # Front matter (roman folios) and body (arabic folios from 1) are separate Chrome documents, because
    # Chrome cannot reset the page counter mid-document; the contents page is re-typeset once the folios are known.
    front = [r for r in ready if r[0]["kind"] == "front"]
    body = [r for r in ready if r[0]["kind"] != "front"]
    body_pdf, body_pages = typeset("body", [section_html(*r) for r in body]) if body else (None, {})
    folios = {s["id"]: str(body_pages[f"S{s['id']}"] + 1) for s, _, _ in body}
    entries = [(s, t) for s, t, _ in ready]
    front_pdf, front_pages = None, {}
    for _ in range(3):
        front_pdf, front_pages = typeset("front", [contents_html(entries, folios)] + [section_html(*r) for r in front])
        new = {**folios, **{s["id"]: roman_folio(front_pages[f"S{s['id']}"]) for s, _, _ in front}}
        if new == folios:
            break
        folios = new
    missing = [s["id"] for s, _, _ in ready if f"S{s['id']}" not in front_pages and f"S{s['id']}" not in body_pages]
    if missing:
        sys.exit(f"markers not found for sections {missing}; the render is broken")
    n_front = len(page_texts(front_pdf)) - 1
    pages = {**{k: v for k, v in front_pages.items()}, **{k: v + n_front for k, v in body_pages.items()}}

    out = Path(opt.out) if opt.out else default_out(Path(meta.get("source") or book), work)
    cover_pdf = build_cover(meta, work, book, bg, chrome, tmp)
    assemble(cover_pdf, [front_pdf] + ([body_pdf] if body_pdf else []), out, ready, pages, meta, title, bg,
             top_margin=meta["page_size"][1] * 0.082, row_height=opt.font_size * 1.8)
    print(f"{out} ({len(pikepdf.open(out).pages)} pages, {len(ready)} sections, cover + linked 目录 + bookmarks)")


def default_out(source, work):
    """<source>-zh.pdf next to the original, or next to the work dir when the original's folder is not writable."""
    target = source.with_name(source.stem + "-zh.pdf")
    try:
        target.parent.joinpath(".translate-write-test").touch()
        target.parent.joinpath(".translate-write-test").unlink()
        return target
    except (PermissionError, OSError):
        return work.parent / target.name


def build_cover(meta, work, book, bg, chrome, tmp):
    """A one-page cover PDF: the source PDF's cover page (PDF build), or the EPUB's cover image rendered
    full-bleed onto a page of the book's size (EPUB build)."""
    out = tmp / "cover.pdf"
    if meta.get("source_kind") == "epub":
        w, h = meta["page_size"]
        img = meta.get("cover_image")
        if img and (work / img).exists():
            uri = (work / img).resolve().as_uri()
            inner = f'<img src="{uri}" style="position:fixed;top:0;left:0;width:100%;height:100%;object-fit:cover">'
        else:
            inner = f'<div style="color:#888;font-family:sans-serif;padding:40pt">{html.escape(meta.get("title", ""))}</div>'
        (tmp / "cover.html").write_text(
            f'<!doctype html><html><head><meta charset="utf-8"><style>'
            f'@page {{ size: {w}pt {h}pt; margin: 0; }} html,body {{ margin:0; background:{bg}; }}'
            f'</style></head><body>{inner}</body></html>')
        print_pdf(chrome, tmp / "cover.html", out)
        return out
    src = pikepdf.open(book)
    cover = next((s for s in meta["sections"] if s["kind"] == "cover"), None)
    cov = pikepdf.new()
    cov.pages.append(src.pages[(cover["start"] - 1) if cover else 0])
    cov.save(out)
    return out


def assemble(cover_pdf, part_pdfs, out, ready, pages, meta, title, bg, top_margin, row_height):
    """Cover page + the typeset parts; every page underlaid with the background, the running head masked on
    opener pages; 目录 rows linked to their sections; flat bookmarks (Cover, 目录, one per section)."""
    pdf = pikepdf.new()
    cov = pikepdf.open(cover_pdf)
    pdf.pages.append(cov.pages[0])
    parts = [pikepdf.open(p) for p in part_pdfs]
    for part in parts:
        pdf.pages.extend(part.pages)
    offset = 1
    r, g, b = (int(bg[i:i + 2], 16) / 255 for i in (1, 3, 5))
    openers = {pages[f"S{s['id']}"] + offset for s, _, _ in ready}
    for i, page in enumerate(pdf.pages):
        if i < offset:
            continue
        x0, y0, x1, y1 = (float(v) for v in page.mediabox)
        fill = f"q {r:.4f} {g:.4f} {b:.4f} rg {x0} {y0} {x1 - x0} {y1 - y0} re f Q\nq\n".encode()
        page.contents_add(pikepdf.Stream(pdf, fill), prepend=True)
        mask = f"q {r:.4f} {g:.4f} {b:.4f} rg {x0} {y1 - top_margin + 2} {x1 - x0} {top_margin - 2} re f Q\n".encode() if i in openers else b""
        page.contents_add(pikepdf.Stream(pdf, b"Q\n" + mask), prepend=False)
    link_contents(pdf, part_pdfs[0], offset, pages, ready, row_height)
    with pdf.open_outline() as outline:
        outline.root.append(pikepdf.OutlineItem("Cover", 0))
        if "TOC" in pages:
            outline.root.append(pikepdf.OutlineItem("目录", pages["TOC"] + offset))
        for s, t, _ in ready:
            name = f"{s['label']}　{t}" if s.get("label") else t
            outline.root.append(pikepdf.OutlineItem(name, pages[f"S{s['id']}"] + offset))
    with pdf.open_metadata() as m:
        m["dc:title"] = title
        if meta.get("author"):
            m["dc:creator"] = [meta["author"]]
    pdf.docinfo["/Title"] = title
    if meta.get("author"):
        pdf.docinfo["/Author"] = meta["author"]
    pdf.save(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("work")
    ap.add_argument("--out")
    ap.add_argument("--only", help="one section id: quick single-section PDF into <work>/pdf/")
    ap.add_argument("--title", help="Chinese book title for the PDF metadata")
    ap.add_argument("--bg", default="iterm", help="page background: #rrggbb, 'iterm' (the iTerm2 default profile's dark background, the default) or 'source' (sampled from the book)")
    ap.add_argument("--fg", default="#606e6a", help="text color: #rrggbb (default #606e6a, a cool gray), 'iterm' (the terminal's foreground) or 'source' (sampled from the book)")
    ap.add_argument("--font-size", type=float, default=9.25, help="body size in pt (default 9.25)")
    ap.add_argument("--eq-scale", type=float, default=0.6, help="EPUB build: block (display) equation images render at their intrinsic width times this (default 0.6), so all equations share one scale")
    ap.add_argument("--bold-factor", type=float, default=1.25, help="bold ink brightness relative to body text (default 1.25); Chinese bold uses a 黑体 face")
    opt = ap.parse_args()
    render(Path(opt.work).resolve(), opt)


if __name__ == "__main__":
    main()
