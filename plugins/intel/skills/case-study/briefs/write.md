# Draft writer brief

You write one chapter of the sourced draft: the layer reviewers audit line by
line. The text the reader gets is written from it later, by someone else.
Your message names the subject, the type file, the work directory, the
language, your chapter file, and the product for the reasoning chapter if any.

Read `references/evidence.md` next to this brief's folder and your chapter's
row in the type file. You did not read the sources and you do not search the
web.

## A chapter built from the notes (every chapter between the introduction and the reasoning chapter)

Your material is every bullet tagged with your chapter:
`<skill>/scripts/case-study.mjs bullets <work> NN` (the skill folder is the
one that holds this brief's folder). The notes of a source the reviewers
failed are not among them. Read all of them. Anything not in the
notes does not go in. A long dated table in the numbers notes is printed with
one row per quarter and a line naming the file the rest is in: open that file
only for a day the chapter needs.

## What goes in

The draft is what the book is written from, and the book is a practical one:
its reader wants to do what the subject did. Every sentence you write is
opened and checked against its source by a reviewer, so a sentence the book
cannot use costs a review and gives the reader nothing. From your bullets,
write the ones that pass this test: does it help a reader act, or understand
why something worked or cannot be repeated? That is the methods, what they
cost and what they brought, the turns and what caused them, what was luck or
timing, and the few figures that show any of it. What only documents the
record stays in the notes, where it remains on file: every reading of a
curve, every year of a company's accounts, each step of a dispute that
changed nothing, a second source that repeats the first. When you cannot
tell whether a bullet passes, it goes in.

A series of figures (the curve, the upload cadence, income by year) is one
table, each row naming its source, with a few sentences on what it shows;
it is not retold row by row in sentences.

## The introduction and the reasoning chapter

These are written after the others. Your material is the finished chapters in
`drafts/`. The introduction says who this is, what they built and on what, from
those chapters. The reasoning chapter opens by saying it is reasoning, not a
finding, and rests only on what the chapters establish. When your message
gives a product, the same chapter also says what that product's own accounts
and creator program can take from the subject. The product is never named:
call it by what it is ("a trading app", "its referral program"). Its terms
are givens of the task, stated as such.

## Rules

- First line `# <chapter title>`, then a lead paragraph, then `##` sections:
  as many as the chapter's material needs, none to fill a count.
- Every sentence that carries a claim shows its kind and names its source by
  the label in the bullet, with the year. Only labels that are values in
  `sources.json` may be named. The label sits inside its own sentence, in
  brackets before the sentence's final full stop, never after it: a script
  matches each sentence's figures against the sources that sentence names.
- A quote in a bullet is in the source's wording. Translate it into the
  study's language and put the bullet's wording right after it in ⟦ ⟧, copied
  from the bullet, never retyped from memory (see "Quotes" in the evidence
  rules). Quotation marks are for a source's words only.
- What a source said, wrote or reported, told without quotation marks, is
  followed the same way by the words the bullet has in quotation marks for
  it, in ⟦ ⟧, copied from the bullet:
  `他说早年每四个视频就有一个没发⟦one out of every four videos we filmed just never got uploaded⟧`.
  A script looks them up and shows the reviewer the passage; a sentence
  without them is searched for by hand. When the bullet quotes no words for
  it, the sentence carries no ⟦ ⟧.
- A method is written out in full: what exactly, how often, with whom, at what
  cost, what changed. Fewer solid claims beat more weak ones.
- Where the subject's account and the record differ, print both. Where two
  sources differ, print both. Curve and upload figures come from the numbers
  notes first; press figures second, labelled.
- A tie between a source and the subject is stated in the sentence that uses
  it.
- Write only `drafts/<your file>`, appending section by section as you go (see
  "Writing as you go" in the evidence rules).

Final message: characters written and the places where the notes were too thin.
