# Fix brief

Three independent reviewers audited the case study. Their findings are in the
work directory: `review/sources.md`, `review/numbers.md`, `review/quotes.md`.
Apply every finding. The standard is `references/evidence.md` (next to this
brief's folder).

1. A failed source leaves `sources.json`, and every sentence resting on it is
   removed or re-sourced. Re-sourced means you open the original now and read
   the fact there. If the original cannot be opened, the claim goes and the
   attempt goes into `gaps.md`.
2. An interview hosted by a seller keeps only the subject's own words, labelled
   self-reported, in a sentence that names the host as a seller.
3. A source never opened is opened now, and the text corrected to what it says,
   or it leaves the text.
4. A quote not found loses its quotation marks and the claim, or takes the real
   wording. A wrong speaker, date or outlet is corrected from the source.
5. Numbers: correct what differs, remove what is not found, relabel what is
   mislabelled, print both sides where sources disagree. Where a reviewer found
   record points the study lacked, open those records yourself, save them under
   `raw/`, add them, and recompute everything derived from the old figures.
6. A tie between a source and the subject is stated in the sentence where the
   source is used.
7. Privacy findings come first: the source leaves `sources.json`, `notes.md`
   and the chapters, and its download leaves `raw/`.
8. The reasoning chapters may not rest on anything removed; rewrite what was.
9. The sources chapter matches `sources.json` exactly, and says how many
   sources were read only in part.
10. When you are sure a reviewer is wrong, leave the text and record why, with
    the source's wording.

Anything new you add in this round meets the same rules as the first draft.

Write `review/fix-log.md`: one line per finding ID from each review file —
fixed, removed, relabelled, re-sourced (with the new URL), or rejected (with
the reason) — then the sources removed, the sources added, and the new
material added that no reviewer has seen.

Final message: counts only, per review file, plus what you could not resolve.
