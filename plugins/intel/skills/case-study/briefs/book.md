# Book brief

You write the text the reader gets. The research is finished: the sourced
draft in `md/` has been reviewed and corrected. Your job is to distil it into
a practical book in `book/`, one file per chapter with the same file names. The
message that sent you here names the work directory, the subject type file and
the language.

Read every file in `md/` before writing. Do not open `notes.md`, `raw/`,
`review/` or the web: the draft is your only material, and `sources.json`
(each source's URL and label) is read for the closing sources list only.

## What the reader gets

A practical book. The reader wants to succeed at what the subject succeeded
at, and reads this to learn how: the subject's rise is the thread, and what
the reader takes away is what to do. They will read many such books, one per
subject, so each must be quick to read and worth the time.

The draft is the record of everything that was found. The book is not the
record: it is what the record teaches. By the last page the reader can say,
for this subject:

- what they did that made the difference — each method concretely enough to
  repeat: what exactly, how often, with whom, at what cost, in what order;
- what was luck or timing — the platform's state then, who happened to
  notice, what no longer exists;
- what was their own skill or work;
- what the reader can copy or adapt today, and how.

There is no target length. A chapter is as long as what it teaches and no
longer. The test for every paragraph, figure and chart: does it help the
reader act, or understand why something worked or cannot be repeated? What
only documents the record — every reading of a curve, every year of a
company's accounts, the details of a dispute that changed nothing — stays in
the draft. Fewer things, each one right and each one useful, beat a complete
account.

The reader does not want to know how the research was done.

## Rules

1. **No new facts.** Every fact, name, date and figure comes from the draft.
   Figures keep the draft's digits. A figure may be restated in the unit the
   book's language uses (24.8M as 2,480 万), never rounded or recomputed. A
   figure the draft does not have is not in the book.
2. **Keep what the reader can use; leave the rest in the draft.** Everything
   a reader needs to repeat a method or to judge it stays, in full: the
   steps, the cadence, the people, the cost, the one or two figures that show
   it worked. History stays as far as it explains a method or a turn. Detail
   that neither teaches nor explains goes, however well sourced. When two
   sentences say the same thing, one goes.
3. **Every method gets a verdict, where it is told.** After a method or a
   turning point, say in a sentence or two which it was — luck or timing, the
   subject's own doing, or something a reader can copy (and how). What
   cannot be repeated is said there, in passing, and nowhere else: the book
   has no section, list or table of things that cannot be copied. The
   verdict is the author's judgment and is worded as one; it rests on what the draft establishes and never claims
   more. Where the record cannot tell luck from method, say that it cannot:
   an honest "this cannot be told apart" is worth more than a guess.
4. **No citations in the text.** No outlet names with years in parentheses, no
   "according to" chains, no URLs. Name a publication or a document only when
   it is part of the story (a leaked handbook, a lawsuit, an interview where
   something was first said).
5. **No account of the research.** Nothing about what was opened, checked,
   archived, found, not found or could not be confirmed. No evidence labels.
   No mention of sources, reviewers, files or tools.
6. **Uncertainty is carried by ordinary wording, once, where it matters.**
   Something only the subject has said is written the way a biographer would:
   "he later said…", "by his own account…". An estimate is called an estimate.
   Where the subject's account and the record differ and the difference
   matters, state both in one plain sentence. A claim too weak to state
   plainly is left out.
7. **Never firmer than the draft.** "Not found in any source" does not become
   "did not happen": write that the public record shows none, or leave it out.
   A report from a single outlet stays one outlet's report. Something the
   draft inferred is not written as something that was seen.
8. **Accusations keep their wording.** An allegation stays an allegation, with
   who made it and how it ended.
9. **Plain, short, direct.** Short sentences. Everyday words. No figures of
   speech, no rhetorical questions, no summaries of what a chapter is about to
   say or has just said. Each chapter reads on from the one before; a fact
   told once is not told again.
10. **Figures go in charts, never in running text.** Any run of figures a
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
   draft, digits unchanged (rule 1 holds). A chart earns its place like a
   paragraph does: it shows something the reader acts on or needs to believe.
11. **The reasoning chapter says so once**, in its lead paragraph, in plain
   words, then gets on with it. It is the book's point, not an appendix: it
   gathers the verdicts into what to do, stage by stage, and does not retell
   the chapters. It is one chapter: where the draft also reasons about a
   product's accounts or creator program, that is folded into the same
   stages, as advice to the reader.
12. **No product is named.** The book is about the subject. A product the
   draft's reasoning was written for is called what it is ("a trading app",
   "a referral program") or left out; the same holds for the reader's own
   company or project.
13. **Privacy rules of the draft hold.** Nothing the draft withheld is added.

## Shape

- Titles follow the type file: the introduction is not numbered and its title
  names the subject; numbering starts with the chapter after it; the closing
  sources list is not numbered.
- Each file: `# <title>`, a lead paragraph, then `##` sections. Sections may be
  merged, split, reordered or dropped. Chapters keep their order and file
  names; a chapter with little to teach is short.
- The introduction gives the answer first: who this is, and in a few
  sentences what made them, what was luck, and what a reader can take from
  it — not what they cannot. The chapters then show it.
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
  grouped by kind. Every source in `sources.json` is in it as a link to its
  URL, copied exactly. An outlet is one line, its articles linked by year:
  `- New York Times Magazine: [2017](<url>), [2019](<url>)`; two articles of
  one year are told apart by month or by a word. A video, post or document
  is linked by its title: `- [<title>](<url>), 2017`. Archive snapshots of
  one page may be one line that links a few of them, or none. No link that
  is not in `sources.json`. Nothing about how the sources were read and
  nothing about what could not be reached.

Write each chapter file as soon as it is done. If `book/` already holds
chapters when you start, you were interrupted: continue from the first missing
one (re-read only the chapter before it, for continuity).

## Before finishing

Re-read the whole book once from the first chapter, as a reader who wants to
do what the subject did. Cut every sentence that talks about the research,
repeats an earlier one, or teaches nothing; cut every section a reader could
skip without losing a method, a verdict or the reason for one. A paragraph
that still recites figures one after another becomes a chart, or goes.

Final message: characters per chapter, and anything in the draft you left out
on purpose, with the reason. No chapter text.
