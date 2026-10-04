---
name: case-study
description: >
  Research ONE named subject in depth and produce a sourced case study of how
  they got where they are, as a PDF in the digest book format: what they
  started with, the dated growth record, the methods, the money, the failures,
  and what can be copied. One subject per run; today the subject is a creator
  (an influencer or an account that built its audience by posting). Every
  source is opened and read.
  The PDF reads as a short book: a cover with the subject's name and linked accounts, an
  introduction, numbered chapters, no citations in the text. Use for "/case-study <name or profile URL>", "do a
  case study of <creator>", "how did <creator> grow", "调研一个网红",
  "做一份案例研究", "这个网红是怎么做起来的".

  NOT for: a brand or a company (not supported yet), a list of candidates or
  several subjects at once, a quick profile from a single feed, or digesting
  one article or video (use digest).
---

# Case study — one subject, researched, typeset

A case study answers one question about one subject: how did they get here,
and which parts of it are on the record. What the reader gets is a practical
book with a cover — what the subject did, what was luck, what was their own
doing and what a reader can copy — written plainly, with no citations in the
text and nothing about how the research was done. The notes are the record;
the book is what the record teaches, and is as long as that takes. Its
figures are shown as charts and tables, not recited in sentences.

Each chapter is written once, as the text the reader gets. In `drafts/` every
sentence of it carries a bracket that names its source and the kind of its
claim; a script removes the marks into `book/`, which is typeset, and lists
the sources the chapters name. Nothing is reviewed or fixed after the writers.

Fewer solid claims beat more weak ones. A thin source list, a subject who
sells their own success story stops the run with a plain report; it never becomes a padded PDF.

`${CLAUDE_PLUGIN_ROOT}` below is this plugin's root; this skill lives at
`${CLAUDE_PLUGIN_ROOT}/skills/case-study`. Subagents cannot see this skill:
they are sent the absolute paths of the files below and read them themselves.

| File | Read by | Holds |
|---|---|---|
| `references/evidence.md` | every agent | What counts as read, the kinds of claim, who cannot be evidence |
| `references/tools.md` | every agent | The commands for pages, search, uploads, archives, records, all through `scripts/gate.mjs` |
| `types/creator.md` | every agent | The gate, source types, scout lanes, what the numbers must establish, the chapters |
| `briefs/scout.md` | scouts | Finding sources of one type, fast |
| `briefs/read.md` | readers | Reading a batch of sources into tagged notes |
| `briefs/numbers.md` | numbers agents | The curve from the archive, the upload record |
| `briefs/write.md` | chapter writers | One chapter of the book, from the notes, with its source marks |
| `briefs/book.md` | the writer of the introduction and the reasoning chapter | The two chapters that rest on the others |
| `workflows/creator.mjs` | you | The stages below as a workflow script |
| `scripts/gate.mjs` | every agent, through the commands in `references/tools.md` | The machine-wide gate: queues, paces and retries every third-party call; its settings (`ISP_PROXY_URL`, `FETCH_X_POSTS`, `GDELT_BQ_PROJECT`) come from a `.env` file, see the end of `references/tools.md` |

## Arguments

| Argument | Meaning |
|---|---|
| the subject | A name, a handle, or a profile URL. Exactly one. |
| `--type` | `creator` (default). `brand` is not supported yet: say so and stop. |
| `--apply-to` | A product whose own accounts and creator program the reasoning chapter also covers, described in a sentence or two without its name. Without it, the text in `~/.cache/secrets-manager/profiles/case-study/apply-to.txt`; with no such file, the chapter covers a person only. |
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

3. **News lists.** Fetch them yourself, before the workflow, for every name
   the subject has gone by — the name, the real name, every handle the
   accounts have had (an account renamed after it grew is searched under its
   old handle too) — each on its own, no URL, no description, from the first
   year of their growth:

   ```bash
   "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" news "<work>" --since <YYYY> "<name>" "<other name>"...
   ```

   It prints the articles per list and exits non-zero when a list failed.
   GDELT with its free quota of the month used up prints `GAP:` and is not a
   failure: the run goes on without it, and the report names the gap.
   Run it again until every list holds articles: what was fetched is kept, so
   a second run asks only for what is missing. Do not launch the workflow on
   a failed list; a list that keeps failing is reported and the run stops.

