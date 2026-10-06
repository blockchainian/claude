#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Typesets the translated Markdown sections into a PDF in the source book's format (page size, colors,
# ABOUTME: running heads, folios, contents page) with headless Chrome, then adds the original cover and bookmarks.
#
# Usage: render.py <work dir> [--out <book.pdf>] [--only 04] [--title <中文书名>] [--bg iterm|#rrggbb --fg #6e7f7a|iterm] [--font-size 9.25]
# Without --only: the whole book (cover + 目录 + every translated section) to --out (default
# <title-slug>.pdf in ~/Documents). With --only: one section to <work>/pdf/<id>-<slug>.pdf for a quick look,
# no cover or contents.
import argparse
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import markdown
import pikepdf
from PIL import Image

ROMAN = ["", "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv", "xvi",
         "xvii", "xviii", "xix", "xx"]
MARK_TOC = "⟦TOC⟧"

# KaTeX typesets the inline \(..\) / display \[..\] LaTeX in the browser before Chrome prints (see print_pdf's
# --virtual-time-budget). Loaded from the CDN; headless Chrome has network access. Once the KaTeX web fonts have
# loaded (document.fonts.ready — measuring before they load under-scales, and the fonts then widen the equation
# and it gets clipped), any display equation still wider than the text column is shrunk with font-size to fit: a
# single over-wide element otherwise makes Chrome's print scale the WHOLE document's font down uniformly (see
# css()'s overflow guard). Any equation KaTeX could not parse renders as a red .katex-error with its raw source;
# a ⟦KERR⟧ marker is dropped next to each so render() can detect them and fail instead of shipping red source.
KATEX_HEAD = (
    '<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">'
    '<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>'
    '<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js" '
    'onload="renderMathInElement(document.body,{delimiters:['
    "{left:'\\\\[',right:'\\\\]',display:true},{left:'\\\\(',right:'\\\\)',display:false}"
    '],throwOnError:false});'
    "var bad=[];document.querySelectorAll('.katex-error').forEach(function(e){bad.push(e);});"  # structural errors,
    "document.querySelectorAll('.katex').forEach(function(k){if(/#cc0000/i.test(k.innerHTML))bad.push(k);});"  # and
    "bad.forEach(function(e){var m=document.createElement('span');m.className='mk';"  # inline red (undefined cmd) ones;
    "m.textContent='\\u27e6KERR\\u27e7';e.parentNode.insertBefore(m,e.nextSibling);});"  # mark now, not in fonts.ready
    "document.querySelectorAll('.nb').forEach(function(n){var t=n.lastChild,h=n.querySelector('.katex-html');"  # math
    "if(!h||!t||t.nodeType!==3)return;var b=h.querySelectorAll(':scope>.base');if(!b.length)return;"  # + punctuation:
    "var p=document.createElement('span');p.className='nbp';p.textContent=t.textContent;"  # move the punctuation
    "b[b.length-1].appendChild(p);t.remove();n.className='nbm';});"  # into the formula's last no-break atom
    "document.fonts.ready.then(function(){"
    "var col=parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--eqcol-w'))||1e9;"
    "document.querySelectorAll('.katex-display').forEach(function(d){var w=0;"
    "d.querySelectorAll('.base').forEach(function(b){w=Math.max(w,b.getBoundingClientRect().width);});"
    "if(w>col){d.style.fontSize=(0.98*col/w)+'em';}});});"
    '"></script>'
)


def brighten(hexcolor, factor):
    """A #rrggbb color scaled per-channel and clamped: used to make bold ink a step brighter than the body."""
    return "#%02x%02x%02x" % tuple(min(255, round(c * factor)) for c in _hex(hexcolor))


def chrome_binary():
    for c in ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
              "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
              "/Applications/Chromium.app/Contents/MacOS/Chromium", shutil.which("google-chrome"), shutil.which("chrome"), shutil.which("chromium"), shutil.which("chromium-browser")]:
        if c and Path(c).exists():
            return c
    sys.exit("Chrome not found: install Google Chrome, Chrome for Testing, or Chromium")


