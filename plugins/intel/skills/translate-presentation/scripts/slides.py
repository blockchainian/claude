#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf", "pikepdf", "fonttools", "markdown", "pillow", "numpy"]
# ///
# ABOUTME: Translates a slide-deck PDF into Chinese page for page: recolours the deck to the translate skill's dark
# theme, then replaces each prose text block in place with its fitted Chinese translation.
# Usage: slides.py extract <deck.pdf> <work dir>
#        slides.py render  <deck.pdf> <work dir> <out.pdf>
import colorsys
import functools
import glob
import html
import json
import os
import re
import sys
from collections import Counter
from decimal import Decimal

import pikepdf
import pymupdf

HERE = os.path.dirname(os.path.abspath(__file__))


def translate_skill_scripts():
    """The translate skill's scripts dir: the sibling skill in this plugin, else the newest installed copy."""
    sibling = os.path.join(HERE, "..", "..", "translate", "scripts")
    if os.path.exists(os.path.join(sibling, "render.py")):
        return sibling
    vers = glob.glob(os.path.expanduser("~/.claude/plugins/cache/blockchainian/intel/*/skills/translate/scripts"))
    key = lambda p: tuple(int(x) for x in re.findall(r"\d+", p.split("/intel/")[1].split("/")[0]))
    return max(vers, key=key)


sys.path.insert(0, translate_skill_scripts())
from render import _hex, brighten  # noqa: E402  (theme helpers shared with the book renderer)

BG, FG = "#000409", "#6e7f7a"
BOLD = brighten(FG, 1.25)
MONO_RE = re.compile(r"mono|courier|consolas|menlo|inconsolata|monaco|sourcecode|firacode|typewriter|cmtt", re.I)
FONT_DIR = os.path.join(os.environ.get("INTEL_STATE_DIR", os.path.expanduser("~/.local/state/intel")),
                        "translate-presentation", "fonts")


# ---------------------------------------------------------------- recolouring

def rgb01(hexcolor):
    return tuple(c / 255 for c in _hex(hexcolor))


def hex_of(rgb):
    return "#%02x%02x%02x" % tuple(round(255 * v) for v in rgb)


def map_color(rgb):
    """Maps a colour on a light slide to its dark-theme counterpart. Neutrals invert: white paper becomes the page
    background and black ink the body colour. Chromatic colours keep their hue; light tints become dark tints of the
    same hue and strong colours are held to a brightness readable on black."""
    r, g, b = rgb
    bg, fg = rgb01(BG), rgb01(FG)
    if max(rgb) - min(rgb) < 0.12:
        t = 1 - (r + g + b) / 3
        return tuple(bg[i] + t * (fg[i] - bg[i]) for i in range(3))
    h, l, s = colorsys.rgb_to_hls(r, g, b)
    l2 = (1 - l) * 0.6 if l > 0.72 else min(max(l, 0.38), 0.5)
    return colorsys.hls_to_rgb(h, l2, s * 0.85)


def map_fill(rgb):
    """A shape's fill: light neutral and pale tinted boxes (title bars, callouts, paper) vanish into the page
    background, so text sits on the plain dark page; darker and coloured fills (diagram shapes) go through
    map_color."""
    neutral = max(rgb) - min(rgb) < 0.12
    l = colorsys.rgb_to_hls(*rgb)[1]
    if (neutral and l > 0.6) or l > 0.85:
        return rgb01(BG)
    return map_color(rgb)


def map_text(rgb):
    """Text left in place (footers, labels, code): neutral ink of any shade takes the body colour, so white text
    that sat on a now-vanished dark box stays readable; coloured text (syntax highlighting) keeps its hue at a
    lightness of at least 0.55, legible on the page and on darkened code boxes alike."""
    if max(rgb) - min(rgb) < 0.12:
        return rgb01(FG)
    h, l, s = colorsys.rgb_to_hls(*map_color(rgb))
    return colorsys.hls_to_rgb(h, max(l, 0.55), s)


def map_pixels(rgb):
    """map_color over an (N, 3) array of RGB values in 0..1."""
    import numpy as np
    mx, mn = rgb.max(1), rgb.min(1)
    d = mx - mn
    bg, fg = np.array(rgb01(BG)), np.array(rgb01(FG))
    out = bg + (1 - rgb.mean(1))[:, None] * (fg - bg)  # neutrals
    l = (mx + mn) / 2
    s = np.where(d > 0, d / np.maximum(1 - np.abs(2 * l - 1), 1e-6), 0)
    r, g, b = rgb[:, 0], rgb[:, 1], rgb[:, 2]
    dd = np.maximum(d, 1e-6)
    h = np.where(mx == r, ((g - b) / dd) % 6, np.where(mx == g, (b - r) / dd + 2, (r - g) / dd + 4)) / 6
    l2 = np.where(l > 0.72, (1 - l) * 0.6, np.clip(l, 0.38, 0.5))
    s2 = np.minimum(s * 0.85, 1)
    c = (1 - np.abs(2 * l2 - 1)) * s2
    hp = h * 6
    x = c * (1 - np.abs(hp % 2 - 1))
    z = np.zeros_like(c)
    sector = np.floor(hp).astype(int) % 6
    table = [(c, x, z), (x, c, z), (z, c, x), (z, x, c), (x, z, c), (c, z, x)]
    rgb2 = np.zeros_like(rgb)
    for k, (cr, cg, cb) in enumerate(table):
        m = sector == k
        rgb2[m] = np.stack([cr[m], cg[m], cb[m]], 1)
    rgb2 += (l2 - c / 2)[:, None]
    chromatic = d >= 0.12
    out[chromatic] = rgb2[chromatic]
    return np.clip(out, 0, 1)


def is_flat_light(rgb):
    """Whether an image is a screenshot or diagram drawn on a light ground, not a photo: a quarter of its pixels
    are light (lightness over 0.78, which takes in pastel fills) and its eight commonest colours (at 4 bits a
    channel, absorbing JPEG noise) cover half of it."""
    import numpy as np
    if len(rgb) == 0:
        return False
    light = ((rgb.max(1) + rgb.min(1)) / 2 > 0.78).mean()
    q = (rgb * 15).round().astype(int)
    codes = q[:, 0] * 256 + q[:, 1] * 16 + q[:, 2]
    top = np.sort(np.bincount(codes))[-8:].sum() / len(codes)
    return light >= 0.25 and top >= 0.5


