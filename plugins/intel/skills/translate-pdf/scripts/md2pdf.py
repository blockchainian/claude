#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["markdown>=3.5"]
# ///
# ABOUTME: Renders a translated Markdown document to HTML, with Mermaid fences drawn as diagrams, and prints it to a
# ABOUTME: PDF with html2pdf.py. Usage: md2pdf.py <doc.md> <out.pdf>
import html, re, subprocess, sys
from pathlib import Path
import markdown

src, pdf = Path(sys.argv[1]), Path(sys.argv[2]).resolve()
body = markdown.markdown(src.read_text(), extensions=["fenced_code", "tables", "sane_lists"])
# Mermaid fences become diagrams that mermaid.js draws in the page before html2pdf prints it.
body = re.sub(r'<pre><code class="language-mermaid">(.*?)</code></pre>', lambda m: f'<div class="mermaid">{html.unescape(m.group(1))}</div>', body, flags=re.S)
MERMAID = ('<script type="module">import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";'
           'mermaid.initialize({startOnLoad: true, theme: "base", flowchart: {useMaxWidth: false, nodeSpacing: 30, rankSpacing: 22, padding: 6}, block: {useMaxWidth: false}, themeVariables: {background: "#000409", primaryColor: "#0b1414",'
           ' primaryTextColor: "#8a9f98", primaryBorderColor: "#2c3836", lineColor: "#6e7f7a", textColor: "#6e7f7a",'
           ' fontFamily: "Menlo, monospace", fontSize: "10px"}});</script>'
           '<style>.mermaid, .mermaid * { font-family: Menlo, monospace !important; font-size: 10px !important; line-height: 1.3 !important; } .mermaid { margin: 0.6em 0 1em; text-align: center !important; } .mermaid svg { background-color: transparent !important; } .mermaid .edgeLabel, .mermaid .edgeLabel p, .mermaid .labelBkg { background-color: #000409 !important; }</style>')
html = pdf.with_suffix(".html")
html.write_text(f'<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>{pdf.stem}</title>{MERMAID}</head><body>{body}</body></html>')
r = subprocess.run([str(Path(__file__).resolve().parent / "html2pdf.py"), str(html)], capture_output=True, text=True)
html.unlink(missing_ok=True)
print(r.stdout.strip() or r.stderr.strip()[-800:])
# A Mermaid diagram that fails to parse renders as an error box instead of the diagram.
if pdf.exists():
    text = subprocess.run(["pdftotext", str(pdf), "-"], capture_output=True, text=True).stdout
    if "Syntax error" in text or "mermaid version" in text:
        print(f"WARN {pdf} mermaid_error")