def iterm_colors():
    """The iTerm2 default profile's dark-mode background and foreground, as #rrggbb; a near-black page and the
    default text gray when iTerm2 is not installed."""
    import plistlib
    plist = Path.home() / "Library/Preferences/com.googlecode.iterm2.plist"
    if not plist.exists():
        return "#000409", "#6e7f7a"
    prefs = plistlib.load(plist.open("rb"))
    guid = prefs.get("Default Bookmark Guid")
    profile = next((b for b in prefs.get("New Bookmarks", []) if b.get("Guid") == guid), None) or prefs["New Bookmarks"][0]
    def hx(key):
        c = profile.get(key + " (Dark)") if profile.get("Use Separate Colors for Light and Dark Mode") else None
        c = c or profile[key]
        return "#%02x%02x%02x" % tuple(round(c[k] * 255) for k in ("Red Component", "Green Component", "Blue Component"))
    return hx("Background Color"), hx("Foreground Color")


MATH_RE = re.compile(r"\\\[.*?\\\]|\\\(.*?\\\)", re.S)
# Inline math or inline code followed by CJK closing punctuation. render wraps each pair in a .nb span so the
# punctuation can never wrap to the start of the next line (a CJK line must not open with ，or 。).
CJK_CLOSE = "，。、；：！？）」』》〉】〕…—”’"
NOBREAK_RE = re.compile(rf"(@@MATH(\d+)@@|<code>[^<]*</code>)[{CJK_CLOSE}]+")


def repair_math(raw):
    """Fix the LaTeX mistakes the translator commonly makes, which KaTeX would otherwise render as red error
    source or (for leaked blockquote markers) with spurious symbols:
    - blockquote continuation markers that leaked into a multi-line display (\\n> ...) -> drop them;
    - inline \\(..\\) that carries a \\tag (KaTeX allows \\tag only in display math) -> promote to display \\[..\\];
    - a literal currency $ inside math (KaTeX reads $ as a math-mode delimiter) -> escape it as \\$;
    - \\mbox (a LaTeX box command KaTeX does not implement, used for hyphens in operator names) -> \\text."""
    raw = re.sub(r"\n>[ \t]?", "\n", raw)
    if raw.startswith("\\(") and "\\tag" in raw:
        raw = "\\[" + raw[2:-2] + "\\]"
    raw = re.sub(r"(?<!\\)\$", r"\\$", raw)
    raw = raw.replace("\\mbox", "\\text")
    return raw


def md_to_html(md_text):
    """Section Markdown -> body HTML: the first '# ' line is the title (returned separately). Inline LaTeX
    (\\(..\\), \\[..\\]) is stashed before Markdown runs, because Markdown would treat the _ and * inside it as
    emphasis and corrupt it; the raw math is restored (HTML-escaped) for KaTeX to typeset in the browser. Along
    the way the math is repaired (see repair_math) and math-only blockquote lines are unwrapped, so a display
    equation sits in the full text column (a blockquote's narrower column would clip a wide equation)."""
    lines = md_text.strip().splitlines()
    title = lines[0][2:].strip() if lines and lines[0].startswith("# ") else ""
    body = "\n".join(lines[1:] if title else lines)
    body = re.sub(r"^# ", "## ", body, flags=re.M)
    body = body.replace("\\（", "\\(").replace("\\）", "\\)")  # a mis-typed full-width delimiter breaks math pairing
    vault = []

    def stash(m):
        vault.append(repair_math(m.group(0)))
        return f"@@MATH{len(vault) - 1}@@"

    # A line continued after a hard break ("  " or <br>) keeps its leading ideographic spaces (a staircase or an
    # aligned row); Markdown strips them, so they go in as entities.
    body = re.sub(r"((?:  |<br\s*/?>)\n)(\u3000+)", lambda m: m.group(1) + "&#12288;" * len(m.group(2)), body)
    body = MATH_RE.sub(stash, body)
    body = re.sub(r"^>[ \t]?(@@MATH\d+@@)[ \t]*$", r"\1", body, flags=re.M)  # unwrap a math-only blockquote line
    out = markdown.markdown(body, extensions=["smarty"], output_format="html")
    out = NOBREAK_RE.sub(lambda m: m.group(0) if m.group(1).startswith("@@MATH") and vault[int(m.group(2))].startswith("\\[")
                         else f'<span class="nb">{m.group(0)}</span>', out)
    for i, raw in enumerate(vault):
        out = out.replace(f"@@MATH{i}@@", html.escape(raw))
    return title, out