def recolor_images(doc, skip_pages):
    """Recolours screenshots and diagrams (is_flat_light) on every page not in skip_pages for the dark theme, by
    running their pixels through map_color: the light ground turns into the page colour and dark lines and text
    into the body ink, while colours keep their hue. Photos, background pictures and images with their own
    transparency (icons, emoji) stay as they are. Returns how many images were recoloured."""
    import numpy as np
    done, count = set(), 0
    for pno, page in enumerate(doc):
        if pno in skip_pages:
            continue
        for xref, smask, *_ in page.get_images(full=True):
            if xref in done or smask:  # an image with its own transparency is an icon or emoji, kept as drawn
                continue
            done.add(xref)
            try:
                pix = pymupdf.Pixmap(doc, xref)
            except Exception:
                continue
            if pix.width * pix.height < 256 or pix.colorspace is None:  # tiny, or a stencil mask
                continue
            if pix.colorspace.n != 3:
                pix = pymupdf.Pixmap(pymupdf.csRGB, pix)
            if pix.alpha:
                pix = pymupdf.Pixmap(pix, 0)
            rgb = np.frombuffer(pix.samples, np.uint8).reshape(-1, 3) / 255
            if not is_flat_light(rgb):
                continue
            out = (map_pixels(rgb) * 255).round().astype(np.uint8)
            page.replace_image(xref, pixmap=pymupdf.Pixmap(pymupdf.csRGB, pix.width, pix.height, out.tobytes(), False))
            count += 1
    return count


def cmyk_to_rgb(c, m, y, k):
    return ((1 - c) * (1 - k), (1 - m) * (1 - k), (1 - y) * (1 - k))


SET_FILL = {"g", "rg", "k", "sc", "scn"}
SET_STROKE = {"G", "RG", "K", "SC", "SCN"}
PATH_START = {"m", "re"}
FILL_PAINT = {"f", "F", "f*", "B", "B*", "b", "b*"}
STROKE_PAINT = {"S", "s", "B", "B*", "b", "b*"}
TEXT_SHOW = {"Tj", "TJ", "'", '"'}


def device_rgb(operands):
    """The RGB value of a colour operator's numeric operands, or None for a pattern or other colour."""
    if not operands or not all(isinstance(o, (int, float, Decimal)) for o in operands):
        return None
    v = [min(max(float(o), 0), 1) for o in operands]
    return {1: lambda: (v[0],) * 3, 3: lambda: tuple(v), 4: lambda: cmyk_to_rgb(*v)}.get(len(v), lambda: None)()


def recolor_stream(ops, masks, page_size=None, keep_page_fill=False):
    """Rewrites a parsed content stream for the dark theme. Colour operators are dropped and the colour each
    painting operation needs is set right before it, chosen by use: shape fills through map_fill, strokes through
    map_color, text through map_text, while a stencil image mask (masks: its XObject names) and inline images keep
    the original fill colour, since a mask is part of a picture. Pattern colours are left as they are. On a page
    whose background is a picture (keep_page_fill), a fill covering the whole page keeps its colour: it is the
    picture's base, and viewers that honour the transparency of JPEG 2000 tiles (Preview) show it through them."""
    out, fill, stroke, stack, path_at = [], (0, 0, 0), (0, 0, 0), [], None  # PDF's initial colours are black
    path = []

    def rg(rgb, stroking=False):
        return ([round(v, 4) for v in rgb], pikepdf.Operator("RG" if stroking else "rg"))

    for operands, op in ops:
        name = str(op)
        if name in SET_FILL or name in SET_STROKE:
            rgb = device_rgb(operands)
            if rgb is None:  # a pattern or unknown colour: keep the operator and stop overriding
                out.append((operands, op))
            if name in SET_FILL:
                fill = rgb
            else:
                stroke = rgb
            continue
        if name == "q":
            stack.append((fill, stroke))
        elif name == "Q" and stack:
            fill, stroke = stack.pop()
        if name in PATH_START and path_at is None:
            path_at = len(out)
        if path_at is not None and name not in FILL_PAINT | STROKE_PAINT | {"n"}:
            path.append((name, operands))
        if name in FILL_PAINT or name in STROKE_PAINT or name == "n":
            set_ops = []
            whole_page = (page_size and len(path) == 1 and path[0][0] == "re"
                          and abs(float(path[0][1][2])) >= 0.9 * page_size[0]
                          and abs(float(path[0][1][3])) >= 0.9 * page_size[1])
            if name in FILL_PAINT and fill is not None:
                set_ops.append(rg(fill if keep_page_fill and whole_page else map_fill(fill)))
            if name in STROKE_PAINT and stroke is not None:
                set_ops.append(rg(map_color(stroke), True))
            at = len(out) if path_at is None else path_at
            out[at:at] = set_ops
            path_at, path = None, []
        elif name in TEXT_SHOW:
            if fill is not None:
                out.append(rg(map_text(fill)))
            if stroke is not None:
                out.append(rg(map_color(stroke), True))
        elif (name == "Do" and operands and str(operands[0]) in masks) or name == "INLINE IMAGE":
            if fill is not None:
                out.append(rg(fill))
        out.append((operands, op))
    return out


def image_masks(resources):
    xobjs = resources.get("/XObject") if resources is not None else None
    return {str(k) for k, xo in xobjs.items()  # a broken deck can hold null entries
            if xo is not None and xo.get("/Subtype") == "/Image" and xo.get("/ImageMask")} if xobjs else set()


def recolor_forms(resources, seen):
    xobjs = resources.get("/XObject") if resources is not None else None
    if not xobjs:
        return
    for _, xo in xobjs.items():
        if xo is None or xo.get("/Subtype") != "/Form" or xo.objgen in seen:
            continue
        seen.add(xo.objgen)
        res = xo.get("/Resources")
        xo.write(pikepdf.unparse_content_stream(recolor_stream(pikepdf.parse_content_stream(xo), image_masks(res))))
        recolor_forms(res, seen)


