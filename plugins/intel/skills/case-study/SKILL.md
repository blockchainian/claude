---
name: case-study
description: >
  Research ONE named subject in depth and produce a sourced case study of how
  they got where they are, as a PDF in the digest book format: what they
  started with, the dated growth record, the methods, the money, the failures,
  and what can be copied. One subject per run; today the subject is a creator
  (an influencer or an account that built its audience by posting). Every
  source is opened and read, a script matches every figure against the saved
  text of its source, and independent adversarial reviewers audit every source
  the book cites, every quote and what the study makes of its numbers.
  The PDF reads as a short book: a cover with the subject's name and linked accounts, an
  introduction, numbered chapters, no citations in the text. Use for "/case-study <name or profile URL>", "do a
  case study of <creator>", "how did <creator> grow", "调研一个网红",
  "做一份案例研究", "这个网红是怎么做起来的".

  NOT for: a brand or a company (not supported yet), a list of candidates or
  several subjects at once, a quick profile from a single feed, or digesting
  one article or video (use digest).
---

# Case study — one subject, researched, reviewed, typeset

A case study answers one question about one subject: how did they get here,
and which parts of it are on the record. What the reader gets is a practical
book with a cover — what the subject did, what was luck, what was their own
doing and what a reader can copy — written plainly, with no citations in the
text and nothing about how the research was done. The notes are the record;
the book is what the record teaches, and is as long as that takes. Its
figures are shown as charts and tables, not recited in sentences.

Each chapter is written once, as the text the reader gets. In `drafts/` every
sentence of it carries marks: a bracket that names its source and the kind of
its claim, and a source's own words after a quotation. The reviewers audit
that text and the fixers correct it; a script then removes the marks into
`book/`, which is typeset. Nothing is written, reviewed and fixed that the
book does not print.

Fewer solid claims beat more weak ones. A thin source list, a subject who
sells their own success story, or a failed review stops the run with a plain
report; it never becomes a padded PDF.

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at
`${CLAUDE_PLUGIN_ROOT}/skills/case-study`. Subagents cannot see this skill:
they are sent the absolute paths of the files below and read them themselves.

| File | Read by | Holds |
|---|---|---|
| `references/evidence.md` | every agent | What counts as read, the kinds of claim, who cannot be evidence |
| `references/tools.md` | every agent | The commands for pages, search, uploads, archives, records, all through `scripts/gate.mjs` |
| `types/creator.md` | every agent | The gate, source types, scout lanes, what the numbers must establish, the chapters |
| `briefs/scout.md` | scouts | Finding sources by lane |
| `briefs/read.md` | readers | Reading a batch of sources into tagged notes |
| `briefs/numbers.md` | numbers agents | The curve from the archive, the upload record |
| `briefs/write.md` | chapter writers | One chapter of the book, from the notes, with its source marks |
| `briefs/review.md` | reviewers | The three lenses, slices, the findings format |
| `briefs/fix.md` | fixers | Applying the findings to one chapter |
| `briefs/book.md` | the writer of the introduction and the reasoning chapter | The two chapters that rest on the others |
| `workflows/creator.mjs` | you | The stages below as a workflow script |
| `scripts/gate.mjs` | every agent, through the commands in `references/tools.md` | The machine-wide gate: queues, paces and retries every third-party call; its settings (`ISP_PROXY_URL`, `FETCH_X_POSTS`, `GDELT_BQ_PROJECT`) come from a `.env` file, see the end of `references/tools.md` |

## Arguments

| Argument | Meaning |
|---|---|
| the subject | A name, a handle, or a profile URL. Exactly one. |
| `--type` | `creator` (default). `brand` is not supported yet: say so and stop. |
| `--apply-to` | A product whose own accounts and creator program the reasoning chapter also covers, described in a sentence or two without its name. Without it, look for one in the project's memory or instructions; with none found, the chapter covers a person only. |
| `--out` | The PDF path. Required. |
| `--lang` | The language of the study. Default: the language the user is writing in. |

More than one subject, or a request to pick subjects, is outside this skill:
ask for one name.

## Run

1. **Set up.** Run the digest setup, then scaffold the work directory. The slug
   is the subject's name in lowercase with hyphens.

   ```bash
   "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/setup.sh"
   "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" init <slug> \
     --title "<how <Name> grew, in the study's language>" --cover "<Name>" \
     --source "<profile URL>" --out "<pdf path>" [--account "<profile URL>"]...
   ```

   The cover shows the name and, under it, the subject's accounts: each a
   link with its platform's logo. Without `--account` that is the profile URL;
   give `--account` once per account when the subject grew on more than one
   (the account they grew on first). The title is the PDF's document title. It prints the work directory (`~/Documents/case-studies/<slug>/`; override
   the root with `CASE_STUDIES_DIR`).
   An existing work directory is reused: sources and chapters already there are
   kept.

