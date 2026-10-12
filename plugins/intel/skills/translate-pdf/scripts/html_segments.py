#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["beautifulsoup4>=4.12", "lxml>=5"]
# ///
# ABOUTME: Splits a fetched HTML page into translatable text units and joins their translations back into the page.
# ABOUTME: Strips what the print hides or what belongs to the website, so none of it is paid for.
#
# Usage: html_segments.py split <page.html> <segments.json> <skeleton.html> [--drop-heading TEXT ...]
#        html_segments.py join <skeleton.html> <translations.json> <out.html>
# First removes what html2pdf.py hides anyway (navigation, page chrome, exercise answers), what belongs to the
# website rather than the document (footers, licence badges), and each section whose heading contains a
# --drop-heading text (case-insensitive), up to the next heading of its level or higher. A unit is a block holding text but no nested block, or a run of loose text
# beside nested blocks (wrapped in a span). Code, preformatted text, scripts, SVG and math are never units. Inside a
# unit, each tag's attributes are swapped for k="n" so the translator sees short tags; join swaps them back. Code
# text and TeX math must come back verbatim.
import argparse, copy, hashlib, json, re
from pathlib import Path
from bs4 import BeautifulSoup, Comment, NavigableString, Tag

BLOCK = set("address article aside blockquote body dd details dialog div dl dt fieldset figcaption figure footer form "
            "h1 h2 h3 h4 h5 h6 header hr li main nav ol p pre section summary table tbody thead tfoot tr td th ul "
            "caption select option".split())
# codeblock is the code-block tag some course sites use in place of pre.
SKIP = set("pre codeblock script style noscript svg math template textarea head".split())
CODE = {"code", "kbd", "samp", "tt"}
HIDDEN = ("nav, .navbar, .sidebar, #sidebar, .toc-sidebar, iframe, .margin, .process, input, button, textarea, "
          ".exercise-answer, .exercise-explain, .form-inline, .exercise-progress, .exercise-error")
LETTERS = re.compile(r"[A-Za-z]")
# TeX math a page typesets with MathJax: \begin{env}..\end{env}, $$..$$, $..$, \(..\), \[..\]; an escaped \$ is a dollar sign.
MATH = re.compile(r"\\begin\{([a-z]+\*?)\}.*?\\end\{\1\}|\$\$.+?\$\$|(?<!\\)\$(?:\\.|[^$\\])+?\$|\\\(.+?\\\)|\\\[.+?\\\]", re.S)
DOLLAR = r"|(?<!\\)\$(?:\\.|[^$\\])+?\$"
MATH_NO_DOLLAR = re.compile(MATH.pattern.replace(DOLLAR, ""), re.S)


def seg_id(text):
    """A unit's id is a hash of its text, so saved translations stay valid when the page or the splitting changes."""
    return "u" + hashlib.sha1(text.encode()).hexdigest()[:12]


def skipped(t):
    return t.name in SKIP or "mermaid" in (t.get("class") or [])


def has_block(t):
    return any(d.name in BLOCK or skipped(d) for d in t.find_all(True))


def prose(nodes):
    """True when the nodes carry letters outside code."""
    for n in nodes:
        if isinstance(n, NavigableString):
            if LETTERS.search(n):
                return True
        elif n.name not in CODE and prose(n.contents):
            return True
    return False


def strip_chrome(soup, drop=()):
    for c in soup.find_all(string=lambda s: isinstance(s, Comment)):
        c.extract()
    for t in soup.select(HIDDEN):
        t.decompose()
    for h in soup.find_all("header"):
        if not h.find(["h1", "h2"]):
            h.decompose()
    # Page footers and licence badges belong to the website, not the document.
    for f in soup.find_all("footer"):
        f.decompose()
    for img in soup.find_all("img"):
        link = img.find_parent("a")
        target = f'{img.get("alt", "")} {img.get("src", "")} {link.get("href", "") if link else ""}'
        if img.parent is not None and re.search(r"creative ?commons|\bcc-by", target, re.I):
            (link if link and not link.get_text(strip=True) else img).decompose()
    for h in soup.find_all(re.compile(r"^h[1-6]$")):
        if h.parent is None or not any(d.lower() in h.get_text().lower() for d in drop):
            continue
        level, n = int(h.name[1]), h.find_next_sibling()
        while n and not (re.fullmatch(r"h[1-6]", n.name) and int(n.name[1]) <= level):
            nxt = n.find_next_sibling(); n.decompose(); n = nxt
        h.decompose()