def picture_backed(page):
    """Whether one picture, or one group of abutting image tiles, covers nearly the whole page, 90% (measured on a
    100x100 grid, so overlapping tiles count once): a slide set on a background picture, not a slide with many
    small images or large textured code boxes."""
    groups = []  # lists of image boxes that touch
    for i in page.get_image_info():
        r = pymupdf.Rect(i["bbox"]) & page.rect
        if r.is_empty:
            continue
        joined = [r]
        for g in [g for g in groups if any((m + (-1, -1, 1, 1)).intersects(r) for m in g)]:
            joined += g
            groups.remove(g)
        groups.append(joined)
    W, H = page.rect.width, page.rect.height
    for g in groups:
        cells = {(x, y) for r in g for x in range(int(r.x0 / W * 100), int(r.x1 / W * 100))
                 for y in range(int(r.y0 / H * 100), int(r.y1 / H * 100))}
        if len(cells) >= 9000:
            return True
    return False


def recolor_pdf(src, dest):
    """Writes a copy of the deck for the dark theme (see recolor_stream) with a background-colour rectangle under
    every page. Images are left as they are."""
    pdf = pikepdf.open(src)
    pictured = [picture_backed(p) for p in pymupdf.open(src)]
    seen = set()
    bg = rgb01(BG)
    for pno, page in enumerate(pdf.pages):
        x0, y0, x1, y1 = [float(v) for v in page.mediabox]
        ops = recolor_stream(pikepdf.parse_content_stream(page), image_masks(page.get("/Resources")),
                             (x1 - x0, y1 - y0), pictured[pno])
        head = [([], pikepdf.Operator("q")), ([round(v, 4) for v in bg], pikepdf.Operator("rg")),
                ([x0, y0, x1 - x0, y1 - y0], pikepdf.Operator("re")), ([], pikepdf.Operator("f")),
                ([], pikepdf.Operator("Q"))]
        page.Contents = pdf.make_stream(pikepdf.unparse_content_stream(head + ops))
        recolor_forms(page.get("/Resources"), seen)
    pdf.save(dest)


# ---------------------------------------------------------------- extraction

def upright(deck, work):
    """The deck with every page's /Rotate baked into its content, written to <work>/upright.pdf when any page is
    rotated (else the deck itself): on a rotated page PyMuPDF reports text in unrotated coordinates, where a line
    of text stands on end, and the Chinese came out one character per line. Link annotations of rotated pages are
    dropped."""
    pdf = pikepdf.open(deck)
    if not any(int(p.obj.get("/Rotate", 0)) % 360 for p in pdf.pages):
        return deck
    for page in pdf.pages:
        rot = int(page.obj.get("/Rotate", 0)) % 360
        if not rot:
            continue
        x0, y0, x1, y1 = [float(v) for v in page.mediabox]
        w, h = x1 - x0, y1 - y0
        # Displayed coordinates from unrotated ones, after moving the box's origin to 0,0 (PDF rotates clockwise).
        m = {90: [0, -1, 1, 0, 0, w], 180: [-1, 0, 0, -1, w, h], 270: [0, 1, -1, 0, h, 0]}[rot]
        ops = [([], pikepdf.Operator("q")), (m, pikepdf.Operator("cm")),
               ([1, 0, 0, 1, -x0, -y0], pikepdf.Operator("cm"))]
        ops += pikepdf.parse_content_stream(page) + [([], pikepdf.Operator("Q"))]
        page.Contents = pdf.make_stream(pikepdf.unparse_content_stream(ops))
        page.mediabox = [0, 0, h, w] if rot in (90, 270) else [0, 0, w, h]
        for key in ("/CropBox", "/TrimBox", "/BleedBox", "/ArtBox", "/Rotate", "/Annots"):
            if key in page.obj:
                del page.obj[key]
    os.makedirs(work, exist_ok=True)
    out = os.path.join(work, "upright.pdf")
    pdf.save(out)
    return out


def span_style(s):
    mono = bool(MONO_RE.search(s["font"]))
    bold = bool(s["flags"] & 16) or "bold" in s["font"].lower() or "black" in s["font"].lower()
    return {"size": round(s["size"], 1), "color": "#%06x" % s["color"], "bold": bold, "mono": mono}


def is_ocw_notice(page):
    """MIT OpenCourseWare's closing licence page, which the user wants dropped from the translation."""
    t = page.get_text()
    return "MIT OpenCourseWare" in t and "ocw.mit.edu/terms" in t


def unmapped_fonts(doc):
    """Names of the fonts whose text cannot be extracted: composite (Identity-H) fonts without a ToUnicode map,
    such as subset code and math fonts, whose glyphs extract as arbitrary characters."""
    mapped, unmapped = set(), set()
    for page in doc:
        for xref, _, ftype, basefont, _, enc in page.get_fonts():
            name = basefont.split("+", 1)[-1]
            ok = ftype != "Type0" or doc.xref_get_key(xref, "ToUnicode")[0] != "null"
            (mapped if ok else unmapped).add(name)
    return unmapped - mapped


def is_garbled(text, mono):
    """Text extracted without a Unicode mapping comes out as digit and punctuation soup ('23445#>', '!"#$%&'):
    under 30% letters, and for a non-code font also at least four characters with two of !"#$%&' among them, since
    real prose spans such as '(' or '= 36' are short or clean. Real code that trips this only becomes a picture of
    itself, which looks the same."""
    t = re.sub(r"\s", "", text)
    soup = len(t) >= 2 and len(re.findall(r"[A-Za-z]", t)) < 0.3 * len(t)
    return soup and (mono or (len(t) >= 4 and len(re.findall(r"[!\"#$%&']", t)) >= 2))


def is_picture(span, unmapped):
    """A run that cannot be re-typeset (an emoji glyph, text in an unmapped font, code that extracted as
    punctuation soup) travels through translation as an ⟦En⟧ token and is re-inserted as a picture cut from the
    page."""
    return "emoji" in span["font"].lower() or span["font"] in unmapped or is_garbled(span["text"], span_style(span)["mono"])


BULLET_FONT_RE = re.compile(r"wingding|dingbat|symbol", re.I)
BULLETS = "●•▪◦■–▸►"


