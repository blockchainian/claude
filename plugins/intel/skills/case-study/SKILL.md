---
name: case-study
description: >
  Research ONE named subject in depth and produce a sourced case study of how
  they got where they are, as a PDF in the digest book format: what they
  started with, the dated growth record, the methods, the money, the failures,
  and what can be copied. One subject per run, a creator (an influencer or an
  account that built its audience by posting). Every source is opened and read.
  The PDF reads as a short book: a cover with the subject's name and linked
  accounts, an introduction, numbered chapters, no citations in the text.
  Use for "/case-study <name or profile URL>", "do a case study of <creator>",
  "how did <creator> grow", "调研一个网红", "做一份案例研究",
  "这个网红是怎么做起来的".

  NOT for: a brand or a company (not supported yet), a list of candidates or
  several subjects at once, a quick profile from a single feed, or digesting
  one article or video (use digest).
---

# Case study

A case study answers one question about one subject: how did they get here,
and which parts of it are on the record. The reader gets a practical book with
a cover that says plainly what the subject did, what was luck, what was their
own doing and what a reader can copy, with no citations and nothing about how
the research was done. Fewer solid claims beat more weak ones: a thin source
list, or a subject who sells their own success story, stops the run with a
plain report, never a padded PDF.

## Skill directory

`${CLAUDE_PLUGIN_ROOT}` is this plugin's root, and this skill lives at
`${CLAUDE_PLUGIN_ROOT}/skills/case-study`. Subagents cannot see this skill, so
the workflow sends them the absolute paths of the files below.

| File | Read by | Holds |
|---|---|---|
| `references/evidence.md` | every agent | What counts as read, the kinds of claim, who cannot be evidence |
| `references/tools.md` | every agent | The commands for pages, search, uploads, archives and records, all through `scripts/gate.mjs` |
| `types/creator.md` | every agent | The gate, source types, scout workstreams, what the numbers must establish, the chapters |
| `briefs/scout.md` | scouts | Finding sources of one type |
| `briefs/read.md` | readers | Reading a batch of sources into tagged notes |
| `briefs/numbers.md` | numbers agents | The curve from the archive, the upload record |
| `briefs/write.md` | chapter writers | One chapter, from the notes, with its source marks |
| `briefs/book.md` | the book agent | The introduction and the reasoning chapter |
| `workflows/creator.mjs` | you | The research and writing stages as a workflow script |

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `RESIDENTIAL_PROXY_URL` | Residential proxy for news and archive requests | When needed | ~/.config/intel/.env |
| `ISP_PROXY_URL` | ISP proxy pool | When needed | ~/.config/intel/.env |
| `BIGQUERY_PROJECT_ID` | Google Cloud project ID for GDELT BigQuery queries | For GDELT | ~/.config/intel/.env |
| `INTEL_STATE_DIR` | State root; default ~/.local/state/intel, with work directories under case-study/work/, fetched news under case-study/news/ and rate limits under limits/ | Optional | ~/.config/intel/.env |
| `INTEL_OUTPUT_DIR` | Output root; default ~/Documents, with the PDFs under case-studies/ | Optional | ~/.config/intel/.env |

The Exa key is `exaApiKey` in `~/.mcporter/mcporter.json`, not in the `.env`.
After rotating it, delete `<INTEL_STATE_DIR>/limits/exa-out-of-credits`, or
searches skip Exa for up to 10 minutes.

## Arguments

Take exactly one subject: a name, a handle or a profile URL. When the user
gives several, or asks you to pick subjects, ask for one name.

| Argument | Meaning |
|---|---|
| `--type` | `creator`, the default. `brand` is not supported yet: say so and stop. |
| `--apply-to` | A product whose own accounts and creator program the reasoning chapter also covers, described in a sentence or two without its name. Without it, use the text in `~/.config/intel/case-study-apply-to.txt`; with no such file, the chapter covers a person only. |
| `--out` | The PDF path, only when the user names one. Default: `case-studies/<slug>.pdf` under the output root. |
| `--lang` | The language of the study. Default: the language the user is writing in. |

## Set up

Run the digest setup, then scaffold the work directory. The slug is the
subject's name in lowercase with hyphens.

```bash
"${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/setup.sh"
"${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" init <slug> \
  --title "<how <Name> grew, in the study's language>" --cover "<Name>" \
  --source "<profile URL>" [--out "<pdf path>"] [--account "<profile URL>"]...
```

