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

The input is the user's ask, in whatever shape they think in — a goal, a list
of requirements, a design, or all three at once. The sweep works from six
fields (Ask, Example, Accept when, Constraints, Premises, Keep unchanged);
steps 1–2 read and sort the input into them so the user never has to pre-sort
into a schema shaped for this skill. Every factual claim in it, wherever it sat
in the input, is unverified until checked; a refuted one becomes an open
question, never a silent fix.

The output is one file, `specs/<date>-<topic>/problem.md`, in the shape of
`${CLAUDE_PLUGIN_ROOT}/skills/ground/problem-template.md`. Its consumer is a later
planning turn that works from this file alone, so the doc must be clear,
succinct and sufficient on its own. How that turn runs is not this skill's
concern.

## Procedure

1. **Read each statement's mood; resolve what is ambiguous.** For each line of
   the ask decide: current fact, wanted change, or unclear. "The flag defaults
   to on" can be either, and the code contradicts both readings the same way,
   so only the user can say. Ask all unclear lines in one prompt, one yes/no
   each: "<line> — requirement?". Yes = wanted change. No = a claim about
   current code; verify it like any premise, and if the code refutes it, report
   it as refuted. Never guess. With no user to ask, reject and list the flagged
   lines; tell the author to write facts as "X currently does Y" and wants as
   "change Y to Z".

2. **Classify the survivors, and confirm what will be tested.** Sort the
   mood-clear material into the six fields and echo the mapping back in one
   short block before sweeping. The one classification that carries weight:
   separate a **decision the user is imposing** (a Constraint — respected, not
   tested) from a **claim about how the code works** (a Premise — verified). A
   Constraint is respected, but the facts it rests on are not exempt: "keep the
   verdict JSON shape" presupposes that JSON exists with a known shape — pull
   that presupposition out as a Premise and verify it with the rest.
   List the Premises back under "here is what I will try to break — confirm or
   correct." Skip the echo-back only for an ask that carries no claim
   about existing behavior to pull — one stated purely as goal and imposed
   decisions; tidy phrasing is not that case, since a well-organized design is
   exactly where a confident but unverified premise hides, so extract the
   claims anyway. Don't turn a genuinely claim-free ask into an interview.

   The **Accept when** material is the desired state decomposed into testable
   behaviors, and it defines the feature: it goes into the doc's "Accepted when"
   section verbatim, one per bullet, numbered `AC<n>` and tagged with the kind of
   test that settles it (unit | integration | ui | benchmark). Carry the criteria
   through — they are optative, never swept for truth — but treat each like a
   Constraint for its presuppositions: a criterion citing `GET /trades`
   presupposes that route, so pull that out as a Premise and verify it with the
   rest. If the ask describes a desired state but carries no acceptance criteria,
   do not invent them and do not proceed on the desired state alone: interactively,
   ask the user to state them — "how do we know <desired state> holds? one testable
   line each" — in a single prompt; non-interactively, reject with
   `MISSING ACCEPTANCE: <ask> states a desired state but no Accepted-when` and the
   uncovered elements listed. A feature grounded here always leaves with acceptance
   criteria; "none" is not a valid Accepted-when.

3. **Name the decision and pin the base commit.** One sentence on what is
   being changed and why now. Record `git rev-parse --short HEAD`; every line
   number in the doc is valid only at that SHA.

