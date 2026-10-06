---
name: translate
description: >
  Translate a whole English EPUB book into a Chinese PDF that keeps the original's format: the same cover
  image, page size, chapter structure, running heads, folios, a clickable 目录 page and PDF bookmarks (Cover,
  目录, one per chapter). Reads the EPUB's OPF spine (order) and nav/ncx (titles), keeping each section's bold,
  emphasis, sub/superscripts and images. Chapters are translated in parallel by gpt-6-luna through `codex exec`,
  one call per chapter, and typeset with headless Chrome in Baskerville + Songti SC. Use for
  "/translate <book.epub>", "把这本书翻译成中文", "translate this book", or to re-render an already translated
  book after editing its Markdown. Needs the book as an EPUB — get it with the download-book skill first. NOT
  for a single page or article (just translate it inline), and not for digesting or summarizing a source (use
  digest).
---

# Translate — an EPUB book into a Chinese PDF in the same format

## Runtime and paths

Works in Claude Code and Codex. Resolve `SKILL_DIR` from the absolute directory of
this loaded `SKILL.md`, not the working directory or a host-specific environment variable:

```sh
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Repeat this assignment and any `S`, `T` or `U` assignments used below in every shell call;
shell variables may not persist between calls. If the loaded path is unavailable, stop
and report it. Keep the full intel plugin installed: sibling skills share scripts.
Run archive commands from the repository that owns the archive; configuration and
account stores are shared between hosts and are not migrated by installing intel.

For finite long-running commands, choose a deadline before launch and retain the process
handle and output. In Claude Code use `run_in_background` and its completion notification;
in Codex use the shell tool's process/session handle and wait for completion. Subagents
must await their own commands before returning. Do not repeatedly poll logs or assume a
background completion wakes either host. On timeout, preserve diagnostics and report the
process state before retrying. Use the current host's image/file tools to inspect artifacts.

## Environment Variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `CODEX_HOME` | Existing Codex login directory; default ~/.codex | No | Shell environment before running the command; no automatic `.env` loading |

The translator uses the existing Codex CLI login; no API key is required.

The book's sections drive everything: each section (from the EPUB's OPF spine and nav/ncx) is one Luna call, and
the finished sections are typeset into one book that copies the source's page size, chapter openers, running
heads, roman/arabic folios and cover image.

extract.mjs takes an EPUB (`.epub`); it does not read PDFs. If you only have the book as a PDF, get its EPUB with
the download-book skill first.

The book is usually the download-book skill's EPUB in `~/Documents/books/`. Work lives in
`~/.local/state/intel/translate/<slug>/` (`<INTEL_STATE_DIR>/translate/<slug>/` when set; resumable, and the
translated Markdown in it is costly to redo, so keep it); the deliverable is `<title-slug>.pdf` in
`~/Documents/translate/` (`<INTEL_OUTPUT_DIR>/translate/` when set) — the book's main title (the part before a
`:`/`：` subtitle) lowercased with every run of non-alphanumerics turned into one dash, e.g. `Addiction by Design:
Machine Gambling in Las Vegas` → `addiction-by-design.pdf` (no language suffix); an already-Chinese title falls
back to the work dir's slug. Scratch files go under `~/.local/share/intel/tmp/translate/` and are removed when
each run ends. Nothing is written next to the book.

## Setup (automatic, idempotent)

```bash
"$SKILL_DIR/scripts/setup.sh"
```

Installs `poppler` (pdftotext) and `uv` when missing (extract/translate are Node scripts, Node >= 18.18, no npm packages;
render.py runs under `uv`); reports whether `codex` is logged in and Chrome
is present. Luna runs on the user's ChatGPT plan through `codex`; when its quota is out, wait or pass
`--model` to a different codex model.

## 1. Extract

```bash
"$SKILL_DIR/scripts/extract.mjs" <book.epub>
```

Extraction walks the EPUB's OPF spine (order) and nav/ncx (titles), keeping each section as a cleaned XHTML
fragment whose `<strong>`, `<em>`, `<sub>`, `<sup>` tags the translator turns into correct LaTeX; part-divider
files identified by their titles fold into the next chapter; numbered `part` filenames carry no structural meaning, the cover comes from the OPF, and the page size is `--page-size WxH` (default
468x680pt, a 6.5x9.4in trade book). Add `--keep-images` to carry figures and image equations through.

```bash
"$SKILL_DIR/scripts/extract.mjs" <book.epub> --keep-images
```

Prints one line per section (`id kind title: words`) and the work dir. Kinds: `contents` and `skip` (Cover,
Index, Notes, References/Bibliography, copyright/title pages: not translated; the cover comes from the OPF and
the 目录 is regenerated), `front` (preface, introduction — roman folios), `chapter` (第N章, arabic folios from
1), `back` (acknowledgments, appendix, letters; a Conclusion/Epilogue/Afterword, the Answers to Exercises and each
Appendix open their own back section). A section over 15,000 words (a textbook chapter, the answers) is split at
its numbered subsection headings ("1.2.1. …", "Section 1.2.1"), one section each, titled by the heading; a
heading with no body of its own leads the next section. Sibling appendix sections within a spine file are split into their own sections. About the Author and Colophon
open separate back sections. `images.json` records each image's EPUB file name as
`src`, so a recurring glyph (a marker, a symbol) can be found by name.
Once the book's terminal back-matter starts (Notes/References/Bibliography/Index) past the last chapter, it and
every spine file after it (continuations with no nav title of their own included) is skipped, so endnotes and
the index never fold into the last chapter. An untitled spine file right after a skipped one (an index's
continuation) is skipped too.

Pass `--keep-images` for books whose figures and equations are stored as images (e.g. a textbook): each
section's text then carries an `⟦IMG:key⟧` placeholder at every image's position (block images on their own
line, inline symbols within the line), the images are copied into `<work>/images/` and mapped in
`<work>/images.json`. The translator is told to keep the placeholders verbatim, and `render.py` puts the images
back — block ones as centered figures, inline ones in the line. Line art (equations, diagrams) is recoloured to
the page foreground on a transparent background so it blends into the dark page like the body text; a colour
figure keeps a white plate (inverting a photo would ruin it). Line art is judged by colour alone, so a book whose
figures are grayscale screenshots or photos (a UI design book) renders with `render.py --no-recolor`, which keeps
every image as is on a white plate. Without the flag, extraction is text-only. Use the
flag only when images matter. Code listings (`<pre>`, block `<tt>`, or `<code>` with line breaks) are always
protected as `⟦CODE:key⟧`; `codeblocks.json` stores their verbatim text. Translation sees only these tokens.
The renderer restores smaller, wrapping monospace `<pre><code>` blocks; fenced or raw code bypasses math parsing.

Check the listing before spending calls: a section with suspiciously few or many words, or a title parsed wrong,
means a spine file was mis-grouped. A warning that chapter numbers skip or repeat means an opener was not
recognised — inspect the sections and, if needed, edit `<work>/sections.json` (drop an entry, or change its
`kind`) before translating. A common case: a book that stores its endnotes per chapter under "Chapter N" nav
titles; those are references, not chapters — remove them from `sections.json` so they are not translated.

## 2. Glossary (short, before translating)

Write `<work>/glossary.md`: the book's key terms and every chapter title with its Chinese rendering, plus the
authors' names. The chapter titles become the 目录 and the bookmarks, so pinning them here is what keeps the
in-text references, the contents page and the bookmarks consistent. Ask the user only when a term is a real
choice (e.g. a coined term with two accepted renderings); otherwise decide and note it in the summary.

## 3. Translate (background, parallel)

```bash
"$SKILL_DIR/scripts/translate.mjs" <work> --glossary <work>/glossary.md \
  > <work>/translate.log 2>&1
