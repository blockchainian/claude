# Fix brief

Reviewers audited the sourced draft. You apply their findings to one chapter.
Your message names the work directory, the type file, your chapter file and
the tool list. The standard is `references/evidence.md` (next to this brief's
folder); the skill folder is the one that holds this brief's folder.

Your findings: `<skill>/scripts/case_study.mjs findings <work> NN` for your
chapter file's number: the lines tagged with it, and the sources-lens lines
about every label your chapter names. The failed sources: every URL in
`<work>/review/*.failed.json`; find their labels in `sources.json`.

1. Every sentence resting on a failed source is removed or re-sourced.
   Re-sourced means you open the original now and read the fact there. If the
   original cannot be opened, the claim goes.
2. An interview hosted by a seller keeps only the subject's own words,
   labelled self-reported, in a sentence that names the host as a seller.
3. A source never opened is opened now, and the text corrected to what it says,
   or it leaves the text.
4. A quote not found loses its quotation marks and the claim, or takes the real
   wording. A wrong speaker, date or outlet is corrected from the source.
5. Numbers: correct what differs, remove what is not found, relabel what is
   mislabelled, print both sides where sources disagree, and recompute what
   was derived from a corrected figure.
6. A tie between a source and the subject is stated in the sentence where the
   source is used.
7. Privacy findings come first: the sentence and the source both go.
8. The introduction and the reasoning chapter are fixed after the others:
   re-read the fixed chapters first, and rest on nothing they no longer say.
9. When you are sure a reviewer is wrong, leave the text and record why, with
   the source's wording.

Anything new you add meets the same rules as the first draft.

Edit only `md/<your file>`. Do not edit `sources.json`. Write:
- `review/fix-<NN>.md` — one line per finding, appended right after you
  apply it (see "Writing as you go" in the evidence rules): fixed, removed, relabelled,
  re-sourced (with the new URL), or rejected (with the reason);
- `review/fix-<NN>.added.json` — `{"<url>": "<Outlet Year>"}` for every source
  you opened and read in this round and now cite (`{}` when none);
- `review/fix-<NN>.raw.json` — `{"<url>": ["raw/<file>"]}` for the text you
  saved of each of those.

Final message: counts only, plus what you could not resolve.
