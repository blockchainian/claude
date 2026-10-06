#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Tests render.py: Markdown typesetting, LaTeX repair, page CSS, and (when Chrome is present) a real
# ABOUTME: render of a small generated EPUB with cover and bookmarks. extract/translate are tested in the .mjs files.
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace

import pikepdf

HERE = Path(__file__).resolve().parent.parent / "scripts"
fails = []


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check(name, cond, detail=""):
    print(f"{'PASS' if cond else 'FAIL'}: {name}" + (f"  -- {detail}" if not cond and detail else ""))
    if not cond:
        fails.append(name)


def test_render_units(rd):
    if (Path.home() / "Library/Preferences/com.googlecode.iterm2.plist").exists():
        bg, fg = rd.iterm_colors()
        check("iterm colors are hex", bg.startswith("#") and len(bg) == 7 and fg.startswith("#") and len(fg) == 7, f"{bg} {fg}")
    check("title_slug: lowercase, dashes, no language suffix",
          rd.title_slug("The Art of Doing Science and Engineering") == "the-art-of-doing-science-and-engineering")
    check("title_slug: drops the subtitle after a colon",
          rd.title_slug("Addiction by Design: Machine Gambling in Las Vegas") == "addiction-by-design")
    check("title_slug: handles a full-width colon",
          rd.title_slug("书名：副标题") == "")
    check("title_slug: a non-Latin title slugs empty (caller falls back)", rd.title_slug("量子力学") == "")
    check("default_out: <title-slug>.pdf, no -zh suffix",
          rd.default_out({"title": "Zero to One"}, Path("/tmp/x/work")).name == "zero-to-one.pdf")
    check("default_out: non-Latin title falls back to the work dir slug",
          rd.default_out({"title": "量子力学"}, Path("/tmp/x/quantum-mechanics")).name == "quantum-mechanics.pdf")
    title, body = rd.md_to_html("# 章名\n\n第一段 *强调*。\n\n> 引文\n\n## 小标题\n\n第二段。\n")
    check("markdown title split off", title == "章名" and "<h1" not in body)
    check("markdown body html", "<em>强调</em>" in body and "<blockquote>" in body and "<h2>小标题</h2>" in body)
    import tempfile as _tmp
    from PIL import Image, ImageDraw
    with _tmp.TemporaryDirectory() as d:
        work = Path(d); (work / "images").mkdir()
        eq = Image.new("RGB", (300, 12), "white"); ImageDraw.Draw(eq).line((2, 6, 280, 6), fill="black", width=2)
        eq.save(work / "images" / "5_1.png")  # black-on-white line art
        photo = Image.new("RGB", (40, 10), (200, 40, 40)); photo.save(work / "images" / "5_2.png")  # colour figure
        images = {"5_1": {"file": "5_1.png", "w": 300, "h": 12, "block": True},
                  "5_2": {"file": "5_2.png", "w": 40, "h": 10, "block": False}}
        out = rd.place_images("<p>ascent in ⟦IMG:5_1⟧ where ⟦IMG:5_2⟧ is</p>", work, images, "#c9c4b8", "#000409")
        check("line-art equation is recoloured, no white plate", '<figure class="fig">' in out and "5_1.rc.png" in out and (work / "images" / "5_1.rc.png").exists())
        check("colour figure keeps a white plate", 'class="infig plate"' in out and "5_2.png" in out)
        check("unknown placeholder dropped, not left raw", rd.place_images("a⟦IMG:zz⟧b", work, images, "#c9c4b8", "#000409") == "ab")
    check("roman folios", [rd.folio_for(i, None) for i in range(3)] == ["i", "ii", "iii"])
    check("roman folio past the table falls back to arabic (long front matter)",
          rd.roman_folio(len(rd.ROMAN) - 1) == str(len(rd.ROMAN)) and rd.folio_for(len(rd.ROMAN) + 5, None) == str(len(rd.ROMAN) + 6),
          f"{rd.roman_folio(len(rd.ROMAN) - 1)} / {rd.folio_for(len(rd.ROMAN) + 5, None)}")
    style = rd.css([427.6, 660], "#181a1d", "#e1ddd5", [("03", "前言", "front"), ("04", "第一章", "chapter")])
    check("css page size and colors", "size: 427.6pt 660pt" in style and "--bg: #181a1d" in style and "Baskerville" in style)
    check("css front roman, body arabic", '@page s03 { @top-center { content: "前言"' in style and "counter(page, lower-roman)" in style.split("@page s03")[1].split("}}")[0]
          and "content: counter(page);" in style.split("@page s04")[1].split("} }")[0])
    # A single element wider than the text column makes Chrome's print scale the whole book's font down uniformly.
    # css() must contain each section's horizontal overflow, and KATEX_HEAD must scale over-wide display equations.
    check("css contains section overflow so one wide element can't shrink the book", "overflow-x: clip" in style.split("section {")[1].split("}")[0])
    check("css caps media and wraps code/long tokens", ".katex-display { max-width: 100%" in style and "white-space: pre-wrap" in style and "overflow-wrap: break-word" in style)
    check("katex head scales over-wide display equations to fit", ".katex-display" in rd.KATEX_HEAD and "--eqcol-w" in rd.KATEX_HEAD and "fontSize" in rd.KATEX_HEAD and "document.fonts.ready" in rd.KATEX_HEAD)
    check("katex head marks parse errors for the render guard", "katex-error" in rd.KATEX_HEAD and "cc0000" in rd.KATEX_HEAD and "KERR" in rd.KATEX_HEAD)
    check("css exposes the text-column width for the equation fit", "--eqcol-w:" in style)
    # A Markdown rule (---, a scene break) becomes <hr>; Chrome's default inset border paints a light bar on a dark page.
    hr = style.split("\nhr {")[1].split("}")[0] if "\nhr {" in style else ""
    check("css renders a scene-break rule as blank space, no border", "border: 0" in hr and "hr + p { text-indent: 0" in style, hr)
    sec = {"id": "04", "kind": "chapter", "label": "第一章"}
    frag = rd.section_html(sec, "标题", "<p>x</p>")
    check("section carries marker, label, named page", "⟦S04⟧" in frag and "第一章" in frag and 'page: s04' in frag)
    toc = rd.contents_html([(sec, "标题")], {"04": "1"})
    check("contents row", "⟦TOC⟧" in toc and '<span class="pg">1</span>' in toc)


