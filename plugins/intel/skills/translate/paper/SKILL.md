---
name: translate-paper
description: >
  Translate a typeset English PDF with a text layer, such as a research paper (one or two columns), lecture or
  recitation notes, a textbook chapter, an outline or a handout, into a Chinese PDF in the translate-epub skill's book
  format. Reading order, headings, lists, code, tables, LaTeX math, footnotes and references are kept, and figures
  are cropped from the page with their captions. MinerU parses the layout, the translate-epub skill's codex runner
  translates each section, and its render.py typesets the result. Use for "/translate-paper <paper.pdf>",
  "把这篇论文翻译成中文", "translate these lecture notes", or a batch of course PDFs.
  NOT for scanned or handwritten PDFs without a text layer (use translate-scan), slides (use translate-presentation),
  EPUB books (use translate-epub), or web pages (use translate-pdf).
---

# Translate papers

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call. The scripts import from
the sibling `translate-epub` and `fetch-x-mentions` skills, so keep the whole intel plugin installed.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `INTEL_STATE_DIR` | State root; work dirs go under `translate-paper/`, shared course glossaries under `translate-glossaries/`; default `~/.local/state/intel` | Optional | `~/.config/intel/.env` |
| `MINERU_KIT` | MinerU's `mineru-kit` binary; default the venv setup.sh installs | Optional | Shell environment |
| `MINERU_VENV` | Where setup.sh installs MinerU; default `~/.local/share/intel/translate-paper/mineru` | Optional | Shell environment |
| `TRANSLATE_GLOSSARIES` | Directory of shared glossaries, one `english: 中文` line per term | Optional | Shell environment |

## Setup

```bash
"$SKILL_DIR/scripts/setup.sh"
```

It installs MinerU into its own venv, KaTeX for the formula check, and runs the translate-epub skill's setup (poppler,
uv, codex login, Chrome). MinerU's models download on first use.

## One PDF

```bash
WORK=~/.local/state/intel/translate-paper/<name>
"$SKILL_DIR/scripts/pdf2work.py" <paper.pdf> "$WORK"
"$SKILL_DIR/scripts/translate_paper.mjs" "$WORK" > "$WORK/translate.log"
"$SKILL_DIR/scripts/render_paper.py" "$WORK" --out <out.pdf>
```

`pdf2work.py` runs MinerU (`mineru-kit parse --tier advanced --disable-image-analysis`; with image analysis on,
MinerU invents data tables for charts) and builds the work dir from its `middle_json.json`:

- Math becomes `\(..\)` and `\[..\]`. A footnote mark is `<sup>N</sup>` only when N opens one of the document's
  footnotes; any other raised digit is an exponent (n²), and a raised 0 is a prime (k′).
- Footnotes go after a `---` line at the end of their section, including first-page notes that MinerU types as
  text or as a list (found by a font under 0.9× the body size in the lower page).
- Figures, algorithms and tables with merged cells are cropped from the page at 144 dpi, crop box only, with a
  `**Fig. N.**` caption. A run of sub-figures with one main caption becomes one crop. Plain tables become pipe
  tables, and code becomes `⟦CODE⟧` tokens.
- A paragraph split across a column, page or figure is joined back.
- Headers, footers, page numbers, printed tables of contents, publisher boilerplate (the ACM permission notice,
  the © line) and the MIT OpenCourseWare terms page are dropped. Bold that the PDF does not set bold is dropped,
  since MinerU marks italic as bold.
- The byline, meaning the short blocks between the title and the first heading or prose paragraph, goes to
  `sections.json` for the cover.
- A printed URL becomes a link with a short label: `DOI: [10.1145/x](url)`, else the host name.
- MinerU's LaTeX is repaired where it would not typeset:
  - text-mode commands become their math forms (`\textgreater`, `\textellipsis`);
  - math commands and scripts left inside `\text{…}` are lifted out of it;
  - stray `$`, `\ref` and unclosed groups are fixed;
  - a formula the model looped on ("{ } { } …") is cut back to one copy.

  Words, ordinals, percentages, § references, quoted characters and phrases it set as math go back to text.