2. **Gate.** Apply the gate in the type file yourself, before spending agents:
   does the subject sell a course, coaching, a paid community or a growth tool?
   Report what you searched and what you found. If they do and the user has not
   already said to include them, stop and ask.

3. **Research, write, review and fix — the workflow.** Every stage that can run in
   parallel does, and nothing waits for a stage it does not need:

   | Stage | Agents | Does |
   |---|---|---|
   | Scout | a script fetches the news lists once (`case-study.mjs news`), then 4 scouts, one per lane; the 2 numbers agents start at once | Find sources; return URLs only. The curve comes from `gate.mjs wayback curve` in one batch |
   | Read | one per 8 sources | Read into `notes/`, tagged by chapter |
   | Write | one per chapter, as soon as the reading is merged, on Opus 4.8 | The chapter as the reader gets it, with its source marks, in `drafts/`, from the notes only |
   | Review | per chapter, as soon as it is written: a script matches its figures against the saved source text and looks up its quotations and the source's words after its reported speech there, and the quotes lens reviews it from what the script found; the record lens reviews the timeline and turning-point chapters | Findings in `review/`, every item checked |
   | Check sources | sources lens per 25 URLs, over the sources the chapters name, once every chapter is written | Findings in `review/`; a merge takes the failed sources out of `sources.json` |
   | Fix | one per chapter, when its reviews and the sources lens are done. Runs on gpt-6-luna through `codex exec` (`case-study.mjs codex`), driven by a Haiku agent; `args.fixer: sonnet` keeps it on Claude | Apply the findings to `drafts/`, then mend what the `quotes` and `figures` scripts still report |
   | Book | one, on the session's model, when every chapter is fixed | `case-study.mjs book` strips the marks into `book/` and lists the cited sources; the agent writes the introduction and the reasoning chapter from those chapters |

   A chapter runs write → review → fix on its own; the slowest chapter sets
   the time, not the slowest agent of every stage added up.

   Run it with the Workflow tool (this skill asks for it). The tool reads a
   script only from the session's working directory or scratchpad, and a
   script that changes under a running workflow makes a resume run every
   agent again: copy `${CLAUDE_PLUGIN_ROOT}/skills/case-study/workflows/creator.mjs`
   into your scratchpad directory first, and launch and resume from that copy,
   never from a repository checkout.
   `scriptPath: <scratchpad>/creator.mjs`,
   `args: { subject, work, skill, lang, today, product, seeds, caps, done, sources, fixer, names, since }`
   — `skill` is this skill's absolute folder, `names` the subject's other names, comma-separated, `since` the first year of their growth (YYYY): the news lists are fetched once for them, `product` the `--apply-to` text, `seeds` any
   starting sources you know, `caps` the machine-wide request limits from the
   tool list in one line (the script gives each agent its share). Empty
   strings where there is nothing. It runs in the background: end the turn.

   The stages run only through the Workflow tool: its `agent()` pins each
   agent's model and effort, which ad-hoc Agent spawns cannot. Never replace
   the workflow with Agent calls.

   Parallel agents never share a file. Each writes its own notes, source list,
   gaps, findings and fix log; `case-study.mjs merge` builds `sources.json`,
   `gaps.md` and the list of every source read (the last file in `drafts/`)
   from them. Reviewers are fresh
   agents, never the writers. Whether a figure is in its source is a lookup, so a
   script does it (`case-study.mjs figures`): about nine figures in ten match
   the saved text, and only the rest reach an agent — the chapter's fixer, or
   the record reviewer in the two chapters that argue from the curve. Whether
   a quotation's words are in its source is a lookup too
   (`case-study.mjs quotes`): the chapter keeps the source's own words after
   every translated quotation, the script finds them and prints the passage
   around them, and the quotes reviewer judges speaker, meaning and
   translation from that passage, searching only for what the script did
   not find. The sources and quotes lenses run on Opus 4.8 at high effort: they check whether
   something is there. The record lens keeps the session's model: it judges
   what the record supports — whether a growth step is really tied to an
   event, whether a capture list was searched in full — and the largest model
   has caught what others missed there. The review is never
   sampled and never skipped: a chapter that looked complete has, in
   practice, carried dozens of findings — sources named but never opened,
   sellers' and managers' statements written as fact, archive captures missed
   that changed a conclusion.

   What is reviewed is what is printed: a sentence the book does not print
   is not written, reviewed or fixed, and a source no chapter names is not
   vetted. So the sources lens covers the sources the chapters name, not
   every source read, and the closing list holds the same ones. Every source is still read: reading
   takes about fifteen minutes for three hundred sources, because only so
   many agents run at once; do not cut sources to save time.