The title becomes the PDF's document title, and the cover lists the subject's
accounts under the name. Without `--account` the cover shows the profile URL;
when the subject grew on more than one platform, give `--account` once per
account, leading with the one they grew on first. `init` prints the work
directory and the PDF path (`pdf`), and it reuses an existing work directory
with its sources and chapters.

## Gate

Before spending any agents, apply the gate in the type file yourself: does the
subject sell a course, coaching, a paid community or a growth tool? Report what
you searched and what you found. If they do and the user has not already said
to include them, stop and ask.

## News lists

Fetch the news lists yourself before the workflow, starting from the first
year of the subject's growth, for every name they have gone by: the name, the
real name, and every handle the accounts have had, including the old handle of
an account renamed after it grew.

```bash
"${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" news "<work>" --since <YYYY> "<name>" "<other name>"...
```

The command prints the articles per list and exits non-zero when a list
failed. A `GAP:` line means GDELT's free monthly quota is used up; that is not
a failure, the run goes on and the report names the gap. Run the command again
until every list holds articles, since it fetches only what is missing. Never
launch the workflow on a failed list: report a list that keeps failing and stop
the run.

## Research and write

The workflow runs ten scouts (one per source type) and the two numbers agents,
then the readers, then one writer per chapter, then the book agent for the
introduction and the reasoning chapter. It runs only in Claude Code, through
the Workflow tool, whose `agent()` pins each agent's model and effort; never
replace it with Agent calls.

The Workflow tool reads a script only from the session's working directory or
scratchpad, and a script that changes under a run makes a resume rerun every
agent. Copy `${CLAUDE_PLUGIN_ROOT}/skills/case-study/workflows/creator.mjs`
into your scratchpad, and launch and resume from that copy, never from a
repository checkout:

- `scriptPath: <scratchpad>/creator.mjs`
- `args: { subject, work, skill, lang, today, product, seeds, caps, done, sources, since }`,
  with empty strings where there is nothing:
  - `subject`: the subject's name and the names it goes by
  - `skill`: this skill's absolute folder
  - `since`: the first year of growth (YYYY)
  - `product`: the `--apply-to` text
  - `seeds`: any starting sources you know
  - `caps`: the machine-wide request limits from the tool list, in one line

The workflow runs in the background, so end the turn. Every source is read,
which takes about fifteen minutes for three hundred; never cut sources to save
time.

## Render

Do not recheck or edit the chapters by hand. Run the check and then render,
even when the check reports hits:

```bash
"${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case-study.mjs" check "<work>"
"${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/pdf_highlights.py" render "<work>" --out "<pdf path>"
```

## Report

Give one short report, then stop without starting another subject. It covers:

- the PDF path and page count;
- sources read, sources the book cites, archive snapshots and distinct sites,
  from the check;
- the check's hits;
- what remains unverified: sources read only in part, periods with no record;
- the gaps that matter.

Take every count from the check, never from an agent's summary. Report archive
snapshots apart from sources: forty captures of one profile page are one source
of numbers, not forty sources.

## Rules for the orchestrator

- Test a changed or new tool before a run, never inside one.
- A product given with `--apply-to` reaches the workflow only as its
  description, never its name. Its terms are givens of the task, never
  findings.

## Resuming a run

An agent that dies mid-stage loses only the item it was on. Relaunch with
`Workflow({scriptPath, resumeFromRunId})` from the same scratchpad copy:
finished agents replay from the cache and the others pick up where their files
stop. An agent replays only when its own prompt is unchanged, so a changed
script, or a changed `subject`, `work`, `skill`, `lang` or `today`, reruns
every agent, while a changed `product` reruns only the book agent.

A run that cannot be resumed, because another session started it, continues
from its files:

- `done: 'scout'` skips the scouts and reads the known sources again into fresh
  notes. Move `notes/` to `notes.old/` first, or both sets are merged. Pass as
  `sources` the array that `case-study.mjs sources "<work>"` prints, never a
  list relayed by an agent. Texts already saved are read from disk, not
  fetched.
- `done: 'read'` starts at the chapters, from the notes and numbers already in
  the work directory.
- To write the chapters again, move `drafts/` and `book/` aside first, because
  writers continue from the files they find.

## Tests

```bash
node --test "${CLAUDE_PLUGIN_ROOT}/skills/case-study/tests/"*.mjs
```
