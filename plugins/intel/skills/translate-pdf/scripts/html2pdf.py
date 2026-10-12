#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pymupdf>=1.24", "websocket-client>=1.7", "pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Prints one Chinese-translated HTML page to <page>.pdf in the translate-epub skill's book look: dark page, folio,
# ABOUTME: Baskerville/Songti body, 黑体 bold, boxed Menlo code, page chrome hidden and wide content fitted.
#
# Usage: html2pdf.py <page.html> [--base URL] [--skip-bookmark TEXT ...]
# --base resolves the page's relative links and images; --skip-bookmark drops outline entries with that exact title
# (case-insensitive), such as a heading every exercise block repeats. Typography: 9.25pt body at line-height 1.8, 黑体
# bold for bold, emphasis and headings, no italic, paragraphs indented 2em, Latin words hyphenated as English.
import argparse, re, shutil, subprocess, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent / "translate-epub" / "scripts"))
from render import chrome_binary  # noqa: E402

W, H = 427.92, 660  # the reference book traction.pdf
BG, FG, BOLD = "#000409", "#6e7f7a", "#8a9f98"
LINK = "#005f87"  # nvim dark-mode Accent (cterm 24), the color it gives help-page links
TOP, SIDE, BOTTOM = round(H * 0.082, 1), round(W * 0.135, 1), round(H * 0.068, 1)
SERIF = 'Baskerville, "Songti SC", serif'
HEI = 'Baskerville, "PingFang SC", "Heiti SC", sans-serif'
BODY, LEAD = 9.25, 1.8
CSS = f"""<style>
@page {{ size: {W}pt {H}pt; margin: {TOP}pt {SIDE}pt {BOTTOM}pt; background: {BG};
        @bottom-center {{ content: counter(page); font-size: 8pt; color: {FG}; font-family: {SERIF}; }} }}
* {{ color: {FG} !important; background-color: transparent !important; background-image: none !important;
     border-color: #1d2a2a !important; box-shadow: none !important; text-shadow: none !important;
     -webkit-print-color-adjust: exact; print-color-adjust: exact; }}
html {{ background: {BG} !important; }}
body {{ margin: 0 !important; padding: 0 !important; max-width: none !important; width: auto !important;
        font-family: {SERIF} !important; font-size: {BODY}pt !important; line-height: {LEAD} !important; overflow-wrap: break-word; }}
p, li, td, th, dd, dt, blockquote, div, span, a {{ font-family: {SERIF} !important; }}
p, li, td, th, dd, dt, blockquote {{ font-size: 1em !important; }}
p, li, dd, blockquote {{ text-align: justify !important; hyphens: auto; }}
/* Some pages leave body text loose in a div (paragraphs split by <p/>): give it the same line height and justification. */
div, section, article, main {{ font-size: {BODY}pt !important; line-height: {LEAD} !important; text-align: justify; hyphens: auto; }}
h1, h2, h3, h4, h5, h6, b, strong, th {{ color: {BOLD} !important; }}
a, a * {{ color: {LINK} !important; text-decoration: none !important; border-bottom: none !important; }}
table, table tr, table td, table th, .table > tbody > tr > td, .table > tbody > tr > th {{ background-color: transparent !important; font-size: {BODY}pt !important; border-color: #1d2a2a !important; }}
/* Boxed widgets (exercise panels, cards, wells) flow inline at the text width, with no frame. */
.panel, .panel-group, .panel-heading, .panel-body, .panel-collapse, .card, .card-header, .card-body, .well, .exercises {{ border: none !important; border-radius: 0 !important; padding-left: 0 !important; padding-right: 0 !important; margin-left: 0 !important; margin-right: 0 !important; width: auto !important; max-width: none !important; }}
.panel-heading, .card-header {{ padding-top: 0.4em !important; padding-bottom: 0.2em !important; }}
/* Exercise choices read as a list: no side bar, list indent, an open circle marking each choice. */
.form-group, .exercise-part, .radio, .checkbox, .exercise-choice {{ border: none !important; margin: 0 !important; padding: 0 !important; }}
.exercise-part {{ padding-left: 1.2em !important; margin: 0.3em 0 0.8em !important; }}
.exercise-choice label {{ display: inline !important; padding-left: 0 !important; margin: 0 !important; font-weight: normal !important; }}
.exercise-choice.radio, .exercise-choice.checkbox {{ display: list-item !important; list-style: circle outside !important; }}
.exercise-answer, .exercise-explain, .form-inline, .exercise-progress, .exercise-error {{ display: none !important; }}
.panel, .panel-group, .panel-collapse, .panel-body {{ margin-bottom: 0 !important; padding-bottom: 0 !important; min-height: 0 !important; }}
/* A thin rule under the block's "reading exercises" heading and one after its last exercise mark where it starts and ends. */
div.reading-exercises.exercises > h1, div.reading-exercises.exercises > h2, div.reading-exercises.exercises > h3, div.reading-exercises.exercises > h4 {{ border-bottom: 0.6pt solid #2c3836 !important; padding-bottom: 0.4em !important; margin-bottom: 0.4em !important; }}
div.reading-exercises.exercises {{ border-bottom: 0.6pt solid #2c3836 !important; padding-top: 0.2em !important; padding-bottom: 0.6em !important; margin: 1em 0 !important; }}
.choices-text {{ font-style: italic; margin: 0.1em 0 0.5em; }}
.panel-title {{ color: {BOLD} !important; font-weight: 700 !important; font-size: 1em !important; }}
ul, ol {{ padding-left: 1.2em !important; margin-left: 0 !important; }}
h1 {{ margin-top: 0 !important; padding-top: 0 !important; }}
main, article, .container, .container-fluid {{ margin-top: 0 !important; padding-top: 0 !important; }}
h1, h2, h3, h4, h5, h6 {{ font-family: Baskerville, "PingFang SC", sans-serif !important; line-height: 1.4 !important; break-after: avoid; }}
h1, h2, h3, h4, h5, h6, th, caption, .panel-title {{ text-align: left !important; hyphens: manual !important; }}
h1 {{ font-size: 1.8em !important; }} h2 {{ font-size: 1.4em !important; }} h3 {{ font-size: 1.15em !important; }}
h4, h5, h6 {{ font-size: 1em !important; }}
code, pre, kbd, samp, tt, code *, pre * {{ font-family: Menlo, monospace !important; }}
code, kbd, samp, tt {{ font-size: 10px !important; }}
pre code, pre kbd, pre samp {{ font-size: 1em !important; }}
/* Code blocks take a boxed look (Bootstrap's pre), whatever the source page styles. */
pre {{ border: 1px solid #1d2a2a !important; border-radius: 4px !important; padding: 9.5px !important; margin: 0 0 10px !important; line-height: 1.42857 !important; }}
pre {{ font-size: 10px !important; white-space: pre-wrap !important; overflow-wrap: anywhere; }}
img, svg, video, canvas, table, pre, iframe {{ max-width: 100% !important; }}
/* Page containers (grid columns, centered wrappers) lose their side padding and offsets so text spans the full column. */
:is(div, main, section, article, h1, h2, h3, h4, h5, h6):not(#print-reset) {{ margin-left: 0 !important; margin-right: 0 !important; padding-left: 0 !important; padding-right: 0 !important; width: auto !important; min-width: 0 !important; max-width: none !important; float: none !important; }}
/* The page lays out at the text-column width on screen too, so FIT_JS measures what the print will show. */
html > body {{ width: {W - 2 * SIDE}pt !important; min-width: 0 !important; max-width: none !important; }}
html {{ min-width: 0 !important; width: auto !important; }}
table:has(img, svg) {{ table-layout: fixed !important; width: 100% !important; }}
svg {{ max-width: 100% !important; height: auto !important; }}
img, svg, canvas {{ height: auto; background-color: #ffffff !important; }}
header:not(:has(h1, h2)), iframe, .margin, .process, nav, .navbar, .sidebar, #sidebar, .toc-sidebar, input, button, select, textarea {{ display: none !important; }}
.collapse, details > * {{ display: block !important; height: auto !important; }}
/* Chinese typography: bold in 黑体 (Songti's bold barely differs from its regular), and paragraphs indented 2em
   except inside lists, tables, quotes and exercises. Emphasis sets Chinese in 黑体, since slanted Songti is
   illegible, and Latin in Baskerville italic, so a math variable (<em>a</em>) still reads as one. */
@font-face {{ font-family: "EmLatin"; src: local("Baskerville-Italic"); unicode-range: U+0000-024F; }}
@font-face {{ font-family: "EmHan"; src: local("PingFangSC-Semibold"); }}
strong, b, h1, h2, h3, h4, h5, h6, th, .panel-title {{ font-family: {HEI} !important; font-style: normal !important; }}
strong, b {{ font-weight: 700 !important; color: {BOLD} !important; }}
em, i, cite, dfn {{ font-family: "EmLatin", "EmHan", {HEI} !important; font-style: normal !important; font-weight: normal !important; }}
em {{ color: {BOLD} !important; }}
.choices-text {{ font-style: normal !important; }}
p {{ text-indent: 2em; }}
li p, td p, th p, dd p, blockquote p, .exercises p, .panel p, .mermaid p, figure p {{ text-indent: 0 !important; }}
</style><script>/* fitWide: fixes each box that runs past the column while its parent's box does not: a box of running text is never scaled (its too-wide children are fitted instead); an overlay (absolutely positioned) zooms its host so host and overlay fit together by their visible boxes. Once every visible box fits, layout overflow nothing draws in (a relatively positioned overlay's in-flow slot) is clipped so it cannot shrink the print, never on list items, whose markers sit outside; an inline element cannot clip, so it becomes an inline-block capped at the column; reflow a box wider than the column, then zoom what still cannot shrink. */
/* Latin words in the Chinese text are marked lang="en" so the print hyphenates them as the English pages do; under zh-CN a long word or code span that cannot break stretches the CJK spacing of its line instead. Text holding
   TeX ($ or a backslash) is left whole, since MathJax finds a formula only within one text node. */
addEventListener("load", function () {{ document.querySelectorAll("code, kbd, samp, tt").forEach(function (e) {{ if (!e.closest("pre")) e.setAttribute("lang", "en"); }}); var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT), texts = []; while (walker.nextNode()) texts.push(walker.currentNode); texts.forEach(function (t) {{ var p = t.parentElement; if (!p || p.closest("pre, code, kbd, samp, tt, script, style, svg, .mermaid, [lang=en]") || !/[A-Za-z]{{6}}/.test(t.data) || /[\\\\$]/.test(t.data)) return; var frag = document.createDocumentFragment(); t.data.split(/([A-Za-z]{{6,}})/).forEach(function (part, i) {{ if (i % 2) {{ var sp = document.createElement("span"); sp.lang = "en"; sp.textContent = part; frag.appendChild(sp); }} else if (part) frag.appendChild(document.createTextNode(part)); }}); p.replaceChild(frag, t); }}); }});
addEventListener("load", function () {{ function fitWide() {{ try {{ fitPasses(); }} catch (err) {{ document.documentElement.setAttribute("data-fit-error", err.message); }} }} function fitPasses() {{ for (var pass = 0; pass < 6; pass++) {{ var right = document.body.getBoundingClientRect().right - 1; var shown = function (e) {{ var r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(e).display !== "none" && getComputedStyle(e).visibility !== "hidden"; }}; var past = function (e) {{ return shown(e) && e.getBoundingClientRect().right > right + 1.5; }}; var extent = function (e) {{ return [e].concat(Array.from(e.querySelectorAll("*"))).filter(function (d) {{ return !d.ownerSVGElement && shown(d); }}).reduce(function (m, d) {{ return Math.max(m, d.getBoundingClientRect().right); }}, 0); }}; var opened = function (e) {{ return e.hasAttribute("data-fit-open"); }}; var wide = Array.from(document.querySelectorAll("body *")).filter(function (e) {{ return !e.ownerSVGElement && !opened(e) && past(e) && (!past(e.parentElement) || opened(e.parentElement)); }}); if (!wide.length) {{ var spill = function (e) {{ return !e.ownerSVGElement && e.clientWidth > 0 && getComputedStyle(e).display !== "list-item" && e.getBoundingClientRect().left + e.scrollWidth > right + 2; }}; Array.from(document.querySelectorAll("body *")).filter(spill).forEach(function (e) {{ if (getComputedStyle(e).display.indexOf("inline") === 0) {{ if (getComputedStyle(e).display === "inline") e.style.setProperty("display", "inline-block", "important"); e.style.setProperty("max-width", (right - e.getBoundingClientRect().left) + "px", "important"); }} e.style.setProperty("overflow-x", "clip", "important"); e.style.setProperty("overflow-clip-margin", "4px", "important"); }}); return; }} wide.forEach(function (e) {{ var pos = getComputedStyle(e).position; if (pos === "absolute" || pos === "fixed") {{ var host = e.offsetParent; if (host && host !== document.body) {{ var hl = host.getBoundingClientRect().left; host.style.zoom = ((parseFloat(host.style.zoom) || 1) * (right - hl) / (extent(host) - hl)).toFixed(4); return; }} }} var left = e.getBoundingClientRect().left; if (right - left <= 0) return; e.style.setProperty("max-width", (right - left) + "px", "important"); e.style.setProperty("margin-right", "0", "important"); e.style.setProperty("box-sizing", "border-box", "important"); var x = extent(e); if (x <= right) return; if (!/^(IMG|SVG|TABLE|PRE|CANVAS|VIDEO|IFRAME|FIGURE)$/i.test(e.tagName) && (e.innerText || "").length > 200) {{ e.setAttribute("data-fit-open", ""); return; }} e.style.zoom = ((parseFloat(e.style.zoom) || 1) * (right - left) / (x - left)).toFixed(4); }}); }} }} setTimeout(fitWide, 3000); setTimeout(function () {{ fitWide(); document.documentElement.setAttribute("data-fit-done", ""); }}, 8000); document.querySelectorAll("select").forEach(function (sel) {{ var opts = Array.from(sel.options).map(function (o) {{ return o.text.trim(); }}).filter(Boolean); var t = document.createElement("div"); t.className = "choices-text"; t.textContent = "[ " + opts.join(" / ") + " ]"; sel.replaceWith(t); }}); document.querySelectorAll("details").forEach(function (d) {{ d.open = true; }}); document.querySelectorAll("body *").forEach(function (e) {{ var p = getComputedStyle(e).position; if (p === "fixed" || p === "sticky") e.style.setProperty("display", "none", "important"); }}); }});</script>"""


