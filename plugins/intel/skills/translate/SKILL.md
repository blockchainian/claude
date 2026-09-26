---
name: translate
description: >
  Translate a whole English book PDF into a Chinese PDF that keeps the original's format: the same cover
  page, page size, chapter structure, running heads, folios, a clickable 目录 page and PDF bookmarks (Cover,
  目录, one per chapter). Works from the PDF outline, or from the printed contents page when there is none,
  and on two-up scans. Chapters are translated in parallel by gpt-6-luna through `codex exec`, one call
  per chapter, and typeset with headless Chrome in Baskerville + Songti SC. Use for "/translate <book.pdf>",
  "把这本书翻译成中文", "translate this book", or to re-render an already translated book after editing its
  Markdown. NOT for a single page or article (just translate it inline), and not for digesting or
  summarizing a source (use digest).
---

# Translate — an English book PDF into a Chinese PDF in the same format

The book's sections drive everything: each section (from the PDF outline, else from the printed contents
page) is one Luna call, and the finished sections are typeset into one book that copies the source's page size,
chapter openers, running heads, roman/arabic folios and cover page.

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at `${CLAUDE_PLUGIN_ROOT}/skills/translate`.
Work lives in `<book dir>/.translate/<slug>/` (hidden, resumable); the deliverable is `<book>-zh.pdf` next to
the source. When the book's folder is not writable (macOS keeps this process out of some folders, e.g.
`~/Downloads`), `extract.py` copies the book to `~/Documents/translate/<slug>/` and everything, including the
result, lands there; it says so on stderr. Never leave other copies next to the book.

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
roman folios), `chapter` (第N章, arabic folios from 1), `back` (acknowledgments, appendix, letters). Text is
cleaned: running heads, section-numbered running feet ("DEFINITIONS 2-1") and folios dropped, hyphenation
undone, paragraphs rebuilt, indented blocks marked `> `.

Two-up scans (a landscape sheet holding two book pages) are split into single pages first, into
`<work>/pages.pdf`; every later step, including the cover and the page size, uses that file.

Without an outline, the sections come from the printed contents page: lines with dot leaders give the titles
(`Section 12  Vacations ..... 12-1`, `Chapter Three ..... 27`, `LOA 4 ..... LOA 4-1`), and each start page is
the first page after the previous section whose top lines carry that label or title (a `LOA` entry matches a
page opening with "Letter of Agreement"); a trailing index is detected by its leader lines. stderr says how
many entries were located and names the ones that were not. `Section N` and `Chapter N` entries become
numbered chapters; other labels keep their label (`LOA 4: Title`).

Check the listing before spending calls: a section with suspiciously few or many words, or a title parsed
wrong, means a start page is off. Exit 2 = neither outline nor contents page worked: write `sections.json` by
hand (`[{"title": "COVER", "start": 1}, {"title": "Preface", "start": 10}, ...]`, 1-based pages of the file
named in the message, in order) and rerun with `--sections`.

## 2. Glossary (short, before translating)

Write `<work>/glossary.md`: the book's key terms and every chapter title with its Chinese rendering, plus the
authors' names. The chapter titles become the 目录 and the bookmarks, so pinning them here is what keeps the
in-text references, the contents page and the bookmarks consistent. Ask the user only when a term is a real
choice (e.g. a coined term with two accepted renderings); otherwise decide and note it in the summary.

## 3. Translate (background, parallel)

```bash
python3 "${CLAUDE_PLUGIN_ROOT}/skills/translate/scripts/translate.py" <work> --glossary <work>/glossary.md \
  > <work>/translate.log 2>&1
```

Run it with `run_in_background`. Defaults: `gpt-6-luna`, effort `low`, Fast service tier (`priority`), 20
sections in flight, one no-tool `codex exec` per section in a private `CODEX_HOME`. Measured: a 250-page trade
book (27 sections of 1.5–3.5k words) is back in about a minute, a 380-page agreement (54 sections, up to 12k
words) in about five; 7–15k input and 2–4k output tokens per section. Each answer is checked (starts with `# title`, at least 0.9 Chinese
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
comes from the source's first page. Colors default to a dark reading page: the background follows the iTerm2
default profile's dark-mode background when iTerm2 is installed (else near-black), the text is `#606e6a`, a
cool gray chosen for long reading on black (neutral or warm grays glare on a black page even when dimmed;
a saturated terminal foreground is too dark for body text). `--bg/--fg` take `#rrggbb`, `iterm` (either
terminal color) or `source` (sampled from a body page of the book). Type is Baskerville for Latin and Songti SC for Chinese. Chapter openers carry the
第N章 label, the title and a drop cap; body pages carry the chapter title as running head and a folio (roman in
front matter, arabic from chapter 1). Every 目录 row is a link to its section, and the bookmarks are flat:
Cover, 目录, one per section. Sections without a
translation yet are skipped with a warning, so a partial book renders at any time.

Then look, do not assume: render the cover, the 目录, one chapter opener and one body page to PNG
(`pdftoppm -r 45`) and check that the header is masked on openers, folios restart at chapter 1, and 目录 page
numbers match the bookmarks (pikepdf: the 目录 page's `/Annots` links point at the same pages). Open the PDF for
the user.

## Editing after the fact

The translation is plain Markdown in `<work>/md/`: fix a sentence there and rerun step 4 (seconds). Retranslate
one section with `translate.py <work> --force --only <id>`. A different look (light theme, other margins) is
`--bg/--fg` or an edit to `css()` in `render.py`.

## Notes

- Source PDFs are often scans with an OCR layer: the extractor cannot tell a sub-heading from a short line, so
  Luna is told to promote title-like lines to `##`, repair OCR misreads and drop-cap damage, and rebuild
  paragraphs. Figures and tables do not survive (a rate table comes out as prose); say so in the summary when
  the source has them, and point the reader at the source pages for numbers.
- Chrome cannot reset the page counter mid-document, so front matter and body are typeset as two documents and
  joined with pikepdf; the running head on opener pages is masked by a background rectangle after the fact.
- The PDF text layer keeps the tiny invisible `⟦S04⟧` markers the page map uses; they are 1pt and transparent.