def is_bullet(span, after=None):
    """A list bullet starting a line: a bullet character, or a lone symbol in a dingbat font or in a font other than
    the text after it (an unmapped bullet glyph extracts as '!')."""
    t = span["text"].strip()
    if not t:
        return False
    if t[0] in BULLETS:
        return True
    lone = len(t) <= 2 and not re.search(r"[A-Za-z0-9]", t)
    return lone and (BULLET_FONT_RE.search(span["font"]) is not None or (after is not None and after["font"] != span["font"]))


def style_tag(st, i):
    """The inline tag for a block's i-th non-dominant style: c<n> for code (kept verbatim), s<n> otherwise."""
    return ("c%d" if st["mono"] else "s%d") % (i + 1)


def split_columns(raw):
    """Text blocks with text, a block holding runs side by side on one baseline (two labels such as "client" and
    "server" over a diagram, as separate lines or one line with a wide gap) split into one block per column of
    runs."""
    out = []
    for b in raw:
        if b["type"] != 0 or not any(s["text"].strip() for l in b["lines"] for s in l["spans"]):
            continue
        runs = []
        for l in b["lines"]:
            spans = [s for s in l["spans"] if s["text"].strip()]
            cur = [spans[0]] if spans else []
            for s in spans[1:]:
                if s["bbox"][0] - cur[-1]["bbox"][2] > 2 * max(s["size"], cur[-1]["size"]):
                    runs.append(cur)
                    cur = [s]
                else:
                    cur.append(s)
            if cur:
                runs.append(cur)
        side_by_side = any(abs(a[0]["bbox"][1] - c[0]["bbox"][1]) < 0.5 * c[0]["size"] for a, c in zip(runs, runs[1:]))
        if side_by_side:
            columns = []  # runs grouped by overlapping horizontal extent, each group one block
            for r in runs:
                x0, x1 = min(s["bbox"][0] for s in r), max(s["bbox"][2] for s in r)
                col = next((c for c in columns if x0 < c["x1"] and x1 > c["x0"]), None)
                if col is None:
                    col = {"x0": x0, "x1": x1, "runs": []}
                    columns.append(col)
                col["x0"], col["x1"] = min(col["x0"], x0), max(col["x1"], x1)
                col["runs"].append(r)
            for col in columns:
                lines = [{"bbox": (min(s["bbox"][0] for s in r), min(s["bbox"][1] for s in r),
                                   max(s["bbox"][2] for s in r), max(s["bbox"][3] for s in r)), "spans": r}
                         for r in col["runs"]]
                bbox = (min(l["bbox"][0] for l in lines), min(l["bbox"][1] for l in lines),
                        max(l["bbox"][2] for l in lines), max(l["bbox"][3] for l in lines))
                out.append({"type": 0, "bbox": bbox, "lines": lines})
        else:
            out.append(b)
    return out


def paragraphs(page):
    """The page's text blocks, with blocks that continue one paragraph merged: Keynote and PowerPoint often export
    each line, or each run around an inline picture, as its own block. First a block that carries on the previous
    block's last line after a short gap joins that line; then a block in the same text size that starts right below
    the previous one (gap under 0.6 of the text size), at the same left edge or centre, and inside the same boxes,
    joins it as more lines."""
    def union(a, b):
        return (min(a[0], b[0]), min(a[1], b[1]), max(a[2], b[2]), max(a[3], b[3]))

    blocks = []
    for b in split_columns(page.get_text("dict")["blocks"]):
        sizes = Counter()  # the block's text size is the size most of its characters are set in
        for l in b["lines"]:
            for sp in l["spans"]:
                sizes[sp["size"]] += len(sp["text"].strip())
        size = sizes.most_common(1)[0][0]
        lines = [dict(l, spans=list(l["spans"])) for l in b["lines"]]
        if blocks:
            p = blocks[-1]
            last, first = p["lines"][-1], lines[0]
            if (abs(first["bbox"][1] - last["bbox"][1]) < size * 0.5
                    and -1 <= first["bbox"][0] - last["bbox"][2] < size * 2.5):
                last["spans"] += first["spans"]
                last["bbox"] = union(last["bbox"], first["bbox"])
                p["lines"] += lines[1:]
                p["bbox"] = union(p["bbox"], b["bbox"])
                p["size"] = max(p["size"], size)
                continue
        blocks.append({"bbox": tuple(b["bbox"]), "lines": lines, "size": size})
    W, H = page.rect.width, page.rect.height
    shapes = [d["rect"] for d in page.get_drawings() if d["rect"].width < 0.9 * W or d["rect"].height < 0.9 * H]

    def frames(box):
        """The shapes enclosing a line's centre: two lines in different boxes are different paragraphs."""
        c = pymupdf.Point((box[0] + box[2]) / 2, (box[1] + box[3]) / 2)
        return {k for k, r in enumerate(shapes) if r.contains(c)}

    out = []
    for b in blocks:
        if out:
            p = out[-1]
            last, first, size = p["lines"][-1]["bbox"], b["lines"][0]["bbox"], b["size"]
            aligned = abs(last[0] - first[0]) < size * 0.6 or abs((last[0] + last[2]) - (first[0] + first[2])) < size
            below = first[1] - last[1] > 0.5 * size and first[1] - last[3] < size * 0.6
            if abs(p["size"] - size) < 1 and aligned and below and frames(last) == frames(first):
                p["lines"] += b["lines"]
                p["bbox"] = union(p["bbox"], b["bbox"])
                continue
        out.append(b)
    return out


def lone_pictures(page):
    """Boxes of the page's real pictures: images of at least 2% of the page that touch no other image. Tiles of a
    background or texture and shadow strips abut each other, and are often transparent."""
    images = [pymupdf.Rect(i["bbox"]) & page.rect for i in page.get_image_info()]
    return [r for k, r in enumerate(images) if r.get_area() >= 0.02 * page.rect.get_area()
            and not any(j != k and (r + (-1, -1, 1, 1)).intersects(o) for j, o in enumerate(images))]


