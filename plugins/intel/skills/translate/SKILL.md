---
name: translate
description: >
  Translate a whole English EPUB book into a Chinese PDF that keeps the original's format: the same cover
  image, page size, chapter structure, running heads, folios, a clickable 目录 page and PDF bookmarks (Cover,
  目录, one per chapter), with each section's bold, emphasis, sub/superscripts and, on request, images. Chapters
  are translated in parallel through `codex exec`, one call per chapter. Use for "/translate <book.epub>",
  "把这本书翻译成中文", "translate this book", or to re-render an already translated book after editing its
  Markdown. Needs the book as an EPUB — get it with the download-book skill first. NOT for a single page or
  article (just translate it inline), and not for digesting or summarizing a source (use digest).
---

# Translate — an EPUB book into a Chinese PDF in the same format

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The
scripts import from the sibling `fetch-x-mentions` skill, so keep the whole intel plugin installed.

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default ~/.codex | No | Shell environment before running the command; no automatic `.env` loading |
| `INTEL_OUTPUT_DIR` | Output root; finished books go under `translate/`; default `~/Documents` | Optional | `~/.config/intel/.env` |

The book is usually the download-book skill's EPUB under `books/` in the output root; a book you have only as a PDF must
be fetched as an EPUB first. Work lives in `~/.local/state/intel/translate/<slug>/` (`<INTEL_STATE_DIR>/translate/<slug>/`
when set). Keep it: it is resumable, and the translated Markdown in it is costly to redo. The finished book is
`<title-slug>.pdf` under `translate/` in the output root, named after the
book's main title without its subtitle.

## Setup

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It installs what is missing and reports whether `codex` is logged in and Chrome is present. Translation runs on
the user's ChatGPT plan through `codex`; when its quota is out, wait or pass `--model` to another codex model.

## 1. Extract

```bash
"$SKILL_DIR/scripts/extract.mjs" <book.epub>
```

Add `--keep-images` when the book's figures or equations are stored as images (a textbook); without it,
extraction is text-only and figures and tables are lost, so a rate table comes out as prose.

```bash
"$SKILL_DIR/scripts/extract.mjs" <book.epub> --keep-images
```

Extraction prints one line per section (`id kind title: words`) and the work dir. The kinds are `contents` and
`skip` (not translated), `front` (roman folios), `chapter` (第N章, arabic folios from 1) and `back`. Check the
listing before spending calls: a section with suspiciously few or many words, or a wrongly parsed title, means a
spine file was mis-grouped, and odd EPUBs can lose bodies or type them `front`. A warning that chapter numbers
skip or repeat means an opener was not recognised.

Fix any of these in `<work>/sections.json` before translating:

- To skip a section, drop its entry or delete its `file`: translation and rendering skip only sections without a
  `file`, whatever their `kind`. Per-chapter endnotes filed under "Chapter N" nav titles are a common case.
- A body section typed `front` becomes `kind: "chapter"`, plus `label: "第N章"` when numbered.

Edit `sections.json` in place. Extract refuses a work dir that already holds one, because re-extracting
renumbers the ids the translations are keyed to.

## 2. Glossary

Write `<work>/glossary.md` with the book's key terms, every chapter title with its Chinese rendering, and the
authors' names. The chapter titles become the 目录 and the bookmarks, so pinning them here keeps in-text
references, the contents page and the bookmarks consistent. Ask the user only when a term is a real choice (a
coined term with two accepted renderings); otherwise decide and note it in the summary.

## 3. Translate

```bash
"$SKILL_DIR/scripts/translate.mjs" <work> --glossary <work>/glossary.md \
  > <work>/translate.log 2>&1
```

Run it in the background and wait for it to finish. Claude Code and Codex both call the same logged-in `codex exec`
CLI; do not replace it with host agents. A trade book is back in a few minutes. Rerunning skips sections whose
`.md` exists; `--force --only 04,05` redoes chosen ones.

A section whose answer fails the checks twice is kept as `<id>.rejected.md` and reported as `FAILED`. Short or
heading-less sections and MathML-heavy ones are sometimes rejected while complete: check the text against the
source, add a `# 标题` line if it lacks one, and accept it by renaming `<id>.rejected.md` to the section's
`<id>-<slug>.md`.

While it runs, preview any finished section as its own PDF, and read one chapter against the source before the
rest lands: wrong register, a dropped paragraph or a glossary miss is cheaper to fix now, by editing the glossary
and rerunning `--force --only`.

```bash
"$SKILL_DIR/scripts/render.py" <work> --only 04
```

When every section is back, lint the Markdown and fix each reported line in it:

```bash
"$SKILL_DIR/scripts/lint_md.py" <work> [id ...]
```

An unclosed inline tag matters most, since one open `<code>` turns every later section into code. A section with
many "unconverted math" hits had its LaTeX conversion skipped: retranslate it with
`translate.mjs <work> --force --only <id> --effort medium`.

## 4. Render the book

```bash
"$SKILL_DIR/scripts/render.py" <work> --title "<中文书名>"
```

Rendering needs network, because KaTeX loads from a CDN. Sections without a translation yet are skipped with a
warning, so a partial book renders at any time. Pages default to a dark reading theme; `--bg/--fg` change it.
For a book whose figures are grayscale screenshots or photos, add `--no-recolor` so images keep their own
colours on a white plate; for one whose chapters open with numbered articles, add `--no-dropcap`.

Then look, do not assume: render the cover, the 目录, one chapter opener and one body page to PNG
(`pdftoppm -r 45`) and check that the header is masked on openers, folios restart at chapter 1, and 目录 page
numbers match the bookmarks (pikepdf: the 目录 page's `/Annots` links point at the same pages). Then run the
format check on the PDF:

```bash
"$SKILL_DIR/scripts/format_check.py" <book.pdf> > <work>/format-check.txt
```

Loose lines in its report are for information; every other finding is a layout defect to fix before handing the
book over. Open the PDF for the user, and say in the summary when the source had figures or tables that a
text-only extract dropped.

## 5. Fix equations the render rejects

An equation KaTeX cannot parse renders as raw red LaTeX, so `render.py` exits non-zero and lists each one as
`section NN (page P)`. A render that exits non-zero is not done: never hand that book to the user. Fix each
equation in the section's Markdown in `<work>/translated/`, near the reported spot, and re-run step 4 until it
exits 0:

- **Unbalanced braces, or a `\begin{aligned}` row with CJK punctuation outside `\text`**: balance the braces and
  wrap the Chinese in `\text{…}`.
- **An `⟦IMG:key⟧` token inside `\(..\)`/`\[..\]`** is a symbol the EPUB stored as an image. Open
  `<work>/images/<file>` (from `images.json`) to see the glyph, such as an accented q̂, and write it as LaTeX
  (`\hat{q}`); keep `⟦IMG⟧` tokens only for real figures, outside math.
- **Markdown (`**bold**`) or a stray `\(` inside math**: move the markup outside the math and restructure the
  line.

Editing the Markdown is faster and more faithful than retranslating; retranslate only when the prose itself is
wrong.

## Editing after the fact

The translation is plain Markdown in `<work>/translated/`: fix a sentence there and rerun step 4. A section's
title is its Markdown's first `# ` line, not the `sections.json` title, so fix titles in the Markdown. Retranslate
one section with `translate.mjs <work> --force --only <id>`. A different look beyond `--bg/--fg` is an edit to
`css()` in `render.py`.
