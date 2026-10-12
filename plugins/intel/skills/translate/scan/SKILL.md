---
name: translate-scan
description: >
  Translate a scanned or handwritten PDF (pages that are images, with no usable text layer) into a Chinese PDF in
  the translate-epub skill's book format: codex reads each page image into Markdown (text, headings, math as LaTeX,
  figures cropped with their captions), macOS Vision cross-checks the words, and the translate-paper pipeline
  translates and typesets it. Also produces per-page text boxes (position, text, font size) for image-only slide
  decks, for the slides pipeline to render. Use for "/translate-scan <file.pdf>", "translate this scanned paper",
  "翻译这份扫描件/手写笔记", or "text boxes for this image-only deck". NOT for PDFs with a real text layer (use
  translate-paper), EPUB books (use translate-epub), or slides with a text layer (use translate-presentation).
---

# Translate scan

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The scripts import from
the sibling `translate-epub`, `translate-paper` and `fetch-x-mentions` skills, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default ~/.codex | No | Shell environment |
| `INTEL_STATE_DIR` | Work dirs go under `translate-scan/`; default `~/.local/state/intel` | Optional | `~/.config/intel/.env` |
| `INTEL_OUTPUT_DIR` | Finished PDFs go under `translate/` unless `--out-dir`; default `~/Documents` | Optional | `~/.config/intel/.env` |
| `INTEL_DATA_DIR` | The compiled Vision helper and codex scratch dirs; default `~/.local/share/intel` | Optional | `~/.config/intel/.env` |

## Setup

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It runs the translate-epub skill's setup (poppler, uv, codex login, Chrome) and checks ImageMagick, the Swift compiler
(the macOS Vision helper is compiled on first use) and the translate-paper skill. macOS only.

## Translate scanned documents

Confirm first that the PDF has no text layer (`pdftotext` returns almost nothing per page) and whether it is a
printed paper or handwritten notes; classifiers mix the two.

```bash
"$SKILL_DIR/scripts/scan.py" <file.pdf> ... --kind paper|handwritten [--out-dir DIR] [--mirror ROOT] [--jobs 2]
```

Each file runs through six steps in its work dir `<INTEL_STATE_DIR>/translate-scan/<name>/`, logged in `run.log`;
a rerun resumes at the first step without output.

1. `pdftoppm` renders the pages: 300 dpi for printed scans, 200 dpi for handwriting.
2. `ocr.mjs` sends each page image in one codex call (gpt-6-luna, effort low, 12 in flight) and gets the page back
   as Markdown. A figure comes back as a `[FIGURE x0,y0,x1,y1]` box followed by a bold caption; a printed page that
   opens mid-paragraph starts with `<<CONT>>`. A page with an unclosed formula is read again, up to three times.
3. `ocr.mjs --proofread` and `proof.mjs` check the words: macOS Vision reads the page independently, and every spot
   outside math where its words differ from codex's is cropped at full resolution and read blind by codex, one call
   per page. A spot changes only when the blind reading agrees with Vision. The proofreader never sees the draft,
   because a draft-aware reading confirms the draft's errors and a whole-page re-read "fixes" the author's own typos.
4. `assemble.py` crops the figures (walled off from the transcribed text above and below, grown along ink the
   model's box cut off, snapped to the ink), joins paragraphs across pages, drops publisher and site boilerplate
   pages, shortens URLs into links, moves footnotes after a `---` line, lifts the byline out of the body for the
   cover, splits sections at `##` headings and writes the work dir that translate-paper reads. Handwritten notes
   stay one section titled from the file name; their sketches become black ink on white with the graph-paper grid
   removed, so the render recolours them to the page's ink.
5. translate-paper' `translate_paper.mjs` translates (paper rules, KaTeX check, automatic glossary, the cover's
   author lines); `restore_verbatim.py` then puts Markdown tables (data stays in English) and section numbers back
   exactly as in the source.
6. The translate-epub skill's `lint_md.py`, translate-paper' `render_paper.py` (a paper gets a title cover and a 目录,
   notes get neither) and the translate-epub skill's `format_check.py` finish the PDF.

The output is `<out-dir>/<name>.pdf`; with `--mirror ROOT` it is `<out-dir>/<path below ROOT>`, so a tree of course
folders keeps its shape. A `FAILED` line names the step; the end of `run.log` has the cause. A render that fails on
an equation means the translation still holds broken LaTeX: fix it in `translated/` and rerun, as the translate
skill describes. Before calling a file done, open the PDF for the user and look at one figure page and one page
with math.

The scripts also run one step at a time, taking the work dir as their argument: `ocr.mjs <work> [--mode
typeset|handwritten|slide] [--proofread]`, `assemble.py <work> --source <pdf> [--title T] [--one-section] [--ink]`,
`restore_verbatim.py <work>`.

## Text boxes for image-only slide decks

```bash
"$SKILL_DIR/scripts/slide_boxes.mjs" <deck.pdf> <out.json> [--work DIR] [--jobs 12]
```

The page images are the deck's own full-page rasters when each page has exactly one, otherwise a 150 dpi render.
Boxes are macOS Vision's line boxes, merged where Vision split or read a line twice, and grouped into paragraphs by
layout and by the paragraph and bullet openings in codex's transcription:

```
{pdf, page_size: [w, h] pt,
 pages: [{index, boxes: [{bbox: [x0, y0, x1, y1] pt from the top left, text, vision_text, matched,
                          font_size, lines: [{bbox, text}], in_figure}]}]}
```

`text` is codex's transcription of the paragraph (Markdown and LaTeX, such as `\(V_{OL}\)`) when it matches the
box (`matched: true`), otherwise Vision's raw text. `font_size` is the median line-box height in points; on decks
with a text layer the true size divided by the box height has a median of 1.00. `in_figure` marks labels inside a
diagram.

## Tests

```bash
cd "$SKILL_DIR" && uv run --with pytest --with pillow pytest -q tests && node --test tests/*.test.mjs
```

The fixtures are real OCR output: a 1991 scanned paper's first page, a course site's terms page, and one 6.1910
slide's Vision lines and transcription.
