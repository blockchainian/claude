# ABOUTME: Turns ```chart blocks in a chapter's Markdown into figures for the typeset PDF: a line chart for a
# ABOUTME: dated series, a bar chart for a comparison, or a table. Drawn as inline SVG in the page's text colour.
#
# A block:
#   ```chart
#   type: line                  (line | bar | table)
#   title: Subscribers          (shown above the figure)
#   scale: log                  (optional, line only: when the values span several orders of magnitude)
#   columns: Date | A | B       (optional: names the series of a line or bar chart; the header of a table)
#   2012-07-11 | 1,002,877 | 110,010
#   2013-07-09 | 10,058,670 | 681,229
#   ```
# The first cell of a row is its label (a date for a line chart: YYYY, YYYY-MM or YYYY-MM-DD); the others are the
# values (digits, with an optional unit K, M, B, 万 or 亿), printed in the figure exactly as written.
import html
import math
import re
import sys
from typing import NoReturn

BLOCK = re.compile(r"^```chart[ \t]*\n(.*?)\n```[ \t]*$", re.S | re.M)
KEYS = ("type", "title", "scale", "columns")
DATE = re.compile(r"^(\d{4})(?:-(\d{1,2}))?(?:-(\d{1,2}))?$")
FIGURE = re.compile(r"^(\d[\d,]*(?:\.\d+)?)\s*(K|M|B|万|亿)?$")
UNITS = {"": 1, "K": 1e3, "M": 1e6, "B": 1e9, "万": 1e4, "亿": 1e8}
W, LEFT, RIGHT, TOP, BOTTOM, FONT = 320, 44, 14, 12, 20, 6.5
DASHES = ["", "4 2", "1 2"]  # how the first, second and third series are told apart without colour

CSS = """
.chart { margin: 12pt 0; break-inside: avoid; text-align: center; }
.chart figcaption { font-family: "PingFang SC", "Heiti SC", sans-serif; font-weight: 700; font-size: 8.5pt; margin-bottom: 4pt; }
.chart svg { width: 100%; height: auto; color: inherit; }
.chart table { margin: 0 auto; border-collapse: collapse; font-size: 8.5pt; line-height: 1.5; }
.chart th, .chart td { padding: 2pt 8pt; text-align: left; border-bottom: 0.4pt solid currentColor; }
.chart th { font-family: "PingFang SC", "Heiti SC", sans-serif; font-weight: 700; border-bottom-width: 0.8pt; }
.chart td.n { text-align: right; font-variant-numeric: tabular-nums; }
"""


def stop(block, why) -> NoReturn:
    sys.exit(f"chart block cannot be drawn ({why}):\n{block}")


def parse(block):
    """A block's text -> (settings, rows of cells)."""
    settings, rows = {}, []
    for line in block.strip().splitlines():
        key, _, value = line.partition(":")
        if not rows and key.strip() in KEYS:
            settings[key.strip()] = value.strip()
        elif line.strip():
            rows.append([cell.strip() for cell in line.split("|")])
    if settings.get("type") not in ("line", "bar", "table"):
        stop(block, "type must be line, bar or table")
    if not rows:
        stop(block, "no rows")
    if len({len(row) for row in rows}) != 1 or len(rows[0]) < 2:
        stop(block, "every row needs a label and the same number of values")
    return settings, rows


def value(cell, block):
    """A cell as a number: digits with optional thousands separators and a unit (K, M, B, 万, 亿)."""
    m = FIGURE.match(cell)
    if not m:
        stop(block, f"'{cell}' is not a number")
    return float(m.group(1).replace(",", "")) * UNITS[m.group(2) or ""]


def moment(label):
    """A date label as a fractional year, or None when the label is not a date."""
    m = DATE.match(label)
    if not m:
        return None
    return int(m.group(1)) + (int(m.group(2) or 1) - 1) / 12 + (int(m.group(3) or 1) - 1) / 365