# A block figure and the caption paragraph right after it ("**图 N.**", "**表 N**", "**Fig. N**", "**Table N**"):
# place_images wraps the pair in a .figcap block that a page break cannot split.
FIGCAP_RE = re.compile(r'(<figure class="fig[^"]*"><img[^>]*></figure>)(?:<p></p>)?\s*'
                       r'(<p><strong>\s*(?:图|表|Fig|Table)(?:(?!</p>).)*</p>)', re.S)
EMPTY_P_RE = re.compile(r"<p>\s*</p>")
# The paragraph right before a block figure (or figure + caption): it leads into the figure ("如下：", "**9、10、11。**")
# and is kept with it (.keep: break-after: avoid) so it is never left alone at a page bottom.
LEAD_P_RE = re.compile(r'<p>((?:(?!</?p>).)*)</p>(?=\s*(?:<figure class="fig|<div class="figcap">))', re.S)
IMG_TOKEN_RE = re.compile(rf"⟦IMG:([^⟧]+)⟧([{CJK_CLOSE}]*)")  # with any CJK punctuation right after it


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


def place_images(body_html, work, images, fg, bg, eq_scale=None, inline_scale=None):
    """Replace ⟦IMG:key⟧ placeholders with the extracted images. Line art (equations, diagrams) is recoloured
    to foreground ink on the opaque page colour so it blends into the dark page; a colour figure keeps a white
    plate (inverting a photo would ruin it). Block images become their own centred figure; inline ones sit in
    the line. With eq_scale set (EPUB build), every block figure is sized to its intrinsic width times eq_scale,
    so equations render at one consistent scale instead of at each raw crop's pixel size; with inline_scale set,
    every inline image is sized the same way (its own factor, so an in-line formula matches the body text) instead
    of being clamped to the line height, which squashes every formula to one height whatever its content."""
    def scaled_width(src, scale):
        try:
            return f"width:{Image.open(src).size[0] * scale:.1f}pt;max-width:100%"
        except Exception:
            return ""

    def repl(m):
        meta = images.get(m.group(1))
        if not meta:
            return m.group(2)
        src = work / "images" / meta["file"]
        rc = src.with_suffix(".rc.png")
        try:
            line = recolor_line_art(src, fg, bg, rc)
        except Exception:
            line = False
        uri = (rc if line else src).resolve().as_uri()
        plate = "" if line else " plate"
        if meta.get("block"):
            width = scaled_width(src, eq_scale) if eq_scale else ""
            style = f' style="{width}"' if width else ""
            return f'</p><figure class="fig{plate}"><img{style} src="{uri}"></figure><p>{m.group(2)}'
        width = scaled_width(src, inline_scale) if inline_scale else ""
        style = f' style="{width};height:auto;max-height:none"' if width else ""
        img = f'<img class="infig{plate}"{style} src="{uri}">'
        return f'<span class="nb">{img}{m.group(2)}</span>' if m.group(2) else img  # punctuation never opens a line
    out = FIGCAP_RE.sub(r'<div class="figcap">\1\2</div>', IMG_TOKEN_RE.sub(repl, body_html))
    out = EMPTY_P_RE.sub("", out)  # the shells left round a figure that stood alone in its paragraph
    return LEAD_P_RE.sub(r'<p class="keep">\1</p>', out)


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
    colpx = round((w - 2 * side) * 96 / 72, 1)  # printable text-column width in CSS px, for KATEX_HEAD's equation fit
    return f"""
:root {{ --bg: {bg}; --fg: {fg}; --eqcol-w: {colpx}px; }}
@page {{ size: {w}pt {h}pt; margin: {top}pt {side}pt {bottom}pt; background: var(--bg);
        @bottom-center {{ content: counter(page, lower-roman); font-size: 8pt; color: {fg}; {margin_font} }} }}
{named}
html {{ background: var(--bg); }}
body {{ margin: 0; color: var(--fg); font-family: Baskerville, "Songti SC", serif; font-size: {font_size}pt; line-height: 1.8; overflow-wrap: break-word; }}
strong, b {{ font-family: {hei}; font-weight: 700; color: {bold}; }}
.katex {{ color: {fg}; font-size: 1em; }}
/* Chrome's print scales the WHOLE document's font down uniformly to fit the single widest element that overflows
   the text column (a long display equation, a wide image or table, an unbreakable line). Contain every section's
   horizontal overflow so one wide element can never shrink the book; each kind of wide content is also made to fit
   (equations scaled by KATEX_HEAD, media capped at 100%, code/long tokens wrapped) so clipping never bites. */
section {{ break-before: page; overflow-x: clip; overflow-clip-margin: 6pt; }}
img, table, pre, .katex-display {{ max-width: 100%; }}
pre {{ white-space: pre-wrap; overflow-wrap: anywhere; }}
/* Code is set in KaTeX's typewriter face (loaded by the KaTeX CSS) at the size \mathtt renders, so `ROW` and
   \(\mathtt{{BASEROW}}\) on one line are the same monospace font. */
code, pre {{ font-family: KaTeX_Typewriter, Menlo, monospace; font-size: 1em; }}
.opener {{ padding-top: {round(h * 0.2)}pt; text-align: center; margin-bottom: {round(h * 0.07)}pt; }}
.opener .label {{ font-family: {hei}; font-weight: 700; color: {bold}; font-size: 9pt; letter-spacing: 3pt; margin-bottom: 14pt; }}
.opener h1 {{ font-family: {hei}; font-weight: 700; color: {bold}; font-size: 22pt; letter-spacing: 2pt; margin: 0; line-height: 1.5; }}
.mk {{ font-size: 1pt; color: transparent; letter-spacing: 0; white-space: nowrap; font-family: Baskerville, "Songti SC", serif; }}
p {{ margin: 0; text-indent: 2em; text-align: justify; }}
p:has(br) {{ text-indent: 0; }}  /* hard line breaks set aligned rows (number tables): no first-line indent */
/* Inline math/code + its CJK closing punctuation: no break between them (inline code is short). After KaTeX
   runs, KATEX_HEAD moves a formula's punctuation into its last atom (.nbp) and lifts the nowrap (.nbm), so a long
   formula keeps its own break points and only its last atom is bound to the punctuation. */
.nb {{ white-space: nowrap; }}
.nbp {{ font-family: Baskerville, "Songti SC", serif; }}
.body-text > p:first-of-type {{ text-indent: 0; }}
.body-text:not(.no-dropcap) > p:first-of-type::first-letter {{ float: left; font-size: 2.6em; line-height: 0.85; padding: 3pt 4pt 0 0; }}
h2 {{ font-family: {hei}; font-size: 11pt; font-weight: 700; color: {bold}; margin: 18pt 0 6pt; break-after: avoid; }}
h3 {{ font-family: {hei}; font-size: 10.5pt; font-weight: 700; color: {bold}; margin: 12pt 0 4pt; break-after: avoid; }}
hr {{ border: 0; height: 0; margin: 1.2em 0; break-after: avoid; }}  /* a scene break (Markdown ---): blank space, never the browser's inset light border */
hr + p {{ text-indent: 0; }}
blockquote {{ margin: 8pt 2em; font-style: italic; }}
blockquote p {{ text-indent: 0; }}
ul, ol {{ margin: 4pt 0 4pt 2em; padding: 0; }}
li {{ margin: 2pt 0; }}
em {{ font-family: {hei}; font-weight: 700; font-style: normal; color: {bold}; }}  /* Chinese emphasis: 黑体 bold, not italic (Songti italic is illegible); math variables are KaTeX, not <em> */
.fig {{ margin: 10pt auto; text-align: center; break-inside: avoid; }}
.fig img {{ max-width: 100%; height: auto; }}
.figcap {{ break-inside: avoid; }}  /* a figure and its caption stay on one page */
p.keep {{ break-after: avoid; }}  /* a lead-in paragraph stays with the figure it introduces */
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


FIRST_P_RE = re.compile(r"<p>(.*?)</p>", re.S)
LEADING_IMG_RE = re.compile(r"^(\s|<img\b[^>]*>)*")
DROPCAP_MIN_CHARS = 60  # the 2.6em cap spans two lines; a shorter first paragraph cannot wrap round it


def wants_dropcap(body):
    """A drop cap suits a section that opens with prose. Not when its first paragraph opens with a bold label
    (an answers or exercises section: "**1.** ...", possibly after a ▶ marker image) or a digit, where the cap
    would land on the number, nor when the paragraph is too short for the next lines to wrap round the cap."""
    m = FIRST_P_RE.search(body)
    if not m:
        return False
    head = LEADING_IMG_RE.sub("", m.group(1), count=1)
    if head.startswith(("<strong", "<b>")) or head[:1].isdigit():
        return False
    return len(re.sub(r"<[^>]+>", "", m.group(1)).strip()) >= DROPCAP_MIN_CHARS


def section_html(s, title, body):
    cls = ["body" if s["kind"] != "front" else "front"]
    label = f'<div class="label">{html.escape(s["label"])}</div>' if s.get("label") else ""
    return (f'<section class="{" ".join(cls)}" style="page: s{s["id"]}" id="s{s["id"]}">'
            f'<div class="opener">{label}<h1>{html.escape(title)}<span class="mk">⟦S{s["id"]}⟧</span></h1></div>'
            f'<div class="body-text{"" if wants_dropcap(body) else " no-dropcap"}">{body}</div></section>')


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


def katex_errors(pdf_path, marker_pages):
    """(page index, section id) for every page carrying a ⟦KERR⟧ marker. KATEX_HEAD drops that marker next to
    each equation KaTeX could not parse (which otherwise ships as red raw LaTeX source); render() fails on any,
    so a broken formula can never pass silently. marker_pages maps 'S<id>'/'TOC' -> page index in this PDF."""
    sec_at = sorted((v, k[1:]) for k, v in marker_pages.items() if k.startswith("S"))
    errs = []
    for i, text in enumerate(page_texts(pdf_path)):
        if re.search(r"⟦\s*K\s*E\s*R\s*R\s*⟧", text):
            sid = next((s for p, s in reversed(sec_at) if p <= i), "?")
            errs.append((i, sid))
    return errs


def roman_folio(page_index):
    return ROMAN[page_index + 1] if page_index + 1 < len(ROMAN) else str(page_index + 1)


def folio_for(page_index, first_body_index):
    if first_body_index is None or page_index < first_body_index:
        return roman_folio(page_index)
    return str(page_index - first_body_index + 1)


def page_colors(opt):
    """(background, foreground) as #rrggbb, with 'iterm' resolved to the iTerm2 profile's colors."""
    term = iterm_colors() if "iterm" in (opt.bg, opt.fg) else None
    resolve = lambda v, i: term[i] if v == "iterm" else v
    return resolve(opt.bg, 0), resolve(opt.fg, 1)


