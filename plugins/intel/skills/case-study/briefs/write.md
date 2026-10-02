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
`grep -h "\[cNN\]" <work>/notes/*.md`. Read all of them. Anything not in the
notes does not go in.

## The introduction and the reasoning chapter

These are written after the others. Your material is the finished chapters in
`md/`. The introduction says who this is, what they built and on what, from
those chapters. The reasoning chapter opens by saying it is reasoning, not a
finding, and rests only on what the chapters establish. When your message
gives a product, the same chapter also says what that product's own accounts
and creator program can take from the subject. The product is never named:
call it by what it is ("a trading app", "its referral program"). Its terms
are givens of the task, stated as such.

## Rules

- First line `# <chapter title>`, then a lead paragraph, then five or six `##`
  sections.
- Every sentence that carries a claim shows its kind and names its source by
  the label in the bullet, with the year. Only labels that are values in
  `sources.json` may be named. The label sits inside its own sentence, in
  brackets before the sentence's final full stop, never after it: a script
  matches each sentence's figures against the sources that sentence names.
- A method is written out in full: what exactly, how often, with whom, at what
  cost, what changed. Fewer solid claims beat more weak ones.
- Where the subject's account and the record differ, print both. Where two
  sources differ, print both. Curve and upload figures come from the numbers
  notes first; press figures second, labelled.
- A tie between a source and the subject is stated in the sentence that uses
  it.
- Write only `md/<your file>`, appending section by section as you go (see
  "Writing as you go" in the evidence rules).

Final message: characters written and the places where the notes were too thin.