def short(v):
    """An axis figure: 1,500,000 as 1.5M."""
    for size, unit in ((1e9, "B"), (1e6, "M"), (1e3, "K")):
        if abs(v) >= size:
            return f"{v / size:.3g}{unit}"
    return f"{v:.3g}"


def ticks(top):
    """Even steps of a round size from zero to the first one at or above `top`, five at most."""
    unit = 10 ** math.floor(math.log10(top / 5 or 1))
    step = next(unit * m for m in (1, 2, 2.5, 5, 10) if math.ceil(top / (unit * m)) <= 5)
    return [step * i for i in range(math.ceil(top / step) + 1)]


def text(x, y, s, anchor="middle"):
    return f'<text x="{x:.1f}" y="{y:.1f}" text-anchor="{anchor}" font-size="{FONT}" fill="currentColor">{html.escape(s)}</text>'


def width(s):
    """A rough printed width of a label: a CJK character is a full square, a Latin one about two thirds."""
    return sum(FONT if ord(c) > 255 else FONT * 0.68 for c in s)


def legend(names):
    if len(names) < 2:
        return "", 0
    parts, x = [], LEFT
    for i, name in enumerate(names):
        parts.append(f'<line x1="{x}" y1="{TOP - 2}" x2="{x + 16}" y2="{TOP - 2}" stroke="currentColor" stroke-width="1.2" stroke-dasharray="{DASHES[i % 3]}"/>')
        parts.append(text(x + 19, TOP, name, "start"))
        x += 24 + width(name) + 10
    return "".join(parts), 12


def line_chart(settings, rows, block):
    names = [c.strip() for c in settings.get("columns", "").split("|")][1:] if settings.get("columns") else []
    series = [[value(row[i], block) for row in rows] for i in range(1, len(rows[0]))]
    moments = [moment(row[0]) for row in rows]
    dated = all(x is not None for x in moments)
    # labels that are not dates are spaced evenly
    xs = [float(x) for x in moments if x is not None] if dated else [float(i) for i in range(len(rows))]
    log = settings.get("scale") == "log"
    flat = [v for s in series for v in s]
    if log and min(flat) <= 0:
        stop(block, "a log scale needs values above zero")
    key, top = legend(names)
    h = 150 + top
    x0, x1 = min(xs), max(xs) if max(xs) > min(xs) else min(xs) + 1
    px = lambda x: LEFT + (x - x0) / (x1 - x0) * (W - LEFT - RIGHT)
    if log:
        lo, hi = math.floor(math.log10(min(flat))), math.ceil(math.log10(max(flat)))
        hi = hi if hi > lo else lo + 1
        py = lambda v: h - BOTTOM - (math.log10(v) - lo) / (hi - lo) * (h - BOTTOM - TOP - top)
        marks = [10 ** e for e in range(lo, hi + 1)]
    else:
        marks = ticks(max(flat))
        py = lambda v: h - BOTTOM - v / marks[-1] * (h - BOTTOM - TOP - top)
    out = [key]
    for m in marks:
        out.append(f'<line x1="{LEFT}" y1="{py(m):.1f}" x2="{W - RIGHT}" y2="{py(m):.1f}" stroke="currentColor" stroke-width="0.3" opacity="0.4"/>')
        out.append(text(LEFT - 4, py(m) + 2, short(m), "end"))
    if dated and x1 - x0 >= 2:
        every = math.ceil((x1 - x0) / 7)
        for year in range(math.ceil(x0), math.floor(x1) + 1, every):
            out.append(text(px(year), h - BOTTOM + 10, str(year)))
    else:
        for i in sorted({0, len(rows) - 1}) if len(rows) > 6 else range(len(rows)):
            out.append(text(px(xs[i]), h - BOTTOM + 10, rows[i][0]))
    taken = []  # the boxes of the value labels already placed

    def label(x, y, s, anchor):
        """The value next to its point: above it, else below it, else left out when both places are taken."""
        w = width(s)
        left = x if anchor == "start" else x - w if anchor == "end" else x - w / 2
        for base in (y - 4, y + FONT + 3):
            box = (left, base - FONT, left + w, base)
            if TOP + top <= box[1] and box[3] <= h - BOTTOM + 1 and not any(box[0] < b[2] and b[0] < box[2] and box[1] < b[3] and b[1] < box[3] for b in taken):
                taken.append(box)
                return text(x, base, s, anchor)
        return ""

    for n, s in enumerate(series):
        points = " ".join(f"{px(x):.1f},{py(v):.1f}" for x, v in zip(xs, s))
        out.append(f'<polyline points="{points}" fill="none" stroke="currentColor" stroke-width="1.2" stroke-dasharray="{DASHES[n % 3]}"/>')
        # the last, the first and the highest value are placed first, then the others where there is room
        for i in sorted(range(len(s)), key=lambda i: (i != len(s) - 1, i != 0, -s[i])):
            anchor = "start" if px(xs[i]) < LEFT + 30 else "end" if px(xs[i]) > W - RIGHT - 30 else "middle"
            placed = label(px(xs[i]), py(s[i]), rows[i][n + 1], anchor)
            if placed:
                out.append(f'<circle cx="{px(xs[i]):.1f}" cy="{py(s[i]):.1f}" r="1.6" fill="currentColor"/>' + placed)
    return f'<svg viewBox="0 0 {W} {h}" xmlns="http://www.w3.org/2000/svg">{"".join(out)}</svg>'


