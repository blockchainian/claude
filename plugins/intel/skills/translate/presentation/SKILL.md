---
name: translate-presentation
description: >
  Translate an English lecture slide deck PDF (Keynote, PowerPoint or Google Slides export with a text layer) into a
  Chinese PDF page for page: the same pages, figures and layout, the translate-epub skill's dark theme (page #000409, ink
  #6e7f7a, 黑体), each prose text box replaced in place by fitted Chinese, screenshots and diagrams recoloured, photos
  kept. Text inside figures, code and names stay English. Translation goes through `codex exec`. Use for
  "/translate-presentation <deck.pdf>", "把这份幻灯片翻译成中文", "translate these lecture slides", or a batch of decks.
  NOT for decks whose pages are pictures with no text layer (scans or rasterised slides: they need OCR boxes first),
  NOT for papers or notes (translate-paper), and NOT for EPUB books (translate).
---

# Translate slides

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The scripts import from the
sibling `translate-epub` skill (theme helpers in `render.py`, `runCodex` in `translate.mjs`), which imports from
`fetch-x-mentions`, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default ~/.codex | No | Shell environment |
| `INTEL_STATE_DIR` | State root; the cut system fonts are cached under `translate-presentation/fonts`; default `~/.local/state/intel` | Optional | `~/.config/intel/.env` |

## Setup

Run the translate-epub skill's setup (`uv`, poppler, the codex login). The fonts come from macOS: Heiti SC Light and
Medium, and Menlo.

```bash
"$SKILL_DIR/../epub/scripts/setup.sh"
```

## Translate one deck

```bash
"$SKILL_DIR/scripts/slides.py" extract <deck.pdf> <work>
"$SKILL_DIR/scripts/translate_slides.mjs" <work> --title "<course>, <lecture>" [--glossary g.md]
"$SKILL_DIR/scripts/slides.py" render <deck.pdf> <work> <out.pdf>
```

`extract` prints the page, block and figure-label counts and writes `<work>/blocks.json` and
`<work>/figure-labels.json`, the blocks left English as figure text; skim the labels when a deck is new to you.
`translate_slides.mjs` writes `<work>/zh.json`, keyed by source text, and is resumable: a block whose tags keep
coming back wrong is translated once more without its style runs, and one whose code or pictures still change is
left English and logged as `REJECTED`. `render` prints a report, each line with its page: `shrunk to` (a block
scaled to fit), `labels re-inked for contrast` (text left in place given a legible ink) and `still low contrast`
(text that may read poorly).

A batch runs at most two decks at once, to keep memory safe, and skips decks that already have an output:

```bash
"$SKILL_DIR/scripts/batch.sh" <list.tsv> <work root> <out root> [jobs]
```

The list's last column is the deck path, and `*-all-animations.pdf` decks are skipped. Each deck gets one line in
the batch's output: `OK`, `PARTIAL` (some texts left English), `FAILED <step>` or `SKIP`, plus its log under
`<work root>/logs/`.

## Translate a picture-only deck

A deck whose pages are pictures needs OCR text boxes first: the translate-scan skill's `slide_boxes.mjs` writes
one JSON per deck (per page: boxes with a top-left `bbox` in points, `text`, `font_size`, `lines`, `in_figure`,
`matched`). Boxes that are figure labels or that the transcription did not match stay as they are.

```bash
"$SKILL_DIR/scripts/ocr_slides.py" extract <deck.pdf> <boxes.json> <work>
"$SKILL_DIR/scripts/translate_slides.mjs" <work> --title "<course>, <lecture>"
"$SKILL_DIR/scripts/ocr_slides.py" render <deck.pdf> <work> <out.pdf>
"$SKILL_DIR/scripts/ocr_batch.sh" <boxes root> <source root> <work root> <out root> [jobs]
```

Each page picture is recoloured like a screenshot; each translated box is painted over in the colour behind it
and the Chinese is set in its place, with the ink colour measured from the picture, headings in bold, and simple
LaTeX (sub and superscripts, Greek, ≤) as HTML.

## Check the result

Look at every page as a contact sheet, rendered by macOS's own engine as well as MuPDF, since Preview shows a
JPEG 2000 tile's transparency and MuPDF does not:

```bash
"$SKILL_DIR/scripts/sheet.py" <out.pdf> <sheet.png> --cols 6 --width 240 --apple
```

Crop a page at full size before judging a detail.

## How it works

The page is translated in place, so layout and figures stay as the author made them; rebuilding pages would lose
vector diagrams and paint order, and source decks are rarely published.

- **Extract**: text blocks from PyMuPDF. Runs side by side on one baseline split into columns; blocks continuing one
  paragraph are merged, never across a box edge. Non-dominant styles become `<sN>` tags, code `<cN>`, list items
  `<br>`; runs that cannot be re-typeset (emoji, unmapped fonts, code extracted as `23445#>` soup) become `⟦En⟧`
  picture tokens. Code listings, symbol-only blocks, figure text (labels inside a cluster of shapes, or on a lone
  picture), text a later picture covers and text parked off the page stay as they are. Rotated pages are
  baked upright first. MIT OpenCourseWare's closing licence page is dropped.
- **Recolour** (pikepdf, by use): fills of light boxes vanish into the page, so text sits on the plain dark page;
  coloured shapes keep their hue darkened; strokes invert; text left in place takes the body ink, coloured text
  stays bright. On a background-picture page the full-page fill keeps its colour. Screenshots and diagrams on a
  light ground are recoloured pixel by pixel; photos, icons and background pictures are not.
- **Render**: each translated block's spans are redacted (with WordArt outline glyphs inside the box) and the
  Chinese is set with `insert_htmlbox` in the block's box, shrinking to fit, in fonts cut to the deck's characters
  (a full CJK font embedded per insert costs gigabytes). Ink is checked against the colour behind the block and
  changed when contrast is under 3. Pictures are drawn over invisible placeholder glyphs, links are restored, and
  fonts are subset on save.