4. **Verify the chapters yourself.** This step is yours and is not delegated.
   - Read the rejected findings in `review/fix-*.md`. For each, open the
     source and decide who is right.
   - Re-fetch at least two key numbers live — a point on the curve and the
     largest money figure — and compare them with the chapters.
   - Confirm that no source the reviewers failed is still in `sources.json`.
   - Run `case-study.mjs check "<work>" --draft`. It reports the findings no
     fixer applied, per chapter (`findings_unapplied`: a fixer that stopped
     early still ends its agent without an error; run
     `case-study.mjs codex "<work>" NN` for each chapter named until it prints
     `"remaining": 0`, then `case-study.mjs merge "<work>"`), missing chapters,
     chapters with no lead paragraph before their first `##`, whether
     `sources.json` is a `url → label` object, and the counts of sources,
     archive snapshots and distinct sites.

5. **Check the book.** The workflow left it in `book/`.
   - When step 4 changed a chapter in `drafts/`, strip the marks again
     (the introduction and the reasoning chapter are not written over):

     ```bash
     "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" book "<work>" --sources-title "<Sources, in the study's language>"
     ```

   - Run the check, which must pass before rendering:

     ```bash
     "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" check "<work>"
     ```

     On the book text it reports citations left in parentheses (a bracket the
     script did not take for a source mark: a label that is not in
     `sources.json`, a bracket inside a bracket), wording about
     the research, figures that are not in the chapters (chart values included),
     paragraphs that recite a run of figures instead of showing a chart,
     cited sources the closing list does not link, and links there that are not
     sources. Open each hit: a real one is corrected in `drafts/` and the
     marks stripped again, or in `book/` for the two chapters written there;
     a false one (a date in parentheses, a term that belongs to the story, the
     terms of a product given with `--apply-to`) is noted and passed.
   - Read the introduction, the reasoning chapter and one middle chapter
     yourself. The reasoning chapter says it is reasoning, presents nothing as
     a finding and names no product. Text that reads as a report of the
     research, or that documents the record without teaching a reader what to
     do, is rewritten: send the chapter back to a fresh writer with the brief
     it was written under.

6. **Render.**

   ```bash
   "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/pdf_highlights.py" render "<work>" --out "<pdf path>"
   ```

7. **Report and stop.** One short report:
   - the PDF path and page count;
   - sources read, sources the book cites, archive snapshots and distinct
     sites, from the check;
   - findings per lens, and how many were fixed, removed or rejected;
   - what you verified yourself, with the figures;
   - what remains unverified: sources read only in part, periods with no
     record, material added in the fix round that no reviewer saw;
   - the gaps that matter.

   Do not start another subject.

## Rules for the orchestrator

- The files are the deliverable; an agent's summary is its own account. Counts
  it reports ("84 sources, all read") are claims until the review confirms
  them.
- Report archive snapshots apart from sources. Forty captures of one profile
  page are one source of numbers, not forty sources.
- A product given with `--apply-to` appears only in the reasoning chapter,
  and never by name: pass its description, not its name, to the workflow. Its
  terms are givens of the task, stated as such, never findings.
- An agent that dies mid-stage (an account's usage limit, a crash) loses
  only the item it was on: every brief has the agent finish one item and
  write it to its output file before the next, and a script tells a
  relaunched reader or fixer what is left (`unread`, `findings`). Relaunch
  with `Workflow({scriptPath, resumeFromRunId})`, from the same scratchpad
  copy: finished agents replay from the cache, the others run again and pick
  up where their files stop. An agent replays only when its own prompt is
  unchanged: a changed script, or a changed arg that every prompt carries
  (`subject`, `work`, `skill`, `lang`, `today`), reruns every agent; a changed
  `product` reruns only the agent that writes the introduction and the
  reasoning chapter.
- A run that cannot be resumed (another session started it) continues from
  its files: `done: 'scout'` skips the scouts and reads the known sources
  again, into fresh notes; pass them as `sources`, the `sources` array printed
  by `case-study.mjs sources "<work>"` (an agent relaying the list drops
  entries). A source whose text any earlier agent saved is read from disk,
  not fetched; `done: 'read'` starts at the chapters, from the notes and numbers
  already in the work directory; `done: 'read, sources'` also keeps the
  sources lens's findings in `review/`. To write the chapters again, move
  `drafts/`, `book/` and the rest of `review/` aside first: writers and fixers
  continue from the files they find, and the merge reads every fixer's added
  sources.
- Slices are small on purpose. An agent's context grows with every item it
  checks and is re-read on every step, so the cost of a slice grows with the
  square of its size: two agents with 25 items cost less than one with 50.
- Machine-wide limits (a video site's session, a search quota) do not grow
  with the number of agents: pass them as `caps` and each agent gets a share.
  Archive pages are fetched only through `gate.mjs wayback`, by the archive
  numbers agent and by the one reviewer slice that re-checks the curve.
