---
name: handoff
description: >
  Write a mid-phase handoff when a session must stop before its phase's output exists —
  quota exhausted, end of day, the context rail, or the 55-minute idle wake-up firing.
  Use for "/feature:handoff", "write a handoff", or when the idle wake-up prompt says to. NOT at a
  phase end: a phase's output (problem.md, plan.md, the PR, verdict JSONs, the report) is its
  own record and needs no handoff.
---

# Handoff — what a resumer needs, and nothing else

A resumer reads the handoff plus the phase's document (or, for research, the materialized
findings). Everything else must be pointed to, not restated. Target under 40 lines for the
handoff itself. File: `specs/<date>-<topic>/handoff.md`.

## Five sections, in this order

1. **Stopped at** — one sentence: the phase, the step within it, why the stop.
2. **Done** — pointers only: commit SHAs, file paths, artifact URLs, data files, memory
   files. No narration of what they contain.
3. **Next** — the single next step and the command or prompt that starts it.
4. **Unverified** — every assumption still standing, one line each, with what would settle
   it. Write "none" rather than omitting the heading.
5. **Do not redo** — what looks unfinished but is finished, with the evidence; sources
   already exhausted; queries and quotas already spent.

## Rules

- Every claim carries a SHA, path, URL or command. A sentence without one is narration; cut it.
- Do not summarise the plan, the grounding doc or the report. Link them.
- Do not record decisions already in a phase document or a memory file. Link them.
- One next step, not a list. The resumer decides the rest from the phase document.

## Two shapes

**Development flow** (ground → plan → orchestrate → review-fix → deploy): "Done" is SHAs and
phase docs; "Next" is the orchestrate step or the PR round; "Do not redo" is the probes already
green and the workstreams already merged (with the check output path).

**Research session** (gather → analyse → derive → report): there is no phase doc, and mid-session
the substance lives only in tool results and assistant messages. So a research handoff has a
**materialize step first**, then the five sections. Materialize into `specs/<date>-<topic>/`:
- `findings.md` — every insight derived so far, one bullet each, with its evidence (a number,
  a quote, a file path, a URL). This is the file the resumer actually reads.
- raw data that exists only in the transcript (fetched pages, API responses, agent reports)
  saved as files; scripts already on disk are pointers, not copies.
- `sources.md` — the queries run, the sources exhausted, the retrieval quotas spent (X, Reddit,
  opencli budgets), so nothing is re-fetched.
Then "Done" lists those paths; "Unverified" is the claims not yet cross-checked; "Do not redo"
points at `sources.md`. The 40-line cap applies to the handoff, not to the materialized files.
Record the session id in "Stopped at": the transcript under `~/.claude/projects/` holds every
tool result, and a resumer can pull one specific result from it by grep without reading it all.

## Trigger

The idle wake-up: when a turn ends waiting — on a background job or on the user — schedule
one one-shot `CronCreate` 55 minutes out (recurring false) whose prompt is: "If still idle,
write the handoff per the feature:handoff skill and end. Do not reschedule." Measured 2026-09-09: 49
away gaps over an hour per week cost $118 in prefix rewrites; one wake-up catches a third of
them for about ten cents each and renews the cache once.