4. **Sweep, read-only, delegated.** Spawn Explore agents (`model: "sonnet"`)
   for the file-finding fan-out; they return locations and conclusions, not
   file dumps. Run the measuring commands yourself: probes, D1 queries, a
   real request against the producer. Nothing enters the doc from recall.

   Run this skill from the main session: the harness blocks nested spawning
   (the plugin's `subagent-no-spawn` hook tells each subagent so), so a
   subagent invoking `/ground` must sweep by hand.

5. **Write the doc** from the template, then check its paths and anchors
   from inside the repo and fix every flag before stopping:

   ```
   ${CLAUDE_PLUGIN_ROOT}/skills/ground/check-paths.sh specs/<date>-<topic>/problem.md [skip-regex]
   ${CLAUDE_PLUGIN_ROOT}/skills/ground/check-anchors.py specs/<date>-<topic>/problem.md [skip-regex]
   ${CLAUDE_PLUGIN_ROOT}/skills/ground/check-acceptance.py specs/<date>-<topic>/problem.md
   ```

   All three exit 1 on any flag. The optional regex skips paths that are
   deliberately not repo files (recordings, third-party URLs). A bare
   basename that matches several files is `AMBIGUOUS`; write the full path.
   `check-acceptance.py` on the problem doc alone flags `NO-ACCEPTANCE` when
   the "Accepted when" section is empty and `DUPLICATE: AC<n>` when a criterion
   id repeats; fix the section, not the checker.

   `check-anchors.py` reads every `path:line` at the doc's `Base:` commit and
   requires a symbol named in the same sentence (a backticked identifier or a
   "quoted phrase") within 3 lines of the cited range: `UNVERIFIED` when it
   is not there, `NOSYMBOL` when the sentence names nothing, `PAST-EOF`,
   `MISSING`. A backticked `name()` call must be cited by an anchor in its
   own bullet (`UNANCHORED`). Fix the doc, not the symbol: a flag means the
   line, the file or the claim is wrong, and the printed lines show which.

6. **Stop.** Do not propose a design in this turn, not even a sketch.

## What counts as a fact

Every claim carries its evidence inline, or it does not go in:

- **Code** — `path.ts:120` at the recorded SHA, with the symbol named in
  the same sentence. Quote the line if a plan will depend on its exact shape;
  a claim about what a function does quotes the branch it rests on rather
  than paraphrasing it.
- **Behavior, limits, volumes** — the command and its real output, dated.
- **Wire fields, upstream shapes** — a real response body from the producer,
  never a type definition or a client-side interface. A spec-invented wire
  field once cleared every gate and broke in prod. When the feature reproduces
  the shape rather than merely reading a field from it, also save the body as
  `fixtures/<domain>.json` beside the doc and cite that path in the bullet;
  skip this when there is no external producer contract to reproduce.
- **Constraints already decided** — link the memory or prior handoff rather
  than restating it.

**Facts a plan's gate or fix will rest on.** Pin these before a wrong one
breaks the work later, where no exit code catches it:

- **The `--check` command, run on the clean baseline.** If the plan will
  gate on a build/check command, run it at the base SHA. Record pass/fail
  and which errors are pre-existing. A gate already red on baseline is not
  a code signal. The plan must then gate on a differential (new errors in
  touched files only) or on a command that actually passes. A whole codex
  run can write correct code and still be discarded against a check that
  never passed on that module.
- **The existing client for any third-party API the plan will call or
  probe.** When a repo module exists to talk to a service, read it before
  improvising auth or transport. It holds the real recipe: token exchange,
  required headers, anti-bot client. Improvising instead burns probe after
  probe rediscovering what the module already encodes.
- **The real cap behind any numeric bound a fix pins.** A limit or clamp
  value the plan cites must match the existing validation or schema cap it
  flows into. A value that contradicts it — a clamp to 150 into a
  `max(100)` schema — passes mocked tests and fails only live.

**Facts a spec-shaped or external-API goal rests on.** A goal that
generates a deliverable file or live-tests a third-party API carries
premises that a "complete" criterion never tests. The project's own
specifics (proxy/gateway, filenames, conventions) live in its memory or
AGENTS.md — pull them in; the general premises to pin are:

- **The on-disk layout of any deliverable the goal names by filename.**
  `ls` the sibling/precedent directories. Pin where the analogous artifact
  lives and whether it is standalone or folded into another doc; an
  existing repo convention is a knowable fact. If the requested filename
  has NO precedent, record it as an open question and surface the
  consistency conflict before authoring. Building the literal name first
  means reworking it onto the convention later.
- **A spec/data deliverable's acceptance criterion should assert it was
  verified against the live source this session, not merely "complete".**
  "Complete" lets verification defer, the user inject it later, and the
  session re-enter the whole verification method at full cost. Phrase the
  Accept-when as live-verified this session.
- **The live-test protocol for any third-party/external API the plan will
  probe, pinned before the first call.** Four things: the sanctioned
  **egress** (the project's proxy/gateway, never the session/home IP for
  an abuse-sensitive API); the provider's **rate-limit model** (buckets,
  ban behavior, pacing floor, no fan-out); the **read/write route split**
  (enumerate and exclude state-changing / money-moving routes from live
  tests); and **sample-capture redaction** (strip keys, auth headers and
  user-identifying fields before any sample is written).

**Diagnosis is not design.** A candidate mechanism named as a question with
what would settle it belongs in Open questions: "Does the upstream feed drop a
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
