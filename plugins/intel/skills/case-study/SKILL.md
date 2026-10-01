---
name: case-study
description: >
  Research ONE named subject in depth and produce a sourced case study of how
  they got where they are, as a PDF in the digest book format: what they
  started with, the dated growth record, the methods, the money, the failures,
  and what can be copied. One subject per run; today the subject is a creator
  (an influencer or an account that built its audience by posting). Every
  source is opened and read, every number is checked against the record, and
  three independent adversarial reviewers audit every source, number and quote
  before the PDF is made. Use for "/case-study <name or profile URL>", "do a
  case study of <creator>", "how did <creator> grow", "调研一个网红",
  "做一份案例研究", "这个网红是怎么做起来的".

  NOT for: a brand or a company (not supported yet), a list of candidates or
  several subjects at once, a quick profile from a single feed, or digesting
  one article or video (use digest).
---

# Case study — one subject, researched, reviewed, typeset

A case study answers one question about one subject: how did they get here,
and which parts of it are on the record. The product is a PDF a reader can act
on, in which every claim shows what kind of evidence stands behind it.

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
| `types/creator.md` | every agent | The gate, source types, what the numbers must establish, the chapters |
| `briefs/research.md` | researcher | Target, files, order of work, self-check |
| `briefs/review.md` | reviewers | The three lenses and the findings format |
| `briefs/fix.md` | researcher, fix round | How each kind of finding is applied |

## Arguments

| Argument | Meaning |
|---|---|
| the subject | A name, a handle, or a profile URL. Exactly one. |
| `--type` | `creator` (default). `brand` is not supported yet: say so and stop. |
| `--apply-to` | A product to write the last reasoning chapter for, described in a sentence or two. Without it, look for one in the project's memory or instructions; with none found, that chapter is left out. |
| `--out` | The PDF path. Default: `<store>/case-studies/<slug>.pdf`. |
| `--tools` | A file listing tool commands tested on this machine (logins, paid readers). Agents prefer it over `references/tools.md`. |
| `--lang` | The language of the study. Default: the language the user is writing in. |

More than one subject, or a request to pick subjects, is outside this skill:
ask for one name.

## Run

1. **Set up.** Run the digest setup, then scaffold the work directory. The slug
   is the subject's name in lowercase with hyphens. Use 12 chapters with a
   product, 11 without.

   ```bash
   bash "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/setup.sh"
   "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case_study.py" init <slug> \
     --title "<Name>: <how they grew, in the study's language>" \
     --source "<profile URL>" --out "<pdf path>" --chapters <11|12>
   ```

   It prints the work directory (`<store>/.work/<slug>/`, the digest store).
   An existing work directory is reused: sources and chapters already there are
   kept.

2. **Gate.** Apply the gate in the type file yourself, before spending agents:
   does the subject sell a course, coaching, a paid community or a growth tool?
   Report what you searched and what you found. If they do and the user has not
   already said to include them, stop and ask.

3. **Research — one agent.** Spawn one general-purpose subagent. Its message
   gives: the subject; the absolute paths of `briefs/research.md`,
   `references/evidence.md`, `references/tools.md` and the type file; the work
   directory; the language; the tool list file if any; the product description
   if any; starting sources you already know; and request caps when other
   agents share the machine. It writes the files and returns counts. Run it in
   the background and end the turn.

4. **Review — three independent agents, one lens each.** When the researcher
   returns, spawn three fresh general-purpose subagents at once — never forks,
   never the researcher. Each message gives the absolute path of
   `briefs/review.md`, the evidence and type files, the work directory, the
   tool list, and one lens: `sources`, `numbers`, or `quotes`. Add the points
   the researcher itself reported as doubtful, as things to test, not as facts.
   Each reviewer checks every item, writes `review/<lens>.md`, and returns
   counts and its five worst findings. Reviewers inherit the session's model.

   The review is never sampled and never skipped. A first draft that looked
   complete has, in practice, carried dozens of findings: sources named but
   never opened, sellers' and managers' statements written as fact, and archive
   captures missed that changed a conclusion.

5. **Fix — the researcher.** Send the researcher (it still holds the context)
   the absolute path of `briefs/fix.md` and the worst findings from all three
   reviews. It applies every finding and writes `review/fix-log.md`.

6. **Verify yourself.** This step is yours and is not delegated.
   - Read `review/fix-log.md`. For each rejected finding, open the source and
     decide who is right.
   - Re-fetch at least two key numbers live — a point on the curve and the
     largest money figure — and compare them with the chapters.
   - Confirm that no source the reviewers failed is still in `sources.json`.
   - Run the check, which must pass before rendering:

     ```bash
     "${CLAUDE_PLUGIN_ROOT}/skills/case-study/scripts/case_study.py" check "<work>"
     ```

     It reports missing chapters, chapters with no lead paragraph before their
     first `##` (the typesetting needs one), whether `sources.json` is a
     `url → label` object, and the counts of sources, archive snapshots and
     distinct sites.

7. **Render.**

   ```bash
   "${CLAUDE_PLUGIN_ROOT}/skills/digest/scripts/pdf_highlights.py" render "<work>" --out "<pdf path>"
   ```

8. **Report and stop.** One short report:
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
- A product named with `--apply-to` appears only in the reasoning chapter
  written for it. Its terms are givens of the task, stated as such, never
  findings.
- Agents share one machine: archive requests, video-site requests and search
  quotas are per machine, not per agent. Give each agent its caps in its
  message, and space archive requests at 60 seconds while several agents run.
- A layered variant — parallel readers by source type, one agent owning the
  numbers, writers working only from the readers' notes — is under evaluation.
  Until it is adopted here, run the flow above.