def paint_background(pdf, page, bg, masked_top=0):
    """Underlay one page with the background color; masked_top > 0 also covers that much of the top margin
    over the content (the running head on an opener page)."""
    r, g, b = (c / 255 for c in _hex(bg))
    x0, y0, x1, y1 = (float(v) for v in page.mediabox)
    fill = f"q {r:.4f} {g:.4f} {b:.4f} rg {x0} {y0} {x1 - x0} {y1 - y0} re f Q\nq\n".encode()
    page.contents_add(pikepdf.Stream(pdf, fill), prepend=True)
    mask = f"q {r:.4f} {g:.4f} {b:.4f} rg {x0} {y1 - masked_top + 2} {x1 - x0} {masked_top - 2} re f Q\n".encode() if masked_top else b""
    page.contents_add(pikepdf.Stream(pdf, b"Q\n" + mask), prepend=False)


def print_pdf(chrome, html_path, pdf_path):
    # --virtual-time-budget lets KaTeX (loaded from the CDN) finish typesetting the math before the snapshot;
    # without it the page prints with raw \(...\) source.
    subprocess.run([chrome, "--headless=new", "--disable-gpu", "--no-pdf-header-footer",
                    "--virtual-time-budget=15000",
                    f"--print-to-pdf={pdf_path}", f"file://{html_path}"], check=True, capture_output=True)


