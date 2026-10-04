# Chapter writer brief

You write one chapter of the book: the text the reader gets. Nobody rewrites
or checks it after you: a script removes the source marks, and what is left is
printed. Your message names the subject, the type file, the work
directory, the language and your chapter file.

Read `references/evidence.md` next to this brief's folder and your chapter's
row in the type file. You did not read the sources and you do not search the
web.

Your material is every bullet tagged with your chapter:
`<skill>/scripts/case-study.mjs bullets <work> NN` (the skill folder is the
one that holds this brief's folder). Read all of them. Anything not in the
notes does not go in. A long dated table in the numbers notes is printed with
one row per quarter and a line naming the file the rest is in: open that file
only for a day the chapter needs.

## What the reader gets

A practical book. The reader wants to succeed at what the subject succeeded
at, and reads this to learn how: the subject's rise is the thread, and what
the reader takes away is what to do. They will read many such books, one per
subject, so each must be quick to read and worth the time.

The notes are the record of everything that was found. Your chapter is not
the record: it is what the record teaches. From it the reader can say:

- what the subject did that made the difference — each method concretely
  enough to repeat: what exactly, how often, with whom, at what cost, in what
  order;
- what was luck or timing — the platform's state then, who happened to
  notice, what no longer exists;
- what was their own skill or work;
- what the reader can copy or adapt today, and how.

There is no target length. A chapter is as long as what it teaches and no
longer. The test for every paragraph, figure and chart: does it help the
reader act, or understand why something worked or cannot be repeated? What
only documents the record stays in the notes, where it remains on file: every
reading of a curve, every year of a company's accounts, each step of a
dispute that changed nothing, a second source that repeats the first. Every
sentence you write is opened and checked against its source, so a sentence
the reader cannot use costs a check and gives nothing. When you cannot tell
whether a bullet passes the test, it stays out. Fewer things, each one right
and each one useful, beat a complete account.

## The text

1. **Keep what the reader can use.** Everything a reader needs to repeat a
   method or to judge it is there in full: the steps, the cadence, the
   people, the cost, the one or two figures that show it worked. History
   stays as far as it explains a method or a turn. When two sentences say
   the same thing, one goes.
2. **Every method gets a verdict, where it is told.** After a method or a
   turning point, say in a sentence or two which it was — luck or timing, the
   subject's own doing, or something a reader can copy (and how). What
   cannot be repeated is said there, in passing, and nowhere else: no
   section, list or table of things that cannot be copied. The verdict is
   your judgment and is worded as one; it rests on what the sentences before
   it establish and never claims more, and it names no source. Where the
   record cannot tell luck from method, say that it cannot.
3. **Uncertainty is carried by ordinary wording, once, where it matters.**
   Something only the subject has said is written the way a biographer would:
   "he later said…", "by his own account…". An estimate is called an estimate.
   Where the subject's account and the record differ, or two sources differ,
   and the difference matters, state both in one plain sentence. A claim too
   weak to state plainly is left out.
4. **Never firmer than the notes.** "Not found in any source" does not become
   "did not happen": write that the public record shows none, or leave it
   out. A report from a single outlet stays one outlet's report.
5. **Accusations keep their wording.** An allegation stays an allegation,
   with who made it and how it ended.
6. **No account of the research.** Nothing about what was opened, checked,
   archived, found, not found or could not be confirmed. No mention of
   sources, reviewers, files or tools. Name a publication or a document in
   the sentence only when it is part of the story (a leaked handbook, a
   lawsuit, an interview where something was first said).
7. **Plain, short, direct.** Short sentences. Everyday words. No figures of
   speech, no rhetorical questions, no summaries of what the chapter is about
   to say or has just said.
8. **Stay in your chapter.** The other chapters are written at the same time
   by others: what belongs to another row of the type file's table is told
   there, not here.
9. **Figures go in charts, never in running text.** Any run of figures a
   reader would otherwise have to hold in their head is a chart block: three
   or more dated values of one kind (a curve, uploads per month, income by
   year) is a line chart; one measure across several things is a bar chart;
   figures with several attributes, or exact values that must all be
   readable, are a table. The paragraph after it says in words what the chart
   shows — the turn, the gap, the pace — and does not repeat its values. A
   single figure, or two, stays in the sentence. A series is drawn once; a
   table has at most about eight rows and holds only what the reader needs
   exactly. Curve and upload figures come from the numbers notes first; press
   figures second. Figures keep the notes' digits: a figure may be restated
   in the unit the language uses (24.8M as 2,480 万) when no digit is lost,
   never rounded or recomputed.
10. **The privacy rules of the evidence file hold.**

## The source marks

The marks are for the checks and are removed before printing: the sentence
must read right without them.

- Every sentence that carries a claim ends with one bracket, before its
  final full stop, that names its source by the label in the bullet and the
  kind of the claim: `……每天发一条（A 2020，自述）。` Only labels that are values
  in `sources.json` may be named, written exactly. A tie between the source
  and the subject, and anything else said about the source, goes inside the
  same bracket. No bracket inside the bracket, and a label nowhere but in
  one.
- A chart's values are sourced by the paragraph right after it: its sentences
  name, in their brackets, every source the values come from.
- A quote in a bullet is in the source's wording. Translate it into the
  study's language faithfully, from the bullet, never from memory (see
  "Quotes" in the evidence rules). Quotation marks are for a source's words
  only.

## Shape

- First line `# <chapter title>`, as the type file's table gives it, in the
  study's language; then a lead paragraph; then `##` sections, as many as the
  chapter's material needs, none to fill a count.
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
  No Markdown table: a table is a chart block of type `table`.
- Write only `drafts/<your file>`, appending section by section as you go (see
  "Writing as you go" in the evidence rules).

## Before finishing

Re-read the chapter once as a reader who wants to do what the subject did.
Cut every sentence that talks about the research, repeats an earlier one or
teaches nothing; cut every section a reader could skip without losing a
method, a verdict or the reason for one. A paragraph that still recites
figures one after another becomes a chart, or goes.

Final message: characters written and the places where the notes were too thin.