def figure_text(page, kept, prose, words):
    """Indexes of the prose blocks that belong to a figure, whose text stays in English. A figure is a cluster of at
    least three elements (vector shapes and code or symbol text blocks, each not part of the slide's prose) each
    within 2% of the page width of another. A short label (up to eight words) belongs to a figure when its centre
    lies in the figure's box grown by that gap, or 30% of its area does and it is under half the figure's width;
    a longer block only when its centre lies within that gap of one element, so a note set between two code boxes
    is not swallowed. A block lying on a picture that stands alone (not a background or texture tile) belongs to
    it too. A block that joins a figure becomes one of its elements, so a caption over a diagram's header is caught
    too."""
    W, H = page.rect.width, page.rect.height
    gap = 0.02 * W
    # A straight line has an empty box, which intersects nothing; pad every shape by 1 pt.
    # Shapes spanning most of the page's width or height (backgrounds, title bars, rules) are decoration.
    shapes = [pymupdf.Rect(d["rect"]) + (-1, -1, 1, 1) for d in page.get_drawings()
              if d["rect"].width <= 0.8 * W and d["rect"].height <= 0.8 * H]

    def belongs_to_text(r):
        """A shape that frames a prose block snugly (a title bar, a callout box: at most four times the block's
        area) or lies mostly inside one (outlined title glyphs, an underline) is part of the text, not of a figure.
        A large box holding a small label, such as a diagram node, stays a figure element."""
        for t in prose:
            common = (r & t).get_area()
            if (common >= 0.8 * t.get_area() and r.get_area() <= 4 * t.get_area()) or common >= 0.5 * r.get_area():
                return True
        return False

    def overlaps_text(r):
        return any(r.intersects(t) for t in prose)

    items = [r for r in shapes if not belongs_to_text(r)]
    items += [r for r in kept if not overlaps_text(r)]  # not the bullet column beside a list
    clusters = []  # member boxes of each cluster
    for r in items:
        near = [c for c in clusters if any((m + (-gap, -gap, gap, gap)).intersects(r) for m in c)]
        joined = [r] + [m for c in near for m in c]
        clusters = [c for c in clusters if c not in near] + [joined]
    figures = [c for c in clusters if len(c) >= 3]
    # Images do not join clusters: PowerPoint draws shadows and textured text boxes as image strips and tiles, which
    # would chain a whole slide into one figure. A label lying on a picture still counts as figure text.
    pictures = [r for r in lone_pictures(page) if r.get_area() <= 0.5 * W * H and not belongs_to_text(r)]
    if os.environ.get("SLIDES_DEBUG"):
        print("page", page.number + 1, "items", len(items), "figures",
              [tuple(round(v) for v in functools.reduce(lambda x, y: x | y, f)) for f in figures])
    out, grew = set(), True
    while grew:
        grew = False
        for i, r in enumerate(prose):
            if i in out:
                continue
            centre = pymupdf.Point((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2)
            if any(p.contains(centre) for p in pictures):
                out.add(i)
                continue
            for f in figures:
                if words[i] <= 8:  # a label: tested against the figure's whole box
                    whole = functools.reduce(lambda x, y: x | y, f)
                    box = whole + (-gap, -gap, gap, gap)
                    # Overlapping the edge counts only for a label narrower than half the figure; a subheading
                    # set right above a code box spans most of its width.
                    hit = box.contains(centre) or ((box & r).get_area() >= 0.3 * r.get_area()
                                                   and r.width <= 0.5 * whole.width)
                else:  # a sentence: only when it sits on an element
                    hit = any((m + (-gap, -gap, gap, gap)).contains(centre) for m in f)
                if hit:
                    out.add(i)
                    f.append(r)
                    grew = True
                    break
    return out


def covered_text(page, prose):
    """Indexes of the prose blocks an image is painted over, as when a build step lays a screenshot across a
    slide's bullets: re-inserted text would land on top of the image, so the block stays as it is. Uses the page's
    paint order: a picture painted after the block's text that covers 30% of the block. Shadow strips (thinner than
    1:6) and texture tiles (three or more images of one size) are often transparent, so they do not count."""
    log = page.get_bboxlog()
    images = [pymupdf.Rect(i["bbox"]) for i in page.get_image_info()]  # unclipped, so edge tiles keep their size
    sizes = Counter((round(r.width), round(r.height)) for r in images)
    pictures = [r & page.rect for r in images if (r & page.rect).get_area() >= 0.02 * page.rect.get_area()
                and max(r.width, r.height) <= 6 * min(r.width, r.height) and sizes[(round(r.width), round(r.height))] < 3]
    out = set()
    for i, r in enumerate(prose):
        drawn = [k for k, (kind, box) in enumerate(log) if kind.endswith("-text") and pymupdf.Rect(box).intersects(r)]
        if drawn and any(kind.endswith(("-image", "-imgmask")) and (pymupdf.Rect(box) & r).get_area() >= 0.3 * r.get_area()
                         and any(all(abs(a - b) < 1 for a, b in zip(pymupdf.Rect(box) & page.rect, p)) for p in pictures)
                         for kind, box in log[max(drawn) + 1:]):
            out.add(i)
    return out


def extract(deck, work):
    """Writes blocks.json: one entry per text block that holds prose, with its text marked up by style tags
    (<s1>…</s1>) relative to the block's dominant style, list items split by <br> and pictures as ⟦En⟧ tokens,
    its geometry, and its spans' boxes for redaction. figure-labels.json lists the blocks left in English as
    figure text, for review."""
    doc = pymupdf.open(upright(deck, work))
    blocks = []
    skipped, figure_labels = 0, []
    unmapped = unmapped_fonts(doc)
    for pno, page in enumerate(doc):
        if is_ocw_notice(page):
            continue
        prose, kept = [], []
        for b in paragraphs(page):
            box = pymupdf.Rect(b["bbox"])
            if not page.rect.contains(pymupdf.Point((box.x0 + box.x1) / 2, (box.y0 + box.y1) / 2)):
                continue  # text parked off the page is never seen
            spans = [(li, s) for li, l in enumerate(b["lines"]) for s in l["spans"] if s["text"].strip()]
            chars = Counter()
            for _, s in spans:
                if not is_picture(s, unmapped):
                    chars[json.dumps(span_style(s), sort_keys=True)] += len(s["text"].strip())
            mono = sum(len(s["text"].strip()) for _, s in spans if span_style(s)["mono"])
            if (mono >= 0.5 * sum(chars.values())  # a code listing stays as it is
                    or not re.search(r"[A-Za-z]{2}", "".join(s["text"] for _, s in spans))):  # numbers, symbols
                kept.append(pymupdf.Rect(b["bbox"]))
            else:
                prose.append((b, spans, chars))
        boxes = [pymupdf.Rect(b["bbox"]) for b, _, _ in prose]
        in_figure = figure_text(page, kept, boxes, [sum(len(s["text"].split()) for _, s in spans) for _, spans, _ in prose])
        in_figure |= covered_text(page, boxes)
        skipped += len(in_figure)
        figure_labels += [{"page": pno + 1, "bbox": [round(v, 1) for v in prose[i][0]["bbox"]],
                           "text": " ".join(s["text"] for _, s in prose[i][1]).strip()} for i in sorted(in_figure)]
        for i, (b, spans, chars) in enumerate(prose):
            if i in in_figure:
                continue
            base = json.loads(chars.most_common(1)[0][0])
            styles, pictures, text, last_line, picture_line = [], [], "", None, None
            for k, (li, s) in enumerate(spans):
                st = span_style(s)
                bullet = li != last_line and is_bullet(s, spans[k + 1][1] if k + 1 < len(spans) else None)
                if last_line is not None and li != last_line:
                    if bullet:
                        text = text.rstrip() + "<br>"  # a list keeps one item per line
                    elif not text.endswith((" ", "-")):
                        text += " "
                last_line = li
                if bullet and s["text"].strip()[0] not in BULLETS:
                    s = dict(s, text="•")  # an unmapped bullet glyph would come back as '!'
                    st = span_style(s)
                if is_picture(s, unmapped):
                    r = [round(v, 2) for v in s["bbox"]]
                    if text.endswith("⟦E%d⟧" % len(pictures)) and pictures and li == picture_line:
                        pictures[-1] = [min(pictures[-1][0], r[0]), min(pictures[-1][1], r[1]),
                                        max(pictures[-1][2], r[2]), max(pictures[-1][3], r[3])]
                    else:
                        pictures.append(r)
                        text += "⟦E%d⟧" % len(pictures)
                    picture_line = li
                    continue
                t = html.escape(s["text"], quote=False)
                if st != base:
                    if st not in styles:
                        styles.append(st)
                    tag = style_tag(st, styles.index(st))
                    text += "<%s>%s</%s>" % (tag, t, tag)
                else:
                    text += t
            text = re.sub(r"</([sc]\d+)>(\s*)<\1>", r"\2", text)  # merge adjacent runs of one style
            text = re.sub(r"\s+", " ", text).strip()
            lines = b["lines"]
            xs0 = [l["bbox"][0] for l in lines]
            xs1 = [l["bbox"][2] for l in lines]
            align = "left"
            if len(lines) > 1 and max(xs0) - min(xs0) > 4:
                mids = [(a + c) / 2 for a, c in zip(xs0, xs1)]
                if max(mids) - min(mids) < 6:
                    align = "center"
                elif max(xs1) - min(xs1) < 4:
                    align = "right"
            pitch = ((lines[-1]["bbox"][1] - lines[0]["bbox"][1]) / (len(lines) - 1)) if len(lines) > 1 else None
            blocks.append({
                "id": "p%02d-b%02d" % (pno + 1, len([x for x in blocks if x["page"] == pno + 1]) + 1),
                "page": pno + 1, "bbox": [round(v, 2) for v in b["bbox"]], "align": align,
                "pitch": round(pitch / base["size"], 3) if pitch else None,
                "base": base, "styles": {style_tag(st, i): st for i, st in enumerate(styles)},
                "text": text, "pictures": pictures,
                "redact": [[round(v, 2) for v in s["bbox"]] for _, s in spans],
            })
    os.makedirs(work, exist_ok=True)
    json.dump(blocks, open(os.path.join(work, "blocks.json"), "w"), ensure_ascii=False, indent=1)
    json.dump(figure_labels, open(os.path.join(work, "figure-labels.json"), "w"), ensure_ascii=False, indent=1)
    uniq = {b["text"] for b in blocks}
    print(f"{len(doc)} pages, {len(blocks)} blocks, {len(uniq)} unique texts, {skipped} figure labels left in "
          f"English -> {work}/blocks.json")


# ---------------------------------------------------------------- rendering

def font_files():
    """Heiti SC Light for regular text, Heiti SC Medium for bold and Menlo for code, cut out of the system
    collections once into the state dir: slides are set in a sans face (Helvetica Neue Light and Bold), and a serif
    regular beside a sans bold on one line looks mismatched."""
    from fontTools.ttLib import TTCollection
    os.makedirs(FONT_DIR, exist_ok=True)
    want = {"heiti-light.ttf": ("/System/Library/Fonts/STHeiti Light.ttc", "Heiti SC Light"),
            "heiti.ttf": ("/System/Library/Fonts/STHeiti Medium.ttc", "Heiti SC Medium"),
            "menlo.ttf": ("/System/Library/Fonts/Menlo.ttc", "Menlo Regular")}
    for name, (ttc, face) in want.items():
        dest = os.path.join(FONT_DIR, name)
        if not os.path.exists(dest):
            for f in TTCollection(ttc).fonts:
                if f["name"].getDebugName(4) == face:
                    f.save(dest)
                    break
    return FONT_DIR


def deck_fonts(work, texts):
    """The three fonts cut down to the characters this deck needs (texts, plus printable ASCII and the placeholder),
    written to <work>/fonts. insert_htmlbox embeds a whole font file at every call, so full CJK fonts (Heiti is
    55 MB) made a deck's document grow by gigabytes in memory before the final subsetting."""
    from fontTools import subset
    full = font_files()
    out = os.path.join(work, "fonts")
    os.makedirs(out, exist_ok=True)
    chars = set("".join(texts)) | {chr(c) for c in range(32, 127)} | set(PLACEHOLDER + "•")
    for name in ("heiti-light.ttf", "heiti.ttf", "menlo.ttf"):
        opts = subset.Options()
        opts.layout_features = ["*"]
        font = subset.load_font(os.path.join(full, name), opts)
        sub = subset.Subsetter(opts)
        sub.populate(unicodes=[ord(c) for c in chars])
        sub.subset(font)
        subset.save_font(font, os.path.join(out, name), opts)
    return out


def background(page, rect):
    """The colour behind a block once its text is gone: the most common pixel colour in its box."""
    pix = page.get_pixmap(clip=rect, matrix=pymupdf.Matrix(0.5, 0.5))
    n = pix.n
    counts = Counter(tuple(pix.samples[i:i + 3]) for i in range(0, len(pix.samples), n))
    return tuple(c / 255 for c in counts.most_common(1)[0][0]) if counts else rgb01(BG)


def luminance(rgb):
    lin = [c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4 for c in rgb]
    return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]


