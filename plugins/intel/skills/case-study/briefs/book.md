# Book brief

You write the text the reader gets. The research is finished: the sourced
draft in `md/` has been reviewed and corrected. Your job is to turn it into a
short book in `book/`, one file per chapter with the same file names. The
message that sent you here names the work directory, the subject type file and
the language.

Read every file in `md/` before writing. Do not open `notes.md`, `raw/`,
`review/` or the web: the draft is your only material.

## What the reader gets

A short book about one subject, by one author, read from the first page to the
last. The reader wants what happened and how it was done. They do not want to
know how the research was done.

## Rules

1. **No new facts.** Every fact, name, date and figure comes from the draft.
   Figures keep the draft's digits. A figure may be restated in the unit the
   book's language uses (24.8M as 2,480 万), never rounded or recomputed. A
   figure the draft does not have is not in the book.
2. **Nothing needed is lost.** Every method, step, figure, date and name that a
   reader needs to understand or repeat what was done stays. When two
   sentences say the same thing, one goes.
3. **No citations in the text.** No outlet names with years in parentheses, no
   "according to" chains, no URLs. Name a publication or a document only when
   it is part of the story (a leaked handbook, a lawsuit, an interview where
   something was first said).
4. **No account of the research.** Nothing about what was opened, checked,
   archived, found, not found or could not be confirmed. No evidence labels.
   No mention of sources, reviewers, files or tools.
5. **Uncertainty is carried by ordinary wording, once, where it matters.**
   Something only the subject has said is written the way a biographer would:
   "he later said…", "by his own account…". An estimate is called an estimate.
   Where the subject's account and the record differ and the difference
   matters, state both in one plain sentence. A claim too weak to state
   plainly is left out.
6. **Never firmer than the draft.** "Not found in any source" does not become
   "did not happen": write that the public record shows none, or leave it out.
   A report from a single outlet stays one outlet's report. Something the
   draft inferred is not written as something that was seen.
7. **Accusations keep their wording.** An allegation stays an allegation, with
   who made it and how it ended.
8. **Plain, short, direct.** Short sentences. Everyday words. No figures of
   speech, no rhetorical questions, no summaries of what a chapter is about to
   say or has just said. Each chapter reads on from the one before; a fact
   told once is not told again.
9. **Figures go in charts, never in running text.** Any run of figures a
   reader would otherwise have to hold in their head is a chart block: three
   or more dated values of one kind (a curve, uploads per month, income by
   year) is a line chart; one measure across several things (this video
   against the others, the subject against a peer) is a bar chart; figures
   with several attributes, or exact values that must all be readable, are a
   table. The paragraph beside it says in words what the chart shows — the
   turn, the gap, the pace — and does not repeat its values. A single figure,
   or two, stays in the sentence. The point is fewer figures in front of the
   reader, not the same figures in boxes: a series is drawn once, as a line,
   and is not also printed as a table; a table has at most about eight rows
   and holds only what the reader needs exactly (the milestones, the yearly
   totals), not every reading the draft has; the same data is not shown
   twice in the book. Every value in a chart is a figure from the
   draft, digits unchanged (rule 1 holds).
10. **Reasoning chapters say so once**, in their lead paragraph, in plain
   words, then get on with it.
11. **Privacy rules of the draft hold.** Nothing the draft withheld is added.

## Shape

- Titles follow the type file: the introduction is not numbered and its title
  names the subject; numbering starts with the chapter after it; the closing
  sources list is not numbered.
- Each file: `# <title>`, a lead paragraph, then `##` sections. Sections may be
  merged, split or reordered when that reads better. Chapters keep their order
  and file names.
- A chart block stands on its own lines, with a blank line before and after:

  ````
  ```chart
  type: line
  title: Subscribers
  columns: Date | PewDiePie | A peer channel
  2013-02-08 | 4,631,292 | 110,010
  2014-01-23 | 21,105,672 | 1,413,462
  2016-12-12 | 50,566,204 | 15,608,384
  ```
  ````

  `type` is `line`, `bar` or `table`; `title` says what is measured and in
  what unit. Each row is a label and its values, separated by `|`. A line
  chart's labels are dates (`2012`, `2012-07`, `2012-07-11`); `scale: log`
  suits values that grow by orders of magnitude. `columns` names the series
  when there are two or three, and is the header of a table. A value is
  digits with an optional unit (`K`, `M`, `B`, `万`, `亿`) and is printed as
  written; a table cell may hold any text. A line chart prints the values
  that fit beside their points: when every exact value matters, use a table.
- The last file is the sources list: one short sentence, then the sources
  grouped by kind, each as outlet and year. Nothing about how they were read
  and nothing about what could not be reached.

Write each chapter file as soon as it is done. If `book/` already holds
chapters when you start, you were interrupted: continue from the first missing
one (re-read only the chapter before it, for continuity).

## Before finishing

Re-read the whole book once from the first chapter, as its reader. Cut every
sentence that talks about the research, repeats an earlier one, or carries no
information. A paragraph that still recites figures one after another
becomes a chart.

Final message: characters per chapter, and anything in the draft you left out
on purpose, with the reason. No chapter text.