# A page that loads MathJax prints only once it has typeset: under load the script can arrive after fitWide settles,
# and an early print shows raw TeX. MathJax 2 signals its first typeset through the End startup hook (which also
# fires for a hook registered after it), MathJax 3 through startup.promise.
MATH_DONE = """(function () {
  if (!document.querySelector('script[src*="mathjax" i]')) return true;
  if (typeof MathJax === "undefined") return false;
  if (MathJax.Hub && MathJax.Hub.Register) { if (!window.__mjHook) { window.__mjHook = true; MathJax.Hub.Register.StartupHook("End", function () { window.__mjDone = true; }); } return !!window.__mjDone; }
  if (MathJax.startup && MathJax.startup.promise) { MathJax.startup.promise.then(function () { window.__mjDone = true; }); return !!window.__mjDone; }
  return true;
})()"""


def print_pdf(url, out):
    """Prints a page through Chrome's DevTools protocol in real time, at a viewport as wide as the text column,
    once fitWide has settled the layout; the command-line printer's virtual clock can skip those timers."""
    import base64, json, socket, tempfile, time, urllib.request
    from websocket import create_connection
    with socket.socket() as sk:
        sk.bind(("127.0.0.1", 0)); port = sk.getsockname()[1]
    profile = tempfile.mkdtemp(prefix="html2pdf-")
    chrome = subprocess.Popen([chrome_binary(), "--headless=new", "--disable-gpu", f"--remote-debugging-port={port}",
                               f"--user-data-dir={profile}", "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            try:
                tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json")); break
            except OSError:
                time.sleep(0.2)
        ws = create_connection(next(t for t in tabs if t["type"] == "page")["webSocketDebuggerUrl"], suppress_origin=True, timeout=120)
        seq = [0]
        def call(method, **params):
            seq[0] += 1; ws.send(json.dumps({"id": seq[0], "method": method, "params": params}))
            while True:
                m = json.loads(ws.recv())
                if m.get("id") == seq[0]:
                    return m.get("result", {})
        call("Emulation.setDeviceMetricsOverride", width=round((W - 2 * SIDE) * 96 / 72), height=1200, deviceScaleFactor=1, mobile=False)
        call("Emulation.setEmulatedMedia", media="print")
        call("Page.navigate", url=url)
        deadline = time.time() + 150
        while True:
            time.sleep(1)
            r = call("Runtime.evaluate", expression='document.documentElement.hasAttribute("data-fit-done") && ' + MATH_DONE)
            if r.get("result", {}).get("value"):
                break
            if time.time() > deadline:
                raise SystemExit(f"FAIL {out}: page never settled")
        r = call("Page.printToPDF", preferCSSPageSize=True, printBackground=True, generateDocumentOutline=True)
        out.write_bytes(base64.b64decode(r["data"]))
    finally:
        chrome.kill(); chrome.wait()
        shutil.rmtree(profile, ignore_errors=True)


ap = argparse.ArgumentParser(description="Print a translated HTML page to a PDF in the translate book look.")
ap.add_argument("page")
ap.add_argument("--base", help="URL the page's relative links and images resolve against")
ap.add_argument("--skip-bookmark", action="append", default=[], help="drop outline entries with this title")
args = ap.parse_args()
src = Path(args.page).resolve()
tmp = src.with_name("." + src.stem + ".print.html")
pdf = src.with_suffix(".pdf")
html = src.read_text(errors="ignore")
# The page is Chinese now: zh-CN picks Chinese line breaking and Han glyphs.
html = re.sub(r"(<html\b[^>]*?)\s+lang=(\"[^\"]*\"|'[^']*'|\S+)", r"\1", html, count=1, flags=re.I)
html = re.sub(r"<html\b", '<html lang="zh-CN"', html, count=1, flags=re.I)
if args.base:
    html = f'<base href="{args.base}">' + html
html = re.sub(r"</head>", lambda m: CSS + "</head>", html, count=1, flags=re.I) if re.search(r"</head>", html, re.I) else CSS + html
tmp.write_text(html)
try:
    # Print beside the PDF and replace it only on success, so a failed run never clobbers a good file.
    staged = pdf.with_name("." + pdf.name)
    print_pdf(tmp.as_uri(), staged)
    staged.replace(pdf)
finally:
    tmp.unlink(missing_ok=True)
if pdf.exists() and pdf.stat().st_size > 1000:
    import pymupdf
    doc = pymupdf.open(pdf)
    skip = {b.strip().lower() for b in args.skip_bookmark}
    toc = [[l, " ".join(t.split()), p] for l, t, p in doc.get_toc() if t.strip() and t.strip().lower() not in skip]
    doc.set_toc(toc)
    doc.saveIncr()
    import collections
    sizes, over = collections.Counter(), 0
    for page in doc:
        limit = page.rect.width - SIDE + 2
        for b in page.get_text("rawdict")["blocks"]:
            for l in b.get("lines", []):
                for sp in l["spans"]:
                    sp["text"] = "".join(ch["c"] for ch in sp["chars"])
                    ink = [ch["bbox"][2] for ch in sp["chars"] if not ch["c"].isspace()]
                    sp["bbox"] = (0, 0, max(ink) if ink else 0, 0)  # trailing spaces in code draw nothing
                    if "mono" not in sp["font"].lower() and "menlo" not in sp["font"].lower():
                        sizes[round(sp["size"], 2)] += len(sp["text"])
                    over += sp["bbox"][2] > limit and bool(sp["text"].strip())
    body = sizes.most_common(1)[0][0] if sum(sizes.values()) > 200 else BODY  # a page of code alone has no body text
    if body != BODY or over:
        print(f"WARN {pdf} body={body}pt overflow_spans={over}")

    print("OK", pdf)
else:
    print("FAIL", src)