def contrast(a, b):
    """WCAG contrast ratio of two RGB colours in 0..1."""
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


LIGHT_INK = "#d6dedb"  # for text left on a coloured shape, such as a label inside a filled circle


def css_color(hexcolor, bold, bg):
    """The theme colour for a text run on background bg (RGB 0..1, sampled from the page): neutral ink of any shade
    takes the body or bold tone (boxes behind text are gone, so white text needs ink too), colours go through
    map_color. When that reads poorly on bg (contrast under 3, as on a mid-blue shape), a coloured ink is lightened in
    its own hue, and failing that the most legible of the theme inks, a light ink and the page colour is used."""
    rgb = rgb01(hexcolor)
    if max(rgb) - min(rgb) < 0.12:
        ink = BOLD if bold else FG
    else:
        ink = hex_of(map_color(rgb))
    if contrast(rgb01(ink), bg) < 3 and max(rgb) - min(rgb) >= 0.12:
        # A coloured run (syntax highlighting, a highlighted word) first gets lighter in its own hue.
        h, l, sat = colorsys.rgb_to_hls(*rgb01(ink))
        while l < 0.85 and contrast(colorsys.hls_to_rgb(h, l, sat), bg) < 3:
            l += 0.05
        ink = hex_of(colorsys.hls_to_rgb(h, l, sat))
    if contrast(rgb01(ink), bg) < 3:
        ink = max([ink, FG, BOLD, LIGHT_INK, BG], key=lambda c: contrast(rgb01(c), bg))
    return ink


