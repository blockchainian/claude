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
  and quote and what the study makes of its numbers.
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
and which parts of it are on the record. The work has two layers. The sourced
draft (`md/`) names the evidence behind every sentence and is what the
reviewers audit. The book (`book/`) is what the reader gets: a practical book
with a cover, distilled from the draft — what the subject did, what was luck,
what was their own doing and what a reader can copy —
written plainly, with no citations in the text and nothing about how the
research was done. The draft is the record; the book is what the record
teaches, and is as long as that takes. Its figures are shown as charts and tables,
not recited in sentences.

Fewer solid claims beat more weak ones. A thin source list, a subject who
sells their own success story, or a failed review stops the run with a plain
report; it never becomes a padded PDF.

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at
`${CLAUDE_PLUGIN_ROOT}/skills/case-study`. Subagents cannot see this skill:
they are sent the absolute paths of the files below and read them themselves.

| File | Read by | Holds |
|---|---|---|
| `references/evidence.md` | every agent | What counts as read, the kinds of claim, who cannot be evidence |
| `references/tools.md` | every agent | Login-free commands for pages, press, uploads, archives, records |
| `types/creator.md` | every agent | The gate, source types, scout lanes, what the numbers must establish, the chapters |
| `briefs/scout.md` | scouts | Finding sources by lane |
| `briefs/read.md` | readers | Reading a batch of sources into tagged notes |
| `briefs/numbers.md` | numbers agents | The curve from the archive, the upload record |
| `briefs/write.md` | draft writers | One chapter of the sourced draft, from the notes |
| `briefs/review.md` | reviewers | The three lenses, slices, the findings format |
| `briefs/fix.md` | fixers | Applying the findings to one chapter |
| `briefs/book.md` | book writer | How the reviewed draft becomes the text the reader gets |
| `workflows/creator.js` | you | The stages below as a workflow script |

## Arguments

| Argument | Meaning |
|---|---|
| the subject | A name, a handle, or a profile URL. Exactly one. |
| `--type` | `creator` (default). `brand` is not supported yet: say so and stop. |
| `--apply-to` | A product whose own accounts and creator program the reasoning chapter also covers, described in a sentence or two without its name. Without it, look for one in the project's memory or instructions; with none found, the chapter covers a person only. |
| `--out` | The PDF path. Default: `<store>/case-studies/<slug>.pdf`. |
| `--tools` | A file listing tool commands tested on this machine (logins, paid readers). Agents prefer it over `references/tools.md`. |
| `--lang` | The language of the study. Default: the language the user is writing in. |

More than one subject, or a request to pick subjects, is outside this skill:
ask for one name.

## Run

