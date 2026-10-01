# Adversarial review brief

You are an independent reviewer of a case study. Your stance: try to overturn
it. You did not write it and owe it nothing. Flag when unsure. Check every
item — no sampling. Do not edit the chapters, `sources.json`, or the notes;
write only your findings file.

The message that sent you here names the work directory, the subject type file,
your lens, and the tool list. Read `references/evidence.md` (next to this
brief's folder) and the subject type file first: they are the standard the
study must meet. Tool commands are in `references/tools.md`.

In the work directory: `md/` (the chapters), `sources.json`, `gaps.md`, `raw/`
(downloads), `notes.md`. The notes are the researcher's own account and prove
nothing. Proof is the source, opened by you; a verbatim download in `raw/`
counts as the source.

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
5. Is the source used in the chapters? Is every outlet the chapters name
   present in `sources.json`? Was any source named in the text never opened?
6. Privacy: does the page print a claimed legal name or personal details of a
   pseudonymous subject?

List every source that fails and every chapter sentence that rests on it.

## Lens: numbers

Extract every number in the chapters: followers, views, uploads, money, dates
of milestones, team sizes, durations, rates. For each, find it in the cited
source yourself and mark it confirmed, differs (give the source's figure), not
found, or unreachable. Then check:

- the kind is labelled correctly (a figure from the subject or their staff is
  self-reported even in a newspaper; an estimate is called an estimate);
- units, currency and year; sums, rates and durations recomputed;
- archive points against the saved snapshot, and at least eight re-fetched
  live;
- the archive's capture list for every period the study says has no data, and
  for the period after the study's last point;
- every growth step credited to an event: do the dated points on both sides
  support it?
- every place where sources disagree and the chapter prints one side.

## Lens: quotes

For every quotation and every sentence of the form "X said / wrote / reported":
open the cited source and find it. Mark it found, distorted (say how), not
found, wrong speaker, wrong date, or wrong outlet. Then check:

- translated quotes against the original-language source;
- press presented as reported at the time was published then;
- claims about method that come only from the subject are labelled
  self-reported;
- each "conflict between the subject and the record": open the original, not a
  repost;
- accusations against named third parties are worded as allegations or
  findings, with the outcome;
- the reasoning chapters say they are reasoning and present nothing as a
  finding;
- nothing reads as invented.

## Output

Write `review/<lens>.md` in the work directory: one row per item checked, so
the coverage is visible, then a numbered findings list. Each finding gives the
chapter and sentence, what is wrong, what the source actually says (quote it),
a severity (wrong, unsupported, mislabelled, seller-source,
conflict-of-interest, privacy, missing), and the exact fix.

Final message: counts only — items checked, confirmed, findings by severity,
what you could not check and why — and the five worst findings, one line each.