def run_css(st, bg):
    fam = "mono" if st["mono"] else ("hei" if st["bold"] else "light")
    return f"font-family: {fam}; font-size: {st['size']}pt; color: {css_color(st['color'], st['bold'], bg)}; font-weight: normal;"


PLACEHOLDER = "〓"
CJK = "　-〿㐀-鿿＀-￯"

CSS = """
@font-face { font-family: light; src: url(heiti-light.ttf); }
@font-face { font-family: hei; src: url(heiti.ttf); }
@font-face { font-family: mono; src: url(menlo.ttf); }
* { margin: 0; padding: 0; }
"""


def placeholders(block, n):
    """How many 1-em placeholder glyphs stand in for the block's n-th picture."""
    r = block["pictures"][n - 1]
    return max(1, round((r[2] - r[0]) / block["base"]["size"]))


def block_html(block, zh, bg):
    base = block["base"]

    # MuPDF breaks a line after any image, so a picture is laid out as invisible placeholder glyphs (PLACEHOLDER in
    # the background colour, one per em of its width) that follow Chinese line-break rules; render() draws it over.
    def holder(m):
        n = placeholders(block, int(m.group(1)))
        return f'<span style="font-family: light; color: {hex_of(bg)};">{PLACEHOLDER * n}</span>'
    zh = re.sub(r"⟦E(\d+)⟧", holder, zh)

    def sub(m):
        tag, inner = m.group(1), m.group(2)
        st = block["styles"].get(tag)
        return f'<span style="{run_css(st, bg)}">{inner}</span>' if st else inner
    # A space between two Chinese characters is an English leftover, often at a style boundary ("复杂性 </s1>限制").
    zh = re.sub(rf"(?<=[{CJK}])((?:\s|</?[sc]\d+>)+)(?=[{CJK}])", lambda m: re.sub(r"\s", "", m.group(1)), zh)
    body = re.sub(r"<([sc]\d+)>(.*?)</\1>", sub, zh, flags=re.S)
    lh = min(max(block["pitch"] or 1.25, 1.15), 1.5)
    return (f'<div style="{run_css(base, bg)} text-align: {block["align"]}; line-height: {lh};">'
            f"{body}</div>")


def low_contrast(page, unmapped):
    """The finished page's text spans whose ink has contrast under 3 against what is behind them, for the report."""
    out = []
    for b in page.get_text("dict")["blocks"]:
        for l in b.get("lines", []):
            for sp in l["spans"]:
                r = pymupdf.Rect(sp["bbox"])
                t = sp["text"].strip()
                if t and not r.is_empty and not is_picture(sp, unmapped) and t != PLACEHOLDER * len(t) \
                        and contrast(rgb01("#%06x" % sp["color"]), background(page, r)) < 3:
                    out.append(t[:20])
    return out


def legible_labels(page, translated, arch, unmapped):
    """Re-inks the text left in place (figure labels, names) wherever its recoloured ink reads poorly on what is
    behind it (contrast under 3, as with a label on a blue box): the span is redacted and set again in the same
    style family with the most legible ink (css_color). Spans of translated blocks (translated: their boxes),
    pictures, unmapped text and code are left alone: PowerPoint code fonts often extract a few wrong characters
    ('{' as '.'), so re-typesetting code would corrupt it. Returns how many spans were re-inked."""
    fix = []
    for b in page.get_text("dict")["blocks"]:
        for l in b.get("lines", []):
            for sp in l["spans"]:
                r = pymupdf.Rect(sp["bbox"])
                if (not sp["text"].strip() or r.is_empty or is_picture(sp, unmapped) or span_style(sp)["mono"]
                        or any(r.intersects(t) for t in translated)):
                    continue
                bg = background(page, r)
                if contrast(rgb01("#%06x" % sp["color"]), bg) >= 3:
                    continue
                fix.append((r, sp, bg))
    if not fix:
        return 0
    for r, sp, bg in fix:
        page.add_redact_annot(r, fill=False)
    page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_NONE, graphics=pymupdf.PDF_REDACT_LINE_ART_NONE,
                          text=pymupdf.PDF_REDACT_TEXT_REMOVE)
    for r, sp, bg in fix:
        st = span_style(sp)
        page.insert_htmlbox(r + (0, 0, sp["size"], sp["size"] * 0.3),
                            f'<div style="{run_css(st, bg)} white-space: nowrap;">{html.escape(sp["text"], quote=False)}</div>',
                            css=CSS, archive=arch, scale_low=0)
    return len(fix)