- Printed contents pages (most lines dot leaders or bare page numbers) and blank pages are skipped.
- Sections: a numbered paper over 2,500 words splits at its "N. TITLE" headings, and any section over 4,000
  words splits further at its headings, each ⟦IMG⟧/⟦CODE⟧ token weighing 25 words. References become a verbatim
  back section that is not translated.

`translate_paper.mjs` uses the translate-epub skill's `runCodex`, `checkOutput` and `buildPrompt` with paper rules, and
sends each section together with the PDF text layer of its blocks. The translator repairs MinerU's formula
misreads against that text layer, such as the word "big" read as `\mathbf{\ddot{big}}`. On top of the skill's
checks, a section is rejected and retried when a display equation is missing, under 90% of the inline formulas
came back, a formula KaTeX cannot parse appears, or a link target is lost. Formulas are not compared verbatim,
because the repairs are wanted. A formula that is only a word, a measurement or a name with a script (C++, 475 µs,
TRANSFER₁, L⁴) may come back as text. Before checking, two mechanical repairs run on each answer:

- **Placeholders.** An ⟦IMG⟧/⟦CODE⟧ token the translator dropped goes back to its source position. The tokens
  still present and the headings divide the text into stretches; a stretch is repaired only when its paragraphs
  line up one to one with the source.
- **Exponents.** A `<sup>N</sup>` the translator made of a source superscript (L⁴) goes back to the character,
  unless N is a footnote mark.

Before the sections it makes two codex calls:

- **Glossary.** When `<work>/glossary.md` is missing, it writes one with the title, every heading and 20–40
  terms. The call is seeded with the course glossary `<INTEL_STATE_DIR>/translate-glossaries/<key>.md`, the key
  being the course number that opens the source PDF's folder name. New terms are appended to that file in one
  write; the file is never rewritten.
- **Cover authors.** When there is a byline, it writes `cover_authors`: names in English, affiliations in Chinese.

`render_paper.py` runs the translate-epub skill's `render.py` with paper patches:

- **Cover.** A paper's cover shows the English title, then one author per line with the shared affiliation on the
  last line, or "name, affiliation" per line when the authors are at different places. Notes, which have no
  byline, get no cover.
- **目录.** A one-section document gets no 目录.
- **Figures.** Crops are shown at their source size, scaled from the source's text column to ours.
- **Footnotes.** They are set small under a rule.
- **References.** No drop cap, and a hanging indent.
- **Links and lists.** Links are in #005f87, and list items are set flush left.

## A batch

```bash
"$SKILL_DIR/scripts/batch.py" <list.txt> --out-root ~/Documents/MIT-zh
```

The list has one PDF path per line. Each output is `<out-root>/<source folder>/<stem>.pdf`, the work dir is
`<state>/translate-paper/<key>-<stem>/`, and progress goes to `status.tsv` there plus a `batch.log` per file.
Files run smallest first. MinerU converts one file at a time: two runs side by side hung at start-up. A conversion
is killed with its whole process group after 60 s + 15 s per page and tried once more. Two files translate and
render at once. A file whose output exists is skipped, and a failed render's PDF is moved to
`<work>/render-failed.pdf`, so rerunning the batch redoes exactly the failed files.

## Fixing a failed file

Fix the cause in the scripts, never in one output, then rerun the batch: it reconverts the file, and the
translator redoes exactly the sections whose source changed (each translation keeps the SHA-1 of the source it
was made from in `translated/<id>.src-sha1`). Each kind of failure met so far has a test in `tests/`:

- MinerU LaTeX the renderer rejects;
- words, numbers or names MinerU set as math;
- footnote marks confused with exponents;
- a translator dropping or nesting placeholders, links or formulas;
- headless Chrome exiting with status 2 while other renders run; a second try clears it.

A KaTeX error at render time names the section and page; `scripts/math_check.py <work>` lists each formula the
renderer would print red, after render.py's own Markdown and math repair. Fix the LaTeX in `translated/` only when
the translator introduced it; when MinerU's source LaTeX is broken, fix the converter.

## Tests

```bash
"$SKILL_DIR/tests/test_pdf2work.py"
node --test "$SKILL_DIR"/tests/*.test.mjs
```