def test_math_units(rd):
    # Inline/display LaTeX must survive Markdown untouched (its _ and * not eaten), so KaTeX can typeset it.
    _, body = rd.md_to_html("# T\n\n设 \\(x_{1}\\)、\\(L^{2}\\)，且 \\[y = a_{i}\\]。\n")
    check("inline/display LaTeX survives markdown",
          "\\(x_{1}\\)" in body and "\\(L^{2}\\)" in body and "\\[y = a_{i}\\]" in body and "<em>" not in body, body)
    # The translator's common LaTeX mistakes are repaired so KaTeX never renders red error source (repair_math),
    # and a blockquote wrapping a display equation is unwrapped so a wide equation gets the full column.
    check("inline \\tag promoted to display", rd.repair_math("\\(x\\tag{1}\\)") == "\\[x\\tag{1}\\]")
    check("currency $ escaped inside math", rd.repair_math("\\($100\\)") == "\\(\\$100\\)")
    check("\\mbox (unsupported by KaTeX) rewritten to \\text", rd.repair_math("\\(\\mathrm{IS\\mbox{-}GOAL}\\)") == "\\(\\mathrm{IS\\text{-}GOAL}\\)")
    check("blockquote markers stripped from multi-line display", "\n>" not in rd.repair_math("\\[\n> a\\\\\n> b\n> \\]"))
    check("\\\\[2pt] array row-skip inside a display is not corrupted", rd.repair_math("\\[a\\\\[2pt]b\\]") == "\\[a\\\\[2pt]b\\]")
    _, bq = rd.md_to_html("# T\n\n> \\[\n> a\\Rightarrow b\n> \\]\n")
    check("math-only blockquote unwrapped (no <blockquote>)", "<blockquote>" not in bq and "\\[" in bq and "\n>" not in bq, bq)
    _, fw = rd.md_to_html("# T\n\n设 \\(x=1\\）在……\n")
    check("mis-typed full-width close delimiter fixed", "\\(x=1\\)" in fw, fw)


def make_epub_book(path):
    """A small EPUB with a cover image, a Preface (front) and one chapter (1. One), for an end-to-end render test."""
    import zipfile
    from PIL import Image
    with tempfile.TemporaryDirectory() as d:
        cover = Path(d) / "cover.png"
        Image.new("RGB", (60, 90), (34, 68, 170)).save(cover)
        cover_bytes = cover.read_bytes()
    opf = ('<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0">'
           '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">'
           '<dc:title>Tiny Book: A Test</dc:title><dc:creator>Tester</dc:creator></metadata>'
           '<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
           '<item id="cover-img" href="cover.png" media-type="image/png" properties="cover-image"/>'
           '<item id="pre" href="preface.xhtml" media-type="application/xhtml+xml"/>'
           '<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest>'
           '<spine><itemref idref="pre"/><itemref idref="c1"/></spine></package>')
    nav = ('<html><body><nav><ol><li><a href="preface.xhtml">Preface</a></li>'
           '<li><a href="ch1.xhtml">1. One</a></li></ol></nav></body></html>')
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("META-INF/container.xml",
                   '<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
                   '<rootfiles><rootfile full-path="package.opf"/></rootfiles></container>')
        z.writestr("package.opf", opf)
        z.writestr("nav.xhtml", nav)
        z.writestr("cover.png", cover_bytes)
        z.writestr("preface.xhtml", "<html><body><h1>Preface</h1><p>Preface text here.</p></body></html>")
        z.writestr("ch1.xhtml", "<html><body><h1>1. One</h1><p>Chapter one text here.</p></body></html>")