def bar_chart(settings, rows, block):
    names = [c.strip() for c in settings.get("columns", "").split("|")][1:] if settings.get("columns") else []
    values = [[value(cell, block) for cell in row[1:]] for row in rows]
    top = max(v for row in values for v in row)
    if top <= 0:
        stop(block, "a bar chart needs a value above zero")
    key, head = legend(names)
    room = W - RIGHT - 70  # the widest bar leaves room for its value
    out, y = [key], TOP + head
    for row, vals in zip(rows, values):
        out.append(text(0, y, row[0], "start"))  # the label sits on its own line, so a long one is never cut
        y += 4
        for n, v in enumerate(vals):
            w = max(v / top * room, 0.5)
            out.append(f'<rect x="0" y="{y:.1f}" width="{w:.1f}" height="7" fill="currentColor" opacity="{0.85 if n == 0 else 0.45}"/>')
            out.append(text(w + 4, y + 6, row[n + 1], "start"))
            y += 9
        y += 11
    return f'<svg viewBox="0 0 {W} {y + 2:.0f}" xmlns="http://www.w3.org/2000/svg">{"".join(out)}</svg>'


def table(settings, rows):
    head = ""
    if settings.get("columns"):
        head = "<tr>" + "".join(f"<th>{html.escape(c.strip())}</th>" for c in settings["columns"].split("|")) + "</tr>"
    numeric = re.compile(r"^[\d,.\s%$€£¥+-]+[KMB万亿]?$")
    body = "".join("<tr>" + "".join(f'<td class="n">{html.escape(c)}</td>' if i and numeric.match(c) else f"<td>{html.escape(c)}</td>"
                                    for i, c in enumerate(row)) + "</tr>" for row in rows)
    return f"<table>{head}{body}</table>"


def figure(block):
    settings, rows = parse(block)
    kind = settings["type"]
    drawn = table(settings, rows) if kind == "table" else line_chart(settings, rows, block) if kind == "line" else bar_chart(settings, rows, block)
    caption = f"<figcaption>{html.escape(settings['title'])}</figcaption>" if settings.get("title") else ""
    return f'<figure class="chart">{caption}{drawn}</figure>'


def figures(md_text):
    """The Markdown with every ```chart block replaced by its figure, as one line of HTML Markdown leaves alone."""
    return BLOCK.sub(lambda m: figure(m.group(1)), md_text)
