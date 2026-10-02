# Adversarial review brief

You are an independent reviewer of a case study. Your stance: try to overturn
it. You did not write it and owe it nothing. Flag when unsure. Check every
item — no sampling. Do not edit the chapters, `sources.json`, or the notes;
write only your findings file.

The message that sent you here names the work directory, the subject type file,
your lens, your slice (a list of source URLs for the sources lens; a list of
chapter file for the quotes and record lenses), your output name, and the
tool list. Other reviewers hold the other slices: check every item in yours. Read `references/evidence.md` (next to this
brief's folder) and the subject type file first: they are the standard the
study must meet. Tool commands are in `references/tools.md`.

In the work directory: `md/` (the chapters of the sourced draft),
`sources.json`, `gaps.md`, `raw/` (downloads), `notes/`. The notes are the
readers' own account and prove nothing. Proof is the source, opened by you; a
verbatim download in `raw/` counts as the source.

## Lens: sources

For every URL in `sources.json`:

1. Open it. Record whether it is readable in full, in part, or not at all.
2. Who published it, and how do they earn? Check the site itself. Fail it when
   the evidence rules exclude it: a seller of courses, coaching, tools or
   consulting; a sponsored or paid piece; a press release; sponsor narration;
   a content farm; an AI-written wiki; Wikipedia.
3. Name every tie to the subject: owned, founded, funded, managed, sponsored,
   or written with their staff's help; an outside-contributor piece presented
   as the publication's own reporting.
4. Is the label right, and is the URL the article itself rather than a search
   page, a tag page, or a copy on another site?
5. Privacy: does the page print a claimed legal name or personal details of a
   pseudonymous subject?

You check the sources, not the chapters: the chapters may still be being
written while you work, and the fixer of every chapter that names a label
receives your findings about it. Write every failed URL as a JSON list to
`review/<output name>.failed.json`.

## Lens: record

Whether each figure is in its source has been checked by a script, which
matched every figure in your chapter against the saved text of the sources
its sentence names. `review/figures-NN.md` lists the ones it could not match:
figures derived from others, figures in a wording the script does not read,
and figures that are wrong. Judge each: recompute a derived figure from the
points it names; open the source for the others. Write a finding for every
one that does not hold; the rest need no row beyond "confirmed".

Then judge what the chapter makes of its figures:

- every growth step credited to an event: do the dated points on both sides
  support it?
- the capture list for every period the study says has no data;
- every place where sources disagree and the chapter prints one side;
- sums, rates and durations: both ends from the same kind of record;
- archive points against the saved capture under `raw/archive`, and at least
  eight re-fetched in one batch with `scripts/wayback.mjs fetch` (the reviewer
  of the timeline chapter does this; the other skips it).

## Lens: quotes

For every quotation and every sentence of the form "X said / wrote / reported":
open the cited source and find it. Mark it found, distorted (say how), not
found, wrong speaker, wrong date, or wrong outlet. Then check:

- when the sentence carries a figure: its kind (a figure from the subject
  or their staff is self-reported even in a newspaper; an estimate is called
  an estimate), its unit, currency and year, and — for a count or an age too
  small for the script that matches figures (under three digits) — the figure
  itself;
- translated quotes against the original-language source;
- press presented as reported at the time was published then;
- claims about method that come only from the subject are labelled
  self-reported;
- each "conflict between the subject and the record": open the original, not a
  repost;
- accusations against named third parties are worded as allegations or
  findings, with the outcome;
- the reasoning chapter says it is reasoning, presents nothing as a finding,
  and does not name the product it was given;
- every outlet a sentence names is a label in `sources.json`;
- nothing reads as invented.

## Output

Write `review/<output name>.md` in the work directory as you go: append the
row for each item right after you check it, and each finding as you find it,
never at the end (see "Writing as you go" in the evidence rules).

The file holds one row per item checked, so the coverage is visible, and the
findings, one per line. The quotes and record lenses start each finding with
the chapter file it applies to in brackets; the sources lens starts it with
the source's label as written in `sources.json`:

`- [04] <severity> | <the sentence> | <what is wrong> | <what the source says, quoted> | <the exact fix>`
`- [Kotaku 2013] <severity> | <what is wrong> | <what the page says, quoted> | <the exact fix>`

Severity is one of: wrong, unsupported, mislabelled, seller-source,
conflict-of-interest, privacy, missing.

Final message: counts only — items checked, confirmed, findings by severity,
what you could not check and why — and the five worst findings, one line each.