1. **Set up.** Run the digest setup, then scaffold the work directory. The slug
   is the subject's name in lowercase with hyphens.

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/setup.sh"
   "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" init <slug> \
     --title "<how <Name> grew, in the study's language>" --cover "<Name>" \
     --source "<profile URL>" --out "<pdf path>" [--account "<profile URL>"]...
   ```

   The cover shows the name and, under it, the subject's accounts: each a
   link with its platform's logo. Without `--account` that is the profile URL;
   give `--account` once per account when the subject grew on more than one
   (the account they grew on first). The title is the PDF's document title. It prints the work directory (`<store>/.work/<slug>/`, the digest
   store).
   An existing work directory is reused: sources and chapters already there are
   kept.

2. **Gate.** Apply the gate in the type file yourself, before spending agents:
   does the subject sell a course, coaching, a paid community or a growth tool?
   Report what you searched and what you found. If they do and the user has not
   already said to include them, stop and ask.

3. **Research, review and fix — the workflow.** Every stage that can run in
   parallel does, and nothing waits for a stage it does not need:

   | Stage | Agents | Does |
   |---|---|---|
   | Scout | 4, one per lane; the 2 numbers agents start with them | Find sources; return URLs only. The curve comes from `scripts/wayback.mjs curve` in one batch |
   | Read | one per 8 sources | Read into `notes/`, tagged by chapter |
   | Write | one per chapter; then the introduction and the reasoning chapter | The sourced draft in `md/`, from the notes only |
   | Review | sources lens per 25 URLs, from the merge on, alongside the writers; per chapter, as soon as it is written: a script matches its figures against the saved source text, and the quotes lens reviews it; the record lens reviews the timeline and turning-point chapters | Findings in `review/`, every item checked |
   | Fix | one per chapter, as soon as its two reviews and the sources lens are done; the introduction and the reasoning chapter after the others | Apply the findings to `md/` |

   A chapter runs write → review → fix on its own; the slowest chapter sets
   the time, not the slowest agent of every stage added up.

   Run it with the Workflow tool (this skill asks for it):
   `scriptPath: ${CLAUDE_PLUGIN_ROOT}/skills/case-study/workflows/creator.js`,
   `args: { subject, work, skill, lang, today, tools, product, seeds, caps }`
   — `skill` is this skill's absolute folder, `tools` is the tool list file, `product` the `--apply-to` text, `seeds` any
   starting sources you know, `caps` the machine-wide request limits from the
   tool list in one line (the script gives each agent its share). Empty
   strings where there is nothing. It runs in the background: end the turn.

   The stages run only through the Workflow tool: its `agent()` pins each
   agent's model and effort, which ad-hoc Agent spawns cannot. Never replace
   the workflow with Agent calls.

   Parallel agents never share a file. Each writes its own notes, source list,
   gaps, findings and fix log; `case-study.mjs merge` builds `sources.json`,
   `gaps.md` and the draft's sources chapter from them. Reviewers are fresh
   agents, never the writers. Whether a figure is in its source is a lookup, so a
   script does it (`case-study.mjs figures`): about nine figures in ten match
   the saved text, and only the rest reach an agent — the chapter's fixer, or
   the record reviewer in the two chapters that argue from the curve. The
   sources and quotes lenses run on Opus at high effort: they check whether
   something is there. The record lens keeps the session's model: it judges
   what the record supports — whether a growth step is really tied to an
   event, whether a capture list was searched in full — and the largest model
   has caught what others missed there. The review is never
   sampled and never skipped: a first draft that looked complete has, in
   practice, carried dozens of findings — sources named but never opened,
   sellers' and managers' statements written as fact, archive captures missed
   that changed a conclusion.

   Two stages are expected to take longer, on purpose. Reading takes about
   fifteen minutes for three hundred sources, because only so many agents run
   at once; do not cut sources to save time. The book (step 5) is one writer
   from the first page to the last, because splitting it breaks the reading.

4. **Verify the draft yourself.** This step is yours and is not delegated.
   - Read the rejected findings in `review/fix-*.md`. For each, open the
     source and decide who is right.
   - Re-fetch at least two key numbers live — a point on the curve and the
     largest money figure — and compare them with the chapters.
   - Confirm that no source the reviewers failed is still in `sources.json`.
   - Run `case-study.mjs check "<work>" --draft`. It reports missing chapters,
     chapters with no lead paragraph before their first `##`, whether
     `sources.json` is a `url → label` object, and the counts of sources,
     archive snapshots and distinct sites.

5. **Write the book — one fresh agent.** Spawn one general-purpose subagent
   that has not seen the research. Its message gives the absolute paths of
   `briefs/book.md` and the type file, the work directory and the language. It
   reads `md/` and writes `book/NN.md`. Then:
   - Run the check, which must pass before rendering:

     ```bash
     "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" check "<work>"
     ```

     On the book text it reports citations left in parentheses, wording about
     the research, figures that are not in the draft (chart values included),
     paragraphs that recite a run of figures instead of showing a chart,
     sources the closing list does not link, and links there that are not
     sources. Open each hit: a real
     one goes back to the writer; a false one (a date in parentheses, a term
     that belongs to the story) is noted and passed.
   - Read the introduction and one middle chapter yourself. Text that reads as
     a report of the research, or that documents the record without teaching
     a reader what to do, goes back to the writer.

6. **Render.**

   ```bash
   "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/pdf_highlights.py" render "<work>" --out "<pdf path>"
   ```

7. **Report and stop.** One short report:
   - the PDF path and page count;
   - sources, archive snapshots and distinct sites, from the check;
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
  nothing: every brief has the agent write its output file as it goes and,
  when relaunched with the same message, read that file and continue. Relaunch
  with `Workflow({scriptPath, resumeFromRunId})`: finished agents replay from
  the cache, the others run again and pick up where their files stop. Do not
  change the script or the args before resuming, or every agent reruns.
- Slices are small on purpose. An agent's context grows with every item it
  checks and is re-read on every step, so the cost of a slice grows with the
  square of its size: two agents with 25 items cost less than one with 50.
- Machine-wide limits (a video site's session, a search quota) do not grow
  with the number of agents: pass them as `caps` and each agent gets a share.
  Archive pages are fetched only through `scripts/wayback.mjs`, by the archive
  numbers agent and by the one reviewer slice that re-checks the curve.
