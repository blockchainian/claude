# Plan: <feature>

<!-- Lines in these comments are guidance for whoever writes the plan; delete them, do not copy them. -->
<!-- Ship requires Workstreams (each block ending in a Files: line), Dependencies and Checks; every
other section is optional and read only by the ship step that names it. -->
<!-- This file is the agent-facing spec: nothing in it is for the user, and no decision rationale,
alternative rejected, risk or shipped outcome belongs here. Decisions stay with the user's plan
review; the shipped outcome goes to outcome.md (post-ship, outcome-template.md). A codex workstream
reads only the shared core — Scope, Facts, Constraints, Dependencies, Invariants — plus its own
block, so that set must be necessary and sufficient to build the block correctly without any other
section. -->

Base: `<short sha>` on branch `<branch>`, <date>. Every code anchor below is `symbol` plus
`path.ts:NN` at that SHA; when they disagree, the symbol wins.

## Scope

The ask, verbatim: "<the user's words>".

In scope: <the decided items, one line each>.
Out of scope: <what a reader might expect and must not do>.

## Facts

<!-- Repo facts the workstreams build on, each checked in the repo at the Base SHA and cited as
`symbol` `path.ts:NN`. A fact that was not checked is a question, not an inference. File locations,
test-file names and each module's package.json scripts are confirmed with grep or glob and cited in
the workstream that needs them. -->

## Constraints

<!-- One line per rule every workstream obeys: the contract for missing or malformed input, the
helpers fields are read through, where thresholds live, what is deleted rather than kept, repo
conventions the implementer would otherwise guess (TDD, ABOUTME headers, no compat shims). -->

- <rule>

## Workstreams

<!-- One block per codex workstream. The block is the whole brief: the implementer reads nothing
else. Every behaviour the ask requires becomes at least one named test here or in the UX checklist
— a ui behaviour belongs in the UX checklist, the rest in a workstream's Tests line, a benchmark in
Tests or Live checks. -->

### `<id>` — <one sentence: what changes and where>

1. <change> — `symbol` `path.ts:NN`. <Values, ordering, edge cases.>
2. <change> — …

<!-- If this workstream reproduces an external producer's wire contract (a pinned
`fixtures/<domain>.json` exists), its tests MUST round-trip that fixture — feed the fixture's own
returned id/cursor/timestamp/date back through the code and assert the output matches the
fixture; a literal invented in the test is a defect. If no fixture is pinned, this does not
apply. -->
Tests: <named cases, one per behaviour: the drop, the keep, each boundary, each missing input>.
Files: `path/a.ts`, `path/b.ts`.

<!-- Every workstream, codex or UX, ends with a `Files:` line; check-overlap.sh reads them. -->

## UX workstreams

<!-- One block per UX workstream, each implemented by its own Fable UX agent, all launched at once.
The block is the whole brief: files it owns, surfaces it changes, whether each surface depends on
the backend lane. Split by surface whenever the file sets are disjoint — a program that touches
many screens is many workstreams in ONE plan, never one plan per screen. A file belongs to exactly
one workstream (check-overlap.sh enforces it); a shared file goes to the workstream that changes
it most, and the others code against its committed interface. When any frontend change is
UX-changing, every frontend change in the plan belongs here, not in a codex workstream: the lanes
never share the frontend. Write `No UX lane.` and drop the wire-contract item, the empty JSON
block and the UX checklist section when nothing a user sees changes. -->

### `<id>` — <one sentence: what changes and where>

1. <change> — `symbol` `path.tsx:NN`. <Values, ordering, edge cases.>
2. <change> — …

Surfaces: <screen or route>, backend dependency: <none | workstream id>.
Files: `path/a.tsx`, `path/b.tsx`.

## Dependencies

- Between workstreams: shared files and the disjoint functions each owns; which side of a merge
  conflict to keep; order, if any.
- Backend → UX: the wire contract the UX workstreams code against, quoted as a real response
  body:

```json
{ }
```

## Invariants

<!-- What no workstream may change, each with the test that proves it (a suite that must stay
green unedited, a call site that keeps its signature). -->

- <invariant> — `path/test.ts`

## Intended changes

<!-- Behavior this plan deliberately changes from before, so the reviewer reads the diff as intent
and does not file it as a regression (a stub or null replaced with a real value, a signature that
moves on purpose). The opposite of an Invariant; never mix the two. One line each, with the test
that proves the new behavior. Delete the section if nothing changes behavior. -->

- <behavior> now <new> (was <old>) — intended, not a regression — `path/test.ts`

## UX checklist per surface

<!-- Delete this whole section, heading included, when the UX workstreams read `No UX lane.`
Otherwise one surface per heading, one objective assertion per line (element exists, computed
style, console clean, navigation happened). These become the probe. -->

### <surface>

- <assertion>

## New files

<!-- Every file that does not exist yet, marked `(new)` on the same line so the path checker
skips it. Name test files too. -->

- `path/to/new-file.ts` (new)

## Checks

<!-- Exactly one command line; implement.sh runs the same command in every worktree and after
merge. For each touched module compose its gate from the project AGENTS.md's "Checks (the gate)" —
its type/compile check, its linter, and its tests with a path filter; never name the runner
(modules may run different runners). A green test+lint is not proof the module compiles: the
type/compile check is required whenever the project declares one. Chain modules with && when more
than one is touched. -->

- `<command>`

After merge, run by the orchestrator: <the full suite and anything the single command omits>.

## Live checks

<!-- After staging deploy, then again after production. Each line is a command and the value it
must show; feature-specific, not the generic deploy scripts. -->

- `<command>` — expect <value>

<!-- No Outcome section: the orchestrator writes the shipped record to outcome.md
(outcome-template.md) at phase end, keeping this file input-only. -->