def render(deck, work, out):
    blocks = json.load(open(os.path.join(work, "blocks.json")))
    zh = json.load(open(os.path.join(work, "zh.json")))
    deck = upright(deck, work)
    tmp = os.path.join(work, "recolored.pdf")
    recolor_pdf(deck, tmp)
    doc = pymupdf.open(tmp)
    todo = [b for b in blocks if zh.get(b["text"]) and zh[b["text"]].strip() != b["text"].strip()]
    # Labels re-inked by legible_labels come from the source text, so the source's characters are included too.
    arch = pymupdf.Archive(deck_fonts(work, [zh[b["text"]] for b in todo] + [pymupdf.open(deck)[i].get_text()
                                                                         for i in range(len(doc))]))
    report = []
    backed = {i for i, p in enumerate(doc) if picture_backed(p)}
    print(f"recoloured {recolor_images(doc, backed)} screenshot and diagram images")
    pics = os.path.join(work, "pictures")
    os.makedirs(pics, exist_ok=True)
    unmapped = unmapped_fonts(pymupdf.open(deck))
    for pno, page in enumerate(doc):
        mine = [b for b in todo if b["page"] == pno + 1]
        relit = legible_labels(page, [pymupdf.Rect(r) for b in mine for r in b["redact"]], arch, unmapped)
        if relit:
            report.append(f"p{pno + 1:02d} {relit} labels re-inked for contrast")
        if not mine:
            continue
        links = page.get_links()  # redaction drops the links it overlaps; they are put back below
        # Keynote draws an emoji as a small image over an invisible glyph: cut each picture out for re-insertion,
        # then remove such an image, unless a larger image overlaps the spot (a removal would take it too).
        images = [pymupdf.Rect(i["bbox"]) for i in page.get_image_info()]
        emoji = False
        for b in mine:
            for n, r in enumerate(b["pictures"], 1):
                r = pymupdf.Rect(r)
                page.get_pixmap(clip=r, dpi=216).save(os.path.join(pics, f"{b['id']}-E{n}.png"))
                hits = [i for i in images if i.intersects(r)]
                if hits and all((r + (-2, -2, 2, 2)).contains(i) for i in hits):
                    page.add_redact_annot(r, fill=False)
                    emoji = True
        if emoji:
            page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_REMOVE, graphics=pymupdf.PDF_REDACT_LINE_ART_NONE)
        # PowerPoint WordArt titles carry their glyphs a second time as outline paths; remove line art lying
        # wholly inside a translated block's box, grown by 0.3 em since outlines overshoot the text box.
        for b in mine:
            g = 0.3 * b["base"]["size"]
            page.add_redact_annot(pymupdf.Rect(b["bbox"]) + (-g, -g, g, g), fill=False)
        page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_NONE,
                              graphics=pymupdf.PDF_REDACT_LINE_ART_REMOVE_IF_COVERED, text=pymupdf.PDF_REDACT_TEXT_NONE)
        for b in mine:
            for r in b["redact"]:
                rr = pymupdf.Rect(r)
                pad = rr.height * 0.15
                page.add_redact_annot(pymupdf.Rect(rr.x0, rr.y0 + pad, rr.x1, rr.y1 - pad))
        page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_NONE, graphics=pymupdf.PDF_REDACT_LINE_ART_NONE,
                              text=pymupdf.PDF_REDACT_TEXT_REMOVE)
        for b in mine:
            r = pymupdf.Rect(b["bbox"])
            # Room to grow: a little wider and one line taller than the English block, clipped to the page.
            grow = b["base"]["size"] * 0.6
            box = pymupdf.Rect(r.x0, r.y0, r.x1 + grow, r.y1 + grow) & page.rect
            spare, scale = page.insert_htmlbox(box, block_html(b, zh[b["text"]], background(page, r)), css=CSS,
                                               archive=arch, scale_low=0)
            order = [int(n) for n in re.findall(r"⟦E(\d+)⟧", zh[b["text"]])]
            used = []  # each picture takes the first unused match of its own run of placeholders
            for n in order:
                hits = [h for h in page.search_for(PLACEHOLDER * placeholders(b, n), clip=box)
                        if not any(h.intersects(u) for u in used)]
                if not hits:
                    report.append(f"{b['id']} picture {n} not placed")
                    continue
                used.append(hits[0])
                page.insert_image(hits[0], filename=os.path.join(pics, f"{b['id']}-E{n}.png"))
            if spare < 0 or scale < 0.999:
                report.append(f"{b['id']} shrunk to {scale:.2f}")
        kept = {(l.get("uri"), l.get("page"), tuple(round(v) for v in l["from"])) for l in page.get_links()}
        for l in links:
            if (l.get("uri"), l.get("page"), tuple(round(v) for v in l["from"])) not in kept:
                page.insert_link(l)
    for pno, page in enumerate(doc):
        dim = low_contrast(page, unmapped)
        if dim:
            report.append(f"p{pno + 1:02d} still low contrast: " + "; ".join(dim[:3]))
    src = pymupdf.open(deck)
    for pno in reversed([i for i, p in enumerate(src) if is_ocw_notice(p)]):
        doc.delete_page(pno)
    # Every insert_htmlbox embeds whole font files; subsetting cuts them to the glyphs used, and garbage=4 merges
    # the identical copies that remain.
    doc.subset_fonts()
    doc.save(out, garbage=4, deflate=True)
    print(f"wrote {out}; {len(todo)} blocks replaced")
    for line in report:
        print("  ", line)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "extract" and len(sys.argv) == 4:
        extract(sys.argv[2], sys.argv[3])
    elif cmd == "render" and len(sys.argv) == 5:
        render(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit("usage: slides.py extract <deck.pdf> <work dir> | render <deck.pdf> <work dir> <out.pdf>")
