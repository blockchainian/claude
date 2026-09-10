---
name: ground
description: >
  Pin repo facts BEFORE a planning turn, so the planner transcribes checked
  facts instead of inferring them. Use when about to plan or design any
  change against an existing codebase — "plan X", "design Y", "write a spec
  for Z" — and especially before handing a spec to /codex:implement, whose
  gates test whether code passes, never whether a premise was true.
  Produces a grounding doc and stops before design. NOT for planning itself,
  and not for changes small enough to hold in one turn.
---

# Ground — pin the premises before planning

A model that needs a repo fact it does not have fills the gap fluently, and
the plan then carries the guess as a premise. No downstream gate catches it:
exit codes test whether the code passes, never whether the premise was true,
and review afterwards has poor recall. The counter is to remove the
opportunity: investigate in a turn that is forbidden to design, and write
only what was observed.

The input is the user's ask in the shape of `docs/user-template.md` (Ask,
Example, Accept when, Constraints, Premises, Keep unchanged). Every factual
claim in it, on the Premises line or not, is unverified until checked; a
refuted one becomes an open question, never a silent fix.

The output is one file, `specs/<date>-<topic>/problem.md`, in the shape of
`${CLAUDE_PLUGIN_ROOT}/skills/ground/problem-template.md`. Its consumer is a later
planning turn that works from this file alone, so the doc must be clear,
succinct and sufficient on its own. How that turn runs is not this skill's
concern.

## Procedure

1. **Name the decision and pin the base commit.** One sentence on what is
   being changed and why now. Record `git rev-parse --short HEAD`; every line
   number in the doc is valid only at that SHA.

2. **Sweep, read-only, delegated.** Spawn Explore agents (`model: "sonnet"`)
   for the file-finding fan-out; they return locations and conclusions, not
   file dumps. Run the measuring commands yourself: probes, D1 queries, a
   real request against the producer. Nothing enters the doc from recall.

   Run this skill from the main session: the `SubagentStart` hook
   (the plugin's `subagent-no-spawn` hook) blocks nested spawning, so a
   subagent invoking `/ground` must sweep by hand.

3. **Write the doc** from the template, then check its paths and anchors
   from inside the repo and fix every flag before stopping:

   ```
   ${CLAUDE_PLUGIN_ROOT}/skills/ground/check-paths.sh specs/<date>-<topic>/problem.md [skip-regex]
   ${CLAUDE_PLUGIN_ROOT}/skills/ground/check-anchors.py specs/<date>-<topic>/problem.md [skip-regex]
   ```

   Both exit 1 on any flag. The optional regex skips paths that are
   deliberately not repo files (recordings, third-party URLs). A bare
   basename that matches several files is `AMBIGUOUS`; write the full path.

   `check-anchors.py` reads every `path:line` at the doc's `Base:` commit and
   requires a symbol named in the same sentence (a backticked identifier or a
   "quoted phrase") within 3 lines of the cited range: `UNVERIFIED` when it
   is not there, `NOSYMBOL` when the sentence names nothing, `PAST-EOF`,
   `MISSING`. A backticked `name()` call must be cited by an anchor in its
   own bullet (`UNANCHORED`). Fix the doc, not the symbol: a flag means the
   line, the file or the claim is wrong, and the printed lines show which.

4. **Stop.** Do not propose a design in this turn, not even a sketch.

## What counts as a fact

Every claim carries its evidence inline, or it does not go in:

- **Code** — `path.ts:120` at the recorded SHA, with the symbol named in
  the same sentence. Quote the line if a plan will depend on its exact shape;
  a claim about what a function does quotes the branch it rests on rather
  than paraphrasing it.
- **Behavior, limits, volumes** — the command and its real output, dated.
- **Wire fields, upstream shapes** — a real response body from the producer,
  never a type definition or a client-side interface. A spec-invented wire
  field once cleared every gate and broke in prod
  (`cross-module-contract-probe-upstream`).
- **Constraints already decided** — link the memory or prior handoff rather
  than restating it.

**Diagnosis is not design.** A candidate mechanism named as a question with
what would settle it belongs in Open questions: "Does GMGN drop a
subscription on a duplicate subscribe? Settled by <probe>." Choosing among
candidates, or proposing a change, does not: "We should re-subscribe on
reconnect" waits for the planning turn.

**Unverified is always present.** Every fact wanted and not confirmed goes
there, one per line, with what would settle it. Write "none" rather than
deleting the heading; a gap that is merely absent gets filled by the planner.

**Succinct** means the planner reads it in one pass: no narrative of the
sweep, no file dumps, no restated code that a `path:line` already points to.

## Scaling

This costs a real extra turn. Spend it when a wrong premise is expensive:
cross-module contracts, migrations, money/fee/PnL paths, auth, anything
going to `/codex:implement`. Skip it for a change small enough that the
planner reads every relevant file in the same turn anyway.
