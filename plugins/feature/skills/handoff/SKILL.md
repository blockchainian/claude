---
name: handoff
description: >
  Write a mid-phase handoff when a session must stop before its phase's output exists —
  quota exhausted, end of day, the context safety rail in the user's CLAUDE.md, or the
  55-minute idle wake-up firing.
  Use for "/feature:handoff", "write a handoff", or when the idle wake-up prompt says to. NOT at a
  phase end: a phase's output (plan.md, the PR, verdict JSONs) is its
  own record and needs no handoff.
---

# Handoff

Whoever resumes reads the handoff together with the phase's document, or for research the
materialized findings, so the handoff points to everything else instead of restating it. Keep it
under 40 lines. Write it as `handoff.md` beside the phase document (`plan.md`), and on every later
handoff for the same topic rewrite that file in place; never date a second file or append.

## Sections

The handoff has five sections, in this order:

1. **Stopped at**: one sentence naming the phase, the step within it and why the session stopped.
2. **Done**: pointers only (commit SHAs, file paths, artifact URLs, data files, memory files),
   with no narration of what they contain.
3. **Next**: the single next step, not a list, and the command or prompt that starts it. The
   resumer decides the rest from the phase document.
4. **Unverified**: every assumption still standing, one line each, with what would settle it.
   Write "none" rather than dropping the heading.
5. **Do not redo**: what looks unfinished but is finished, with the evidence; sources already
   exhausted; queries and quotas already spent.

Every claim carries a SHA, path, URL or command; a sentence without one is narration, so cut it.
Link the plan or the report instead of summarising it, and link decisions already recorded in a
phase document or a memory file instead of recording them again.

## Development flow

In the plan → ship flow, **Done** is SHAs and phase docs. **Next** is the phase after the one that
produced the last document: writing `plan.md`, or the ship step on `plan.md`. **Do not redo** lists
the probes already green and the workstreams already merged, with the path to the check output.

## Research session

A research session (gather → analyse → derive → report) has no phase document, and mid-session its
substance lives only in tool results and assistant messages. Before writing the five sections,
materialize it into `specs/<date>-<topic>/`:

- `findings.md`: every insight derived so far, one bullet each, with its evidence (a number, a
  quote, a file path, a URL). This is the file the resumer actually reads.
- Raw data that exists only in the transcript (fetched pages, API responses, agent reports), saved
  as files. Scripts already on disk are pointers, not copies.
- `sources.md`: the queries run, the sources exhausted and the retrieval quotas spent (X, Reddit,
  opencli budgets), so nothing is fetched again.

**Done** then lists those paths, **Unverified** is the claims not yet cross-checked, and **Do not
redo** points at `sources.md`. The 40-line limit applies to the handoff, not to the materialized
files. Put the session id in **Stopped at**: the transcript under `~/.claude/projects/` holds every
tool result, and the resumer can grep one specific result out of it without reading it all.

## Idle wake-up

A Stop hook in the dotfiles schedules a one-shot 55 minutes after every turn ends. If the session is
still idle then, it fires "[idle-wakeup <session>] If still idle, write the handoff per the
feature:handoff skill and end." The hook owns the scheduling: never create, delete or reschedule
the wake-up yourself.