```

Run it using the host-specific long-command instructions above. Both hosts call the
same logged-in `codex exec` CLI; do not replace it with host agents. Defaults: `gpt-6-luna`, effort `low`, Fast service tier (`priority`), 20
sections in flight, one no-tool `codex exec` per section in a private `CODEX_HOME`. Measured: a 250-page trade
book (27 sections of 1.5–3.5k words) is back in about a minute, a 380-page book (54 sections, up to 12k
words) in about five; 7–15k input and 2–4k output tokens per section. Each answer is checked (starts with `# title`, at least 0.6 Chinese
characters per English word, identical placeholder counts and numeric `<sup>N</sup>` marker sets) and retried once; a section that still fails is kept as `<id>.rejected.md` and
reported as `FAILED`, listing missing or extra protected tokens. Rerunning skips sections whose `.md` exists (`--force` redoes them, `--only 04,05`
narrows).

While it runs, preview any finished section as its own PDF (own page numbers, no cover):

```bash
"$SKILL_DIR/scripts/render.py" <work> --only 04
```

Read one finished chapter against the source before the rest lands: wrong register, a dropped paragraph or a
glossary miss is cheaper to fix by editing the glossary and rerunning `--force --only` now than after the book
is assembled.

When every section is back, lint the Markdown before rendering:

```bash
"$SKILL_DIR/scripts/lint_md.py" <work> [id ...]
```

It lists, per section and line: a block image token (a figure, a display equation) the translation lost or one it
invented; missing or extra code placeholders; an inline tag left open (one unclosed `<code>` turns every later section into code and hides its
math from KaTeX); Markdown, a Unicode sub/superscript or an image token inside `\(..\)`; unbalanced math
delimiters; and math variables left as text (`*n*`, `<em>k</em>`, `<sub>`). Fix each in the Markdown. A section
with many "unconverted math" hits had its LaTeX conversion skipped by the translator: retranslate it with
`translate.mjs <work> --force --only <id> --effort medium`. An inline symbol image turned into LaTeX is correct
and not reported.

## 4. Render the book

```bash
"$SKILL_DIR/scripts/render.py" <work> --title "<中文书名>"
```

Writes `<title-slug>.pdf` in `~/Documents/translate/`: the cover (the EPUB's cover image rendered full-bleed), a
目录 with folios, then every translated section. Page size is the one set at extract (`--page-size`). Colors
default to a dark reading page: the background follows the iTerm2
default profile's dark-mode background when iTerm2 is installed (else near-black), the text is `#6e7f7a`, a
cool gray chosen for long reading on black (neutral or warm grays glare on a black page even when dimmed;
a saturated terminal foreground is too dark for body text). `--bg/--fg` take `#rrggbb` or `iterm` (either
terminal color). Type is Baskerville for Latin and
Songti SC for Chinese. Inline `\(..\)` / `\[..\]` LaTeX is typeset by KaTeX (loaded from the CDN, so rendering
needs network). Chinese **bold** — the source's term emphasis and the translator's highlights — is set in a
gothic (黑体) face a step brighter than the body (`--bold-factor`, default 1.25), because Songti's bold is nearly
invisible; chapter openers and headings match. 黑体 glyphs sit about 0.05em higher than Songti's, so bold Chinese
runs are nudged down onto Songti's line. A quotation or epigraph (Markdown `> `) sets its Chinese upright (Songti has
no italic; Chrome would slant it mechanically) and its Latin in Baskerville italic. Picture-type (image) equations from an EPUB share one scale: a block
(display) one is centered at its pixel width times `--eq-scale` (default 0.6), an inline one sits in the line at its
pixel width times `--inline-scale` (default 0.33, about body-text size). A Markdown rule (`---`, a scene break in the source) is set as blank space between the
paragraphs, never as a drawn line. CJK closing punctuation right after inline math, inline code or an inline image is bound to
it, so a line never opens with ，or 。. A block figure and the caption paragraph right after it (`**图 N.**`, `**表 N**`, `**Fig.**`,
`**Table**`) are never split by a page break. The paragraph right before a block figure is kept with it, so a lead-in ("如下：")
is never left alone at a page bottom; a long one still splits, with only its last lines moving. Ordered lists retain their starting numbers across paragraph, quotation and bullet-list interruptions.
Inline code is set in
KaTeX's typewriter face, the same as `\mathtt`. A paragraph with hard line breaks (aligned rows) has no first-line
indent, and a continued line keeps its leading ideographic spaces (a staircase). Chapter openers carry the
第N章 label, the title and a drop cap on the first paragraph after any epigraph (left off when that paragraph opens
with a bold number label or a digit, as in an answers section, or has no text, only an image; a paragraph opening with a
bold phrase gets a bold cap). Figure captions, including unnumbered `**图　…**` labels and source
caption-classed paragraphs, are skipped so the first prose paragraph carries the cap. Every paragraph is indented 2em except a drop-cap one, so a list section's first item
lines up with the rest; body pages carry the chapter title as running head and a folio (roman in
front matter, arabic from chapter 1). Every 目录 row is a link to its section, and the bookmarks are flat:
Cover, 目录, one per section. Sections without a
translation yet are skipped with a warning, so a partial book renders at any time.

