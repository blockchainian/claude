#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["pikepdf>=9", "markdown>=3.5", "pillow>=10"]
# ///
# ABOUTME: Tests render.py: Markdown typesetting, LaTeX repair, page CSS, and (when Chrome is present) a real
# ABOUTME: render of a small generated EPUB with cover and bookmarks. extract/translate are tested in the .mjs files.
import importlib.util
import json
import re
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace

import pikepdf

HERE = Path(__file__).resolve().parent.parent / "scripts"
sys.path.insert(0, str(HERE))
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
    code = "  a < b\n    \\[literal\\] and \\(open"
    _, listing = rd.md_to_html("# 代码\n\n⟦CODE:c1⟧\n", {"c1": code})
    check("protected listing renders verbatim without math capture", '<pre><code>' + __import__('html').escape(code) + '</code></pre>' in listing, listing)
    _, fenced = rd.md_to_html("# 代码\n\n```python\n" + code + "\n```\n\n<pre><code>\\[raw\\]</code></pre>\n")
    check("fenced and raw code bypass math capture", "@@MATH" not in fenced and '<code class="language-python">' in fenced and '<pre><code>\\[raw\\]</code></pre>' in fenced, fenced)
    _, notes = rd.md_to_html("# 注释\n\n12. 十二\n\n> 引文\n\n13. 十三\n\n补充段落。\n\n14. 十四\n\n- 补充条目\n\n15. 十五\n")
    starts = re.findall(r'<ol(?: start="(\d+)")?>', notes)
    check("ordered notes preserve starts across quote paragraph and bullet interruptions", starts == ["12", "13", "14", "15"], notes)
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
        # EPUB build: an inline formula image is sized from its own pixels (width x inline_scale, in pt) like a block
        # figure, not clamped to a fixed 1.4em height that squashes every formula to one height whatever its content.
        scaled = rd.place_images("<p>where ⟦IMG:5_2⟧ is</p>", work, images, "#c9c4b8", "#000409", 0.6, 0.33)
        check("EPUB inline image sized from intrinsic width x inline_scale, height unclamped",
              'class="infig plate"' in scaled and "width:13.2pt" in scaled and "max-height:none" in scaled, scaled)
        check("non-EPUB inline image keeps the 1.4em clamp (no inline size)", "style=" not in out.split("5_2")[0].rsplit("<img", 1)[1], out)
        bound = rd.place_images("<p>得出 ⟦IMG:5_2⟧。）后 ⟦IMG:5_1⟧，再</p>", work, images, "#c9c4b8", "#000409", 0.6, 0.33)
        check("inline image + CJK punctuation bound together, block figure not",
              bound.count('class="nb"') == 1 and '<span class="nb"><img class="infig plate"' in bound and "。）</span>" in bound, bound)
        # A caption paragraph right after a block figure must never be split from it by a page break.
        _, cap_md = rd.md_to_html("# T\n\n⟦IMG:5_1⟧\n\n**图 3.** 一棵树。\n\n⟦IMG:5_1⟧\n\n**图 4。**两棵树。\n\n⟦IMG:5_1⟧\n\n后文。\n")
        cap = rd.place_images(cap_md, work, images, "#c9c4b8", "#000409", 0.6, 0.33)
        check("block figure + caption kept together in one no-break block",
              cap.count('<div class="figcap"><figure') == 2 and "一棵树。</p></div>" in cap and "两棵树。</p></div>" in cap, cap)
        check("a figure followed by ordinary prose is not wrapped", cap.count("figcap") == 2 and "<p>后文。</p>" in cap, cap)
        lead = rd.place_images("<p>长段。</p>\n<p>如下：</p>\n<p>⟦IMG:5_1⟧</p>\n<p>后文。</p>", work, images, "#c9c4b8", "#000409", 0.6, 0.33)
        check("the paragraph right before a block figure keeps with it, no empty paragraphs left",
              '<p class="keep">如下：</p>' in lead and lead.count("keep") == 1 and "<p></p>" not in lead, lead)
        # A book whose figures are grayscale screenshots (a UI design book) keeps every image as is, on a plate.
        shot = rd.place_images("<p>⟦IMG:5_1⟧</p>", work, images, "#c9c4b8", "#000409", 0.6, 0.33, recolor=False)
        check("recolor=False keeps grayscale line art on a white plate, no recoloured copy",
              '<figure class="fig plate">' in shot and "5_1.png" in shot and "5_1.rc.png" not in shot, shot)
        check("unknown placeholder dropped, not left raw", rd.place_images("a⟦IMG:zz⟧b", work, images, "#c9c4b8", "#000409") == "ab")
    check("roman folios", [rd.folio_for(i, None) for i in range(3)] == ["i", "ii", "iii"])
    check("roman folio past the table falls back to arabic (long front matter)",
          rd.roman_folio(len(rd.ROMAN) - 1) == str(len(rd.ROMAN)) and rd.folio_for(len(rd.ROMAN) + 5, None) == str(len(rd.ROMAN) + 6),
          f"{rd.roman_folio(len(rd.ROMAN) - 1)} / {rd.folio_for(len(rd.ROMAN) + 5, None)}")
    style = rd.css([427.6, 660], "#181a1d", "#e1ddd5", [("03", "前言", "front"), ("04", "第一章", "chapter")])
    check("a plate's padding stays inside max-width (a wide figure is not clipped on the right)",
          re.search(r"\.fig\.plate img \{[^}]*box-sizing: border-box", style) and re.search(r"\.infig\.plate \{[^}]*box-sizing: border-box", style))
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
    # The floated 2.6em drop cap belongs on prose: on a first paragraph that opens with a bold label ("**1.** ...",
    # also after a leading ▶ image), a digit, or that is too short to wrap round it, it breaks the layout.
    dropcap = lambda body: "no-dropcap" not in rd.section_html(sec, "标题", body).split('class="body-text', 1)[1].split(">", 1)[0]
    prose = "<p>" + "正文" * 60 + "</p>"
    check("drop cap kept on a long prose opening paragraph", dropcap(prose))
    check("no drop cap when the first paragraph opens with a bold label", not dropcap("<p><strong>1.</strong> " + "答案" * 60 + "</p>"))
    check("no drop cap after a leading image then a bold label", not dropcap('<p><img class="infig" src="a.png"> <strong>2.</strong> ' + "习题" * 60 + "</p>"))
    check("no drop cap after a hung marker span then a bold label",
          not dropcap('<p><span style="display: inline-block; width: 1.2em; margin-left: -1.2em; text-indent: 0">'
                      '<img class="infig" src="a.png"></span> <strong>3.</strong> ' + "习题" * 60 + "</p>"))
    check("no drop cap after a hung text marker (▶) then a bold label",
          not dropcap('<p><span style="display: inline-block; width: 1.2em; margin-left: -1.2em; text-indent: 0">'
                      '▶</span><strong>4.</strong> ' + "习题" * 60 + "</p>"))
    check("no drop cap when the first paragraph opens with a digit", not dropcap("<p>7 " + "正文" * 60 + "</p>"))
    check("drop cap even on a two-character opening stub", dropcap("<p>数列</p>" + prose))
    check("no drop cap on an index entry label", not dropcap("<p>算法 1.1E</p><p>算法 1.1F</p>"))
    check("no drop cap on an image-only first paragraph", not dropcap('<p><img class="infig" src="a.png"></p>' + prose))
    # A one-sentence opener just over a line still wraps the two-line cap; 60 characters skipped many sections.
    check("drop cap on a one-line-plus opening sentence", dropcap("<p>算法的概念是所有计算机程序设计的基础，因此我们应当从仔细分析这一概念开始。</p>"))
    check("css clears the block after a capped paragraph", ".body-text:not(.no-dropcap) > p.dropcap + * { clear: left; }" in style)
    # place_images marks a paragraph leading into a figure <p class="keep">; it is still the first paragraph.
    check("drop cap judged on a first paragraph that carries a class", dropcap('<p class="keep">' + "正文" * 30 + "</p><p>短。</p>")
          and not dropcap('<p class="keep"><strong>1.</strong> 答</p><p>' + "正文" * 30 + "</p>"))
    # The capped paragraph clears the float, so even a one-line lead-in ("若 x 是任意实数，我们记") takes the cap.
    check("drop cap on a short one-line lead-in", dropcap("<p>若 \\(x\\) 是任意实数，我们记</p>" + prose))
    check("no drop cap on a bold table or figure caption", not dropcap("<p><strong>表 1</strong> 标准子程序和计算机程序分析中常用的量（40 位十进制）</p>")
          and not dropcap('<p><strong><span class="cjkb">图</span> 3</strong> 一棵树的各个部分与它们之间的连接方式</p>'))
    opener = '<p class="keep"><strong>图　示意</strong></p><figure class="fig"><img src="a.png"></figure><blockquote><p>题词。</p></blockquote>' + prose
    cap_html = rd.section_html(sec, "标题", opener)
    check("caption photo epigraph opener caps prose rather than unnumbered caption", '<p class="dropcap">正文' in cap_html and '<p class="keep"><strong>图' in cap_html, cap_html)
    classed = rd.section_html(sec, "标题", '<p class="caption">A photo</p><figure class="fig"><img src="a.png"></figure>' + prose)
    check("source caption class is excluded from drop cap", '<p class="dropcap">正文' in classed and '<p class="caption">A photo</p>' in classed, classed)
    # A bold key phrase opening prose is not an answer label: it gets the cap, set bold to match the phrase.
    bold_open = '<p><strong><span class="cjkb">取自</span> n <span class="cjkb">个对象的组合</span></strong>，是从集合中选出不同元素而不计次序的各种选法，对五个对象每次取三个。</p>'
    check("drop cap on prose that opens with a bold phrase", dropcap(bold_open))
    check("a bold-phrase opener marks the cap bold", "boldcap" in rd.section_html(sec, "标题", bold_open))
    check("a plain opener does not mark the cap bold", "boldcap" not in rd.section_html(sec, "标题", prose))
    # An epigraph (blockquote) before the text: the cap goes on the first top-level paragraph, as the CSS selects it.
    check("drop cap judged on the paragraph after an epigraph", dropcap("<blockquote><p>短题词。</p></blockquote>" + prose))
    check("css sets a bold-phrase cap in the bold face", ".body-text.boldcap > p.dropcap::first-letter" in style)
    # Chinese books indent every paragraph; only a drop-cap paragraph starts flush, so a list section's first item
    # (an answer "1.", an index entry) lines up with the rest.
    check("css starts only a drop-cap paragraph flush", ".body-text:not(.no-dropcap) > p.dropcap { text-indent: 0" in style
          and ".body-text > p.dropcap { text-indent: 0" not in style)
    # Songti has no italic: an italic blockquote is mechanically slanted Chinese. Latin keeps its italic face.
    bq = style.split("\nblockquote {")[1].split("}")[0]
    check("css sets blockquote Chinese upright", "font-style: italic" not in bq and "QuoteLatin" in bq, bq)
    check("css maps blockquote Latin to Baskerville italic", '@font-face { font-family: "QuoteLatin"; src: local("Baskerville-Italic")' in style)
    # 黑体 glyphs sit about 0.05em higher on the baseline than Songti's: bold Chinese runs are nudged down to match.
    bbody = rd.nudge_bold_cjk("<p>正文<strong>粗体 AB 字</strong>，<em>强调</em>。</p>")
    check("bold Chinese runs wrapped for the baseline nudge", '<strong><span class="cjkb">粗体</span> AB <span class="cjkb">字</span></strong>' in bbody
          and '<em><span class="cjkb">强调</span></em>' in bbody, bbody)
    check("css nudges bold Chinese down", ".cjkb { vertical-align: -0.05em; }" in style)
    check("css keeps a figure and its caption on one page", ".figcap { break-inside: avoid" in style)
    check("css gates the drop cap on the body-text class", ".body-text:not(.no-dropcap) > p.dropcap::first-letter" in style)
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
    # A pipe table (the translator's form of an EPUB <table>) is typeset as a table, math in its cells intact.
    _, tb = rd.md_to_html("# T\n\n**表 1**\n\n| \\(i\\): | 0 | 1 |\n|---|---:|---:|\n| `NAME(i)`: | — | `a` |\n")
    check("pipe table becomes <table>, its math intact", "<table>" in tb and "<td" in tb and "\\(i\\)" in tb and "&mdash;" not in tb, tb)
    _, bq = rd.md_to_html("# T\n\n> \\[\n> a\\Rightarrow b\n> \\]\n")
    check("math-only blockquote unwrapped (no <blockquote>)", "<blockquote>" not in bq and "\\[" in bq and "\n>" not in bq, bq)
    # CJK closing punctuation right after inline math or inline code must never start a line: the pair is bound
    # in a no-break span (the math itself stays breakable inside; see the e2e line-start check).
    _, nb = rd.md_to_html("# T\n\n设 \\(x_{1}\\)，都可以；令 `PTR`。然后 \\(y\\) 与 \\[z\\]。\n")
    check("inline math + CJK punctuation bound together", '<span class="nb">\\(x_{1}\\)，</span>' in nb, nb)
    check("inline code + CJK punctuation bound together", '<span class="nb"><code>PTR</code>。</span>' in nb, nb)
    check("math not followed by punctuation, and display math, left unwrapped", nb.count('class="nb"') == 2, nb)
    # A hard-broken staircase ("1 A" / "　　2 A" / "　　　　3 A") keeps the ideographic-space indent of each continued
    # line (Markdown would strip it); a paragraph's own leading ideographic spaces are left to Markdown.
    _, st = rd.md_to_html("# T\n\n`1 A`  \n\u3000\u3000`2 A`  \n\u3000\u3000\u3000\u3000`3 A`\n\n\u3000\u3000正文。\n")
    check("hard-broken lines keep their ideographic-space indent",
          "<br>\n&#12288;&#12288;<code>2 A</code>" in st and "<br>\n&#12288;&#12288;&#12288;&#12288;<code>3 A</code>" in st and "<p>正文。</p>" in st, st)
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
        (work / "translated").mkdir()
        for s in meta["sections"]:
            if s.get("file"):
                (work / "translated" / (Path(s["file"]).stem + ".md")).write_text(f"# {s['title']}译\n\n" + ("正文。" * 400 + "\n\n") * 6)
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
        chap = meta["sections"][1]
        (work / "codeblocks.json").write_text(json.dumps({"c1": "if a < b:\n    \\[literal\\]\n    \\(unclosed"}))
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text("# 代码译\n\n正文。\n\n⟦CODE:c1⟧\n")
        rd.render(work, opt)
        code_pdf = work / "pdf" / (Path(chap["file"]).stem + ".pdf")
        code_text = subprocess.run(["pdftotext", "-layout", str(code_pdf), "-"], capture_output=True, text=True).stdout
        check("protected code reaches PDF with literal math delimiters", "if a < b:" in code_text and "\\[literal\\]" in code_text and "\\(unclosed" in code_text, code_text)
        rd.render(work, opt)
        check("single-section preview written", any((work / "pdf").glob("*.pdf")))
        # CJK closing punctuation after inline math or code must not wrap to the start of a line. Many paragraphs
        # with every prefix length put the math at every position in the line, so some land at a line end.
        chap = next(s for s in meta["sections"] if s["kind"] == "chapter")
        paras = "\n\n".join("正" * n + "\\(x_{1}+y\\)，" + "文" * 30 + "`PTR`。" + "字" * 20 for n in range(1, 45))
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text("# 标点译\n\n" + paras + "\n")
        rd.render(work, SimpleNamespace(only=chap["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
        prev = next((work / "pdf").glob(f"{chap['id']}-*.pdf"))
        lines = [ln.strip() for ln in subprocess.run(["pdftotext", str(prev), "-"], capture_output=True, text=True).stdout.splitlines()]
        starts = [ln for ln in lines if ln and ln[0] in "，。、；：！？"]
        check("no line starts with CJK punctuation after inline math or code", not starts, f"{len(starts)}: {starts[:3]}")
        # Binding the punctuation must not stop a long inline formula from wrapping inside itself (a no-break
        # formula wider than the column would overflow and be clipped).
        long_eq = "+".join(f"\\mathrm{{w{i:02d}}}" for i in range(1, 61))
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text(f"# 长式译\n\n设 \\({long_eq}\\)，于是。\n")
        rd.render(work, SimpleNamespace(only=chap["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
        text = subprocess.run(["pdftotext", str(prev), "-"], capture_output=True, text=True).stdout
        line_of = lambda w: next((i for i, ln in enumerate(text.splitlines()) if w in ln), -1)
        check("a long inline formula followed by punctuation still wraps inside", 0 <= line_of("w01") < line_of("w60"), text[:900].replace(chr(10), " | "))
        check("the wrapped formula's punctuation stays on its last line", "，" in text.splitlines()[line_of("w60")] if line_of("w60") >= 0 else False, text[:900].replace(chr(10), " | "))
        # Inline code and KaTeX \\mathtt are one monospace face (KaTeX_Typewriter), not two side by side; and a paragraph
        # with hard line breaks (aligned rows) gets no first-line indent, so its rows start at one x.
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text(
            "# 等宽译\n\n令 `ROW` 与 \\(\\mathtt{BASEROW}[i]\\) 相同。\n\n甲一二三  \n乙四五六  \n丙七八九\n\n" + "正文。" * 40 + "\n")
        rd.render(work, SimpleNamespace(only=chap["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
        fonts = subprocess.run(["pdffonts", str(prev)], capture_output=True, text=True).stdout
        mono = [ln.split()[0] for ln in fonts.splitlines()[2:] if re.search(r"Typewriter|Menlo|Courier|Mono|Monaco", ln)]
        check("inline code and \\mathtt share one monospace font", len(mono) == 1 and "KaTeX_Typewriter" in mono[0], str(mono))
        bbox = subprocess.run(["pdftotext", "-bbox", str(prev), "-"], capture_output=True, text=True).stdout
        xs = [float(x) for w in ("甲", "乙", "丙") for x in re.findall(rf'xMin="([\d.]+)"[^>]*>{w}', bbox)[:1]]
        check("hard-broken rows start at one x (no first-line indent)", len(xs) == 3 and max(xs) - min(xs) < 1, str(xs))
        # A short paragraph leading into a block figure ("如下：", "**9、10、11。**") must not be left alone at a page
        # bottom while its figure moves to the next page. Fillers of every length put the lead at every position.
        from PIL import Image, ImageDraw
        (work / "images").mkdir(exist_ok=True)
        imgs = {}
        for k in range(1, 25):  # one distinct image per figure, so each placement is its own image in the PDF
            fig = Image.new("RGB", (400, 300), "white"); ImageDraw.Draw(fig).rectangle((10, 10, 390, 290), outline="black", width=3)
            ImageDraw.Draw(fig).rectangle((20, 20, 20 + k * 10, 40), fill="black")
            fig.save(work / "images" / f"f{k}.png")
            imgs[f"f{k}"] = {"file": f"f{k}.png", "w": 0, "h": 0, "block": True}
        (work / "images.json").write_text(json.dumps(imgs))
        blocks = "".join("正文" * (k * 23) + f"\n\n引{k:02d}如下：\n\n⟦IMG:f{k}⟧\n\n" for k in range(1, 25))
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text("# 引图译\n\n" + blocks)
        rd.render(work, SimpleNamespace(only=chap["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
        pages = subprocess.run(["pdftotext", str(prev), "-"], capture_output=True, text=True).stdout.split("\f")
        leads = [pg.count("如下：") for pg in pages]
        listed = subprocess.run(["pdfimages", "-list", str(prev)], capture_output=True, text=True).stdout.splitlines()[2:]
        figs = [0] * len(pages)
        for ln in listed:
            figs[int(ln.split()[0]) - 1] += 1
        orphans = [i + 1 for i in range(len(pages)) if leads[i] != figs[i]]
        check("a short lead-in paragraph never ends a page without its figure", not orphans and sum(figs) == 24,
              f"pages {orphans}; leads {leads}; figures {figs}")
        # A long lead-in paragraph must still split across pages (only its last lines go with the figure), not be
        # pushed whole to the next page leaving a large gap.
        blocks = "".join("正文" * (k * 23) + f"\n\n长{k:02d}" + "文" * 300 + f"尾{k:02d}。\n\n⟦IMG:f{k}⟧\n\n" for k in range(1, 25))
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text("# 长引译\n\n" + blocks)
        rd.render(work, SimpleNamespace(only=chap["id"], out=None, title=None, bg="#ffffff", fg="#000000", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
        pages = subprocess.run(["pdftotext", str(prev), "-"], capture_output=True, text=True).stdout.split("\f")
        page_of = lambda w: next(i for i, pg in enumerate(pages) if w in pg)
        split = [k for k in range(1, 25) if page_of(f"长{k:02d}") != page_of(f"尾{k:02d}")]
        listed = subprocess.run(["pdfimages", "-list", str(prev)], capture_output=True, text=True).stdout.splitlines()[2:]
        fig_pages = sorted(int(ln.split()[0]) - 1 for ln in listed)
        check("a long lead-in paragraph still splits across a page break", len(split) >= 3, str(split))
        check("a split long lead-in paragraph ends on its figure's page", all(page_of(f"尾{k:02d}") in fig_pages for k in split), f"{split} {fig_pages}")
        (work / "images.json").unlink()
        # Last, because it corrupts a section's md: an equation KaTeX cannot parse (an undefined command) renders
        # as red source; the render must fail on it rather than ship it silently.
        chap = next(s for s in meta["sections"] if s["kind"] == "chapter")
        (work / "translated" / (Path(chap["file"]).stem + ".md")).write_text("# 坏公式译\n\n见 \\(\\zzbadmacro\\)。\n\n" + "正文。" * 60)
        try:
            rd.render(work, SimpleNamespace(only=None, out=str(Path(d) / "bad-zh.pdf"), title="小书", bg="#181a1d", fg="#e1ddd5", font_size=9.25, eq_scale=0.6, bold_factor=1.25))
            guarded = False
        except SystemExit as e:
            guarded = "failed to render" in str(e)
        check("render fails on an unparseable equation (KaTeX error guard)", guarded)


def test_render_import():
    with tempfile.TemporaryDirectory() as d:
        result = subprocess.run([sys.executable, "-c", "import runpy,sys; runpy.run_path(sys.argv[1])", str(HERE / "render.py")], cwd=d, capture_output=True, text=True)
    check("renderer loads by file path from another skill", result.returncode == 0, result.stderr)


def test_compiles_clean():
    # Python prints a SyntaxWarning for an invalid escape (e.g. "\\m" in a non-raw string) on every run.
    import warnings
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        try:
            compile((HERE / "render.py").read_text(), "render.py", "exec")
            clean = ""
        except (SyntaxError, SyntaxWarning) as e:
            clean = str(e)
    check("render.py compiles without warnings", not clean, clean)


def main():
    lint = subprocess.run([sys.executable, str(HERE.parent / "tests" / "test_lint_md.py")], capture_output=True, text=True)
    check("Markdown lint regressions", lint.returncode == 0, lint.stdout + lint.stderr)
    formatter = subprocess.run(["uv", "run", str(HERE.parent / "tests" / "test_format_check.py")], capture_output=True, text=True)
    check("PDF caption format check regressions", formatter.returncode == 0, formatter.stdout + formatter.stderr)
    test_render_import()
    test_compiles_clean()
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