def render(work, opt):
    meta = json.loads((work / "sections.json").read_text())
    sections = [s for s in meta["sections"] if s.get("file")]
    if opt.only:
        sections = [s for s in sections if s["id"] == opt.only] or sys.exit(f"no section {opt.only}")
    images_path = work / "images.json"
    images = json.loads(images_path.read_text()) if images_path.exists() else {}

    bg, fg = page_colors(opt)

    ready = []
    for s in sections:
        md_path = work / "md" / (Path(s["file"]).stem + ".md")
        if md_path.exists():
            title_zh, body = md_to_html(md_path.read_text())
            if images:
                epub = meta.get("source_kind") == "epub"
                eq_scale = getattr(opt, "eq_scale", 0.6) if epub else None
                inline_scale = getattr(opt, "inline_scale", 0.33) if epub else None
                body = place_images(body, work, images, fg, bg, eq_scale, inline_scale)
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
        report_katex_errors(katex_errors(pdf_path, pages))
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

    out = Path(opt.out) if opt.out else default_out(meta, work)
    cover_pdf = build_cover(meta, work, bg, chrome, tmp)
    assemble(cover_pdf, [front_pdf] + ([body_pdf] if body_pdf else []), out, ready, pages, meta, title, bg,
             top_margin=meta["page_size"][1] * 0.082, row_height=opt.font_size * 1.8)
    print(f"{out} ({len(pikepdf.open(out).pages)} pages, {len(ready)} sections, cover + linked 目录 + bookmarks)")
    errs = katex_errors(body_pdf, body_pages) if body_pdf else []
    errs += katex_errors(front_pdf, front_pages)
    report_katex_errors(errs)