Then look, do not assume: render the cover, the 目录, one chapter opener and one body page to PNG
(`pdftoppm -r 45`) and check that the header is masked on openers, folios restart at chapter 1, and 目录 page
numbers match the bookmarks (pikepdf: the 目录 page's `/Annots` links point at the same pages). Then run the
format check on the PDF:

```bash
"$SKILL_DIR/scripts/format_check.py" <book.pdf> > <work>/format-check.txt
```

It reads only the PDF and lists, per page: sections whose opening prose has no drop cap (or a cap on a number
label, or a plain cap on a bold opener), slanted Chinese, bold Chinese sitting off Songti's line, loosely spaced
lines (justification stretching the gaps round a long unbreakable Latin run), exercise numbers pushed right of the
indent by an inline marker image, and list sections whose first item sits at the margin. Loose lines are reported
for information; the others are layout defects to fix before handing the book over. Open the PDF for the user.

## 5. Fix any equations the guard flags

An equation KaTeX cannot parse renders as its raw LaTeX source in red. `render.py` guards against shipping that:
after writing the PDF it scans for the errors and, if any, **exits non-zero** listing every one as
`section NN (page P)`. A render that exits non-zero is not done — fix the listed equations and re-render until it
exits 0. Never hand the user a book the guard rejected.

`md_to_html`/`repair_math` already auto-fix the translator's common LaTeX mistakes (promote inline `\tag` to a
display equation, escape a literal `$`, strip leaked `> ` blockquote markers, `\（`/`\）`→`\(`/`\)`, `\mbox`→`\text`),
so what reaches the guard is the structural residual — usually 0–2 per book. Fix each by editing the section's
Markdown in `<work>/translated/` (find the failing `\(..\)`/`\[..\]` near the reported spot) and re-running step 4:

- **Unbalanced braces / a `\begin{aligned}` row with CJK punctuation outside `\text`** → balance the braces and
  wrap the Chinese in `\text{…}`.
- **An `⟦IMG:key⟧` token inside `\(..\)`/`\[..\]`** is a *symbol* the EPUB stored as an image (KaTeX cannot embed
  an image, so it fails). Open `<work>/images/<file>` (from `images.json`) to see the glyph — e.g. an accented
  q̂ / v̂ — and write it as LaTeX (`\hat{q}`, `\hat{v}`); keep `⟦IMG⟧` tokens only for real figures, outside math.
- **Markdown (`**bold**`) or a stray `\(` inside math** → move the markup outside the math, restructure the line.

Editing the md directly (you can view the symbol image and re-render to verify) is faster and higher-fidelity than
re-translating; retranslate the section only if the prose itself is wrong.

## Editing after the fact

The translation is plain Markdown in `<work>/translated/`: fix a sentence there and rerun step 4 (seconds). Retranslate
one section with `translate.mjs <work> --force --only <id>`. A different look (light theme, other margins) is
`--bg/--fg` or an edit to `css()` in `render.py`.

## Notes

- Figures and tables do not survive unless you pass `--keep-images`: a rate table comes out as prose. Say so in
  the summary when the source has them, and point the reader at the source for the numbers — or re-extract with
  `--keep-images` to carry figures and image equations through as placeholders and render them back.
- Chrome cannot reset the page counter mid-document, so front matter and body are typeset as two documents and
  joined with pikepdf; the running head on opener pages is masked by a background rectangle after the fact.
- The PDF text layer keeps the tiny invisible `⟦S04⟧` markers the page map uses; they are 1pt and transparent.