4. **Research and write — the workflow.** Every stage that can run in
   parallel does, and nothing waits for a stage it does not need:

   | Stage | Agents | Does |
   |---|---|---|
   | Scout | 10 scouts, one per source type, each searching all its queries at once and opening nothing; the 2 numbers agents start at once | Find sources from search results; return URLs only, the readers open them. The curve comes from `gate.mjs wayback curve` in one batch |
   | Read | one per 8 sources, per 2 videos or podcast episodes (these first) | Read into `notes/`, tagged by chapter |
   | Write | one per chapter, as soon as the reading is merged, on Opus 4.8 | The chapter as the reader gets it, with its source marks, in `drafts/`, from the notes only |
   | Book | one, on the session's model, when every chapter is written | `case-study.mjs book` strips the marks into `book/` and lists the cited sources; the agent writes the introduction and the reasoning chapter from those chapters |

   Run it with the Workflow tool (this skill asks for it). The tool reads a
   script only from the session's working directory or scratchpad, and a
   script that changes under a running workflow makes a resume run every
   agent again: copy `${CLAUDE_PLUGIN_ROOT}/skills/case-study/workflows/creator.mjs`
   into your scratchpad directory first, and launch and resume from that copy,
   never from a repository checkout.
   `scriptPath: <scratchpad>/creator.mjs`,
   `args: { subject, work, skill, lang, today, product, seeds, caps, done, sources, since }`
   — `subject` the subject's name and the names it goes by, `skill` this skill's absolute folder, `since` the first year of their growth (YYYY), which the scouts search every year from, `product` the `--apply-to` text, `seeds` any
   starting sources you know, `caps` the machine-wide request limits from the
   tool list in one line (the script gives each agent its share). Empty
   strings where there is nothing. It runs in the background: end the turn.

   The stages run only through the Workflow tool: its `agent()` pins each
   agent's model and effort, which ad-hoc Agent spawns cannot. Never replace
   the workflow with Agent calls.

   Parallel agents never share a file. Each writes its own notes, source list
   and gaps; `case-study.mjs merge` builds `sources.json`, `gaps.md` and the
   list of every source read (the last file in `drafts/`) from them. Every
   source is read: reading takes about fifteen minutes for three hundred
   sources, because only so many agents run at once; do not cut sources to
   save time.

5. **Render.** Do not recheck or edit the chapters by hand. Run the check
   and render:

   ```bash
   "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" check "<work>"
   "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/pdf_highlights.py" render "<work>" --out "<pdf path>"
   ```

   Render even when the check reports hits; list them in the report.

6. **Report and stop.** One short report:
   - the PDF path and page count;
   - sources read, sources the book cites, archive snapshots and distinct
     sites, from the check;
   - the check's hits;
   - what remains unverified: sources read only in part, periods with no
     record;
   - the gaps that matter.

   Do not start another subject.

## Rules for the orchestrator

- The files are the deliverable; an agent's summary is its own account. Counts
  come from the check, not from an agent.
- Report archive snapshots apart from sources. Forty captures of one profile
  page are one source of numbers, not forty sources.
- A product given with `--apply-to` appears only in the reasoning chapter,
  and never by name: pass its description, not its name, to the workflow. Its
  terms are givens of the task, stated as such, never findings.
- An agent that dies mid-stage (an account's usage limit, a crash) loses
  only the item it was on: every brief has the agent finish one item and
  write it to its output file before the next, and a script tells a
  relaunched reader what is left (`unread`). Relaunch
  with `Workflow({scriptPath, resumeFromRunId})`, from the same scratchpad
  copy: finished agents replay from the cache, the others run again and pick
  up where their files stop. An agent replays only when its own prompt is
  unchanged: a changed script, or a changed arg that every prompt carries
  (`subject`, `work`, `skill`, `lang`, `today`), reruns every agent; a changed
  `product` reruns only the agent that writes the introduction and the
  reasoning chapter.
- A run that cannot be resumed (another session started it) continues from
  its files: `done: 'scout'` skips the scouts and reads the known sources
  again, into fresh notes: move `notes/` to `notes.old/` first (two sets of
  notes would both be merged); pass them as `sources`, the `sources` array printed
  by `case-study.mjs sources "<work>"` (an agent relaying the list drops
  entries). A source whose text any earlier agent saved is read from disk,
  not fetched; `done: 'read'` starts at the chapters, from the notes and numbers
  already in the work directory. To write the chapters again, move `drafts/`
  and `book/` aside first: writers continue from the files they find.
- Slices are small on purpose. An agent's context grows with every item it
  checks and is re-read on every step, so the cost of a slice grows with the
  square of its size: two agents with 25 items cost less than one with 50.
- Machine-wide limits (a video site's session, a search quota) do not grow
  with the number of agents: pass them as `caps` and each agent gets a share.
  Archive pages are fetched only through `gate.mjs wayback`, by the archive
  numbers agent.