def test_render_e2e(rd):
    chrome = None
    for c in ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
              "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
              "/Applications/Chromium.app/Contents/MacOS/Chromium", shutil.which("google-chrome"), shutil.which("chrome"), shutil.which("chromium")]:
        if c and Path(c).exists():
            chrome = c
    if not chrome or not shutil.which("pdftotext"):
        print("SKIP: render e2e (Chrome or pdftotext missing)")
        return
    with tempfile.TemporaryDirectory() as d:
        book = Path(d) / "tiny.epub"
        make_epub_book(book)
        work = Path(d) / "work"
        r = subprocess.run(["node", str(HERE / "extract.mjs"), str(book), "--work", str(work)], capture_output=True, text=True)
        check("extract runs on generated EPUB", r.returncode == 0, r.stderr[-500:])
        meta = json.loads((work / "sections.json").read_text())
        check("extract finds front, chapter", [s["kind"] for s in meta["sections"]] == ["front", "chapter"], str(meta["sections"]))
        check("extract kept the cover image", bool(meta.get("cover_image")) and (work / meta["cover_image"]).exists(), str(meta.get("cover_image")))
        check("extract page size default", meta["page_size"] == [468.0, 680.0], str(meta["page_size"]))
        (work / "md").mkdir()
        for s in meta["sections"]:
            if s.get("file"):
                (work / "md" / (Path(s["file"]).stem + ".md")).write_text(f"# {s['title']}译\n\n" + ("正文。" * 400 + "\n\n") * 6)
        out = Path(d) / "tiny-zh.pdf"
        opt = SimpleNamespace(only=None, out=str(out), title="小书", bg="#181a1d", fg="#e1ddd5", font_size=9.25, eq_scale=0.6, bold_factor=1.25)
        rd.render(work, opt)
        pdf = pikepdf.open(out)
        with pdf.open_outline() as outline:
            marks = [(str(i.title), pikepdf.Page(i.destination[0]).index) for i in outline.root]
        check("bookmarks: Cover, 目录, sections", [m[0] for m in marks] == ["Cover", "目录", "Preface译", "第一章　One译"], str(marks))
        check("bookmark pages ascend from the cover", marks[0][1] == 0 and marks[1][1] == 1 and marks[2][1] == 2 < marks[3][1], str(marks))
        check("title metadata", str(pdf.docinfo["/Title"]) == "小书" and str(pdf.docinfo["/Author"]) == "Tester")
        links = [a for a in pdf.pages[1].get("/Annots", []) if a.get("/Subtype") == "/Link"]
        targets = sorted(pikepdf.Page(a.Dest[0]).index for a in links)
        check("目录 rows link to the sections", len(links) == 2 and targets == [marks[2][1], marks[3][1]], f"{len(links)} links -> {targets}")
        rects = [[float(v) for v in a.Rect] for a in links]
        check("目录 links are full-width rows in page bounds", all(0 < r[0] < r[2] <= 468 and 0 < r[1] < r[3] <= 680 for r in rects), str(rects))
        text = subprocess.run(["pdftotext", "-layout", str(out), "-"], capture_output=True, text=True).stdout.split("\f")
        folios = [ln.strip() for pg in text for ln in pg.splitlines() if ln.strip() in ("i", "ii", "iii", "iv", "1", "2", "3", "4")]
        check("front roman and body arabic folios present", "i" in folios and "1" in folios, str(folios))
        check("contents lists body page 1", any("One译" in ln and ln.rstrip().endswith("1") for ln in text[1].splitlines()), text[1])
        cover_box = [float(v) for v in pdf.pages[0].mediabox]
        check("cover page is the book page size", round(cover_box[2] - cover_box[0]) == 468 and round(cover_box[3] - cover_box[1]) == 680, str(cover_box))
        opt = SimpleNamespace(only=meta["sections"][1]["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=12, eq_scale=0.6, bold_factor=1.25)
        rd.render(work, opt)
        check("single-section preview written", any((work / "pdf").glob("*.pdf")))
        # Last, because it corrupts a section's md: an equation KaTeX cannot parse (an undefined command) renders
        # as red source; the render must fail on it rather than ship it silently.
        chap = next(s for s in meta["sections"] if s["kind"] == "chapter")
        (work / "md" / (Path(chap["file"]).stem + ".md")).write_text("# 坏公式译\n\n见 \\(\\zzbadmacro\\)。\n\n" + "正文。" * 60)
        try:
            rd.render(work, SimpleNamespace(only=None, out=str(Path(d) / "bad-zh.pdf"), title="小书", bg="#181a1d", fg="#e1ddd5", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
            guarded = False
        except SystemExit as e:
            guarded = "failed to render" in str(e)
        check("render fails on an unparseable equation (KaTeX error guard)", guarded)


def main():
    rd = load("render")
    test_render_units(rd)
    test_math_units(rd)
    test_render_e2e(rd)
    r = subprocess.run(["bash", str(HERE / "setup.sh"), "--check"], capture_output=True, text=True)
    check("setup.sh --check reports", "present:" in r.stdout, r.stdout + r.stderr)
    print(f"\n{len(fails)} failures" if fails else "\nall passed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