def report_katex_errors(errs):
    """Fail (after the PDF is written, so it can be inspected) if any equation rendered as a red KaTeX error."""
    if not errs:
        return
    where = ", ".join(f"section {sid} (page {p + 1})" for p, sid in errs)
    sys.exit(f"{len(errs)} equation(s) failed to render (red raw LaTeX source) in {where}; fix the LaTeX and re-render")


def title_slug(title):
    """The book's main title as a filename slug: the part before a ':'/'：' subtitle, lowercased, every run of
    non-alphanumerics turned into a single dash. Returns '' when the title has no Latin letters/digits (e.g. an
    already-Chinese title), so the caller can fall back to the work dir name."""
    main = re.split(r"[:：]", title or "", 1)[0]
    return re.sub(r"[^a-z0-9]+", "-", main.lower()).strip("-")


def default_out(meta, work):
    """<title-slug>.pdf in the user's ~/Documents, or next to the work dir when Documents is not writable.
    An already-Chinese or empty title falls back to the work dir's slug name."""
    name = (title_slug(meta.get("title")) or work.name) + ".pdf"
    docs = Path.home() / "Documents"
    try:
        docs.mkdir(parents=True, exist_ok=True)
        docs.joinpath(".translate-write-test").touch()
        docs.joinpath(".translate-write-test").unlink()
        return docs / name
    except (PermissionError, OSError):
        return work.parent / name


def build_cover(meta, work, bg, chrome, tmp):
    """A one-page cover PDF: the EPUB's cover image rendered full-bleed onto a page of the book's size."""
    out = tmp / "cover.pdf"
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
    openers = {pages[f"S{s['id']}"] + offset for s, _, _ in ready}
    for i, page in enumerate(pdf.pages):
        if i < offset:
            continue
        paint_background(pdf, page, bg, top_margin if i in openers else 0)
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


def add_style_args(ap):
    """The page-style options shared by every PDF typeset in this book format."""
    ap.add_argument("--bg", default="iterm", help="page background: #rrggbb or 'iterm' (the iTerm2 default profile's dark background, the default)")
    ap.add_argument("--fg", default="#6e7f7a", help="text color: #rrggbb (default #6e7f7a, a cool gray) or 'iterm' (the terminal's foreground)")
    ap.add_argument("--font-size", type=float, default=9.25, help="body size in pt (default 9.25)")
    ap.add_argument("--bold-factor", type=float, default=1.25, help="bold ink brightness relative to body text (default 1.25); Chinese bold uses a 黑体 face")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("work")
    ap.add_argument("--out")
    ap.add_argument("--only", help="one section id: quick single-section PDF into <work>/pdf/")
    ap.add_argument("--title", help="Chinese book title for the PDF metadata")
    add_style_args(ap)
    ap.add_argument("--eq-scale", type=float, default=0.6, help="EPUB build: block (display) equation images render at their intrinsic width times this (default 0.6), so all equations share one scale")
    ap.add_argument("--inline-scale", type=float, default=0.33, help="EPUB build: inline images (in-line formulas, the ▶ marker) render at their intrinsic width times this (default 0.33, about body-text size for 28px-per-em formula crops)")
    opt = ap.parse_args()
    render(Path(opt.work).resolve(), opt)


if __name__ == "__main__":
    main()