def units(t, soup, out):
    """Collects the translatable units under t in document order, marking each with data-tid."""
    if skipped(t):
        return
    if not has_block(t):
        if t.name != "body" and prose(t.contents):
            t["data-tid"] = f"u{len(out)}"; out.append(t)
        return
    run = []
    def flush():
        if prose(run):
            span = soup.new_tag("span"); span["data-tid"] = f"u{len(out)}"
            run[0].insert_before(span)
            for n in run:
                span.append(n.extract())
            out.append(span)
        run.clear()
    for c in list(t.contents):
        if isinstance(c, Tag) and (c.name in BLOCK or skipped(c) or has_block(c)):
            flush(); units(c, soup, out)
        else:
            run.append(c)
    flush()


def shorthand(unit, math, dollar):
    """The unit's inner HTML with each tag's attributes replaced by k="n", and what the translation must keep: each
    attributed tag and each code element whole; and, on a page that typesets TeX (math), its formulas."""
    u = copy.copy(unit)
    keep = []
    for i, d in enumerate(u.find_all(True)):
        if d.attrs:
            d.attrs = {"k": str(i)}; keep.append(f'k="{i}"')
    keep += [str(d) for d in u.find_all(list(CODE))]
    text = u.decode_contents().strip()
    formulas = [m.group(0) for m in (MATH if dollar else MATH_NO_DOLLAR).finditer(text)] if math else []
    return text, keep, formulas


def split(src, out, skeleton, drop=()):
    soup = BeautifulSoup(Path(src).read_text(errors="ignore"), "lxml")
    # Only a page that loads MathJax or KaTeX has TeX math; elsewhere a $ is a dollar sign.
    math = bool(soup.find("script", src=re.compile("mathjax|katex", re.I)))
    # $..$ is inline math only where the page configures it (MathJax and KaTeX default to \(..\)); otherwise $ is money.
    dollar = math and bool(re.search(r"""\[\s*['"]\$['"]\s*,\s*['"]\$['"]\s*\]""", str(soup)))
    strip_chrome(soup, drop)
    found = []
    units(soup.body, soup, found)
    segs = {}
    for u in found:
        text, keep, formulas = shorthand(u, math, dollar)
        u["data-tid"] = seg_id(text)
        segs[u["data-tid"]] = {"id": u["data-tid"], "text": text, "keep": keep, "math": formulas, "dollar": dollar}
    segs = list(segs.values())
    Path(out).write_text(json.dumps(segs, ensure_ascii=False, indent=1))
    Path(skeleton).write_text(str(soup))
    print(f"{len(segs)} units, {sum(len(LETTERS.findall(s['text'])) for s in segs) // 5} words approx")


def join(skeleton, translations, dest):
    soup = BeautifulSoup(Path(skeleton).read_text(), "lxml")
    zh = json.loads(Path(translations).read_text())
    missing = 0
    for u in soup.select("[data-tid]"):
        tid = u["data-tid"]; del u["data-tid"]
        if tid not in zh:
            missing += 1; continue
        attrs = {str(i): d.attrs for i, d in enumerate(u.find_all(True)) if d.attrs}
        frag = BeautifulSoup(f"<div>{zh[tid]}</div>", "html.parser").div
        for d in frag.find_all(True):
            k = d.attrs.pop("k", None)
            if k in attrs:
                d.attrs = dict(attrs[k])
        u.clear()
        for n in list(frag.contents):
            u.append(n.extract())
    Path(dest).write_text(str(soup))
    print(f"joined, {missing} units left in English")


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description="Split an HTML page into translation units, or join translations back.")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sp = sub.add_parser("split")
    sp.add_argument("page"); sp.add_argument("segments"); sp.add_argument("skeleton")
    sp.add_argument("--drop-heading", action="append", default=[], help="drop each section whose heading contains this")
    jp = sub.add_parser("join")
    jp.add_argument("skeleton"); jp.add_argument("translations"); jp.add_argument("out")
    a = ap.parse_args()
    if a.cmd == "split":
        split(a.page, a.segments, a.skeleton, a.drop_heading)
    else:
        join(a.skeleton, a.translations, a.out)
