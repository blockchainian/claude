---
name: translate
description: >
  Translate a whole English book PDF into a Chinese PDF that keeps the original's format: the same cover
  page, page size, colors, chapter structure, running heads, folios, a 目录 page and PDF bookmarks (Cover,
  目录, one per chapter). Chapters are translated in parallel by gpt-6-luna through `codex exec`, one call
  per chapter, and typeset with headless Chrome in Baskerville + Songti SC. Use for "/translate <book.pdf>",
  "把这本书翻译成中文", "translate this book", or to re-render an already translated book after editing its
  Markdown. NOT for a single page or article (just translate it inline), and not for digesting or
  summarizing a source (use digest).
---

# Translate — an English book PDF into a Chinese PDF in the same format

The book's PDF outline drives everything: each outline entry is a section, each section is one Luna call,
and the finished sections are typeset into one book that copies the source's page size, background and
text colors, chapter openers, running heads, roman/arabic folios and cover page.

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at `${CLAUDE_PLUGIN_ROOT}/skills/translate`.
Work lives in `<book dir>/.translate/<slug>/` (hidden, resumable); the deliverable is `<book>-zh.pdf` next to
the source. Never leave other copies next to the book.

## Setup (automatic, idempotent)

```bash
bash "${CLAUDE_PLUGIN_ROOT}/skills/translate/scripts/setup.sh"
```

Installs `poppler` (pdftotext/pdftoppm) and `uv` when missing; reports whether `codex` is logged in and Chrome
is present. Luna runs on the user's ChatGPT plan through `codex`; when its quota is out, wait or pass
`--model` to a different codex model.

## 1. Extract

```bash
"${CLAUDE_PLUGIN_ROOT}/skills/translate/scripts/extract.py" <book.pdf>
```

Prints one line per section (`id kind pages title: words`) and the work dir. Kinds: `cover` (page copied
as-is), `contents` and `skip` (Index, Notes: not translated; the 目录 is regenerated), `front` (preface,
roman folios), `chapter` (第N章, arabic folios from 1), `back` (acknowledgments, appendix). Text is cleaned:
running heads and folios dropped, hyphenation undone, paragraphs rebuilt, indented blocks marked `> `.

Check the listing before spending calls: a chapter with suspiciously few words, or a title parsed wrong, means
the outline is off. Exit 2 = no usable outline: write `sections.json` by hand from the printed table of
contents (`[{"title": "Preface", "start": 10}, ...]`, 1-based PDF pages, in order, including `COVER` at 1 and
`CONTENTS`) and rerun with `--sections`.

## 2. Glossary (short, before translating)

Write `<work>/glossary.md`: the book's key terms and every chapter title with its Chinese rendering, plus the
authors' names. The chapter titles become the 目录 and the bookmarks, so pinning them here is what keeps the
in-text references, the contents page and the bookmarks consistent. Ask the user only when a term is a real
choice (e.g. traction → 牵引力 vs 增长动力); otherwise decide and note it in the summary.

## 3. Translate (background, parallel)

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/translate/scripts/translate.py" <work> --glossary <work>/glossary.md \
  > <work>/translate.log 2>&1
```

Run it with `run_in_background`. Defaults: `gpt-6-luna`, effort `low`, Fast service tier (`priority`), 20
sections in flight, one no-tool `codex exec` per section in a private `CODEX_HOME`. Measured on a 245-page
business book (27 sections, 1.5–3.5k words each): every section back in about 40 s wall clock, 7–15k input
tokens and 2–4k output tokens per section. Each answer is checked (starts with `# title`, at least 0.9 Chinese
characters per English word) and retried once; a section that still fails is kept as `<id>.rejected.md` and
reported as `FAILED`. Rerunning skips sections whose `.md` exists (`--force` redoes them, `--only 04,05`
narrows).

While it runs, preview any finished section as its own PDF (own page numbers, no cover):

```bash
"${CLAUDE_PLUGIN_ROOT}/skills/translate/scripts/render.py" <work> --only 04
```

Read one finished chapter against the source before the rest lands: wrong register, a dropped paragraph or a
glossary miss is cheaper to fix by editing the glossary and rerunning `--force --only` now than after the book
is assembled.

## 4. Render the book

```bash
"${CLAUDE_PLUGIN_ROOT}/skills/translate/scripts/render.py" <work> --title "<中文书名>"
```

Writes `<book>-zh.pdf`: the source's cover page, a 目录 with folios, then every translated section. Page size
comes from the source's first page. Colors default to the user's choice, picked page by page on 2026-09-25:
background = the iTerm2 default profile's dark-mode background (`--bg iterm`), text = `#606e6a`, a cool gray
with a hint of the terminal's teal. Neutral or warm grays glare on a black page even when dimmed, and the
terminal's own teal is too dark to read as body text; that gray sits between the two. `--bg/--fg` take
`#rrggbb`, `iterm` (either terminal color) or `source` (sampled from a body page of the book). Type is Baskerville for Latin and Songti SC for Chinese. Chapter openers carry the
第N章 label, the title and a drop cap; body pages carry the chapter title as running head and a folio (roman in
front matter, arabic from chapter 1). Bookmarks are flat: Cover, 目录, one per section. Sections without a
translation yet are skipped with a warning, so a partial book renders at any time.

Then look, do not assume: render the cover, the 目录, one chapter opener and one body page to PNG
(`pdftoppm -r 45`) and check that the header is masked on openers, folios restart at chapter 1, and 目录 page
numbers match the bookmarks. Open the PDF for the user.

## Editing after the fact

The translation is plain Markdown in `<work>/md/`: fix a sentence there and rerun step 4 (seconds). Retranslate
one section with `translate.py <work> --force --only <id>`. A different look (light theme, other margins) is
`--bg/--fg` or an edit to `css()` in `render.py`.

## Notes

- Source PDFs are often scans with an OCR layer: the extractor cannot tell a sub-heading from a short line, so
  Luna is told to promote title-like lines to `##`, repair OCR misreads and drop-cap damage, and rebuild
  paragraphs. Figures and tables do not survive; say so in the summary when the source has them.
- Chrome cannot reset the page counter mid-document, so front matter and body are typeset as two documents and
  joined with pikepdf; the running head on opener pages is masked by a background rectangle after the fact.
- The PDF text layer keeps the tiny invisible `⟦S04⟧` markers the page map uses; they are 1pt and transparent.
