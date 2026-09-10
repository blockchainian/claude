# Plan: <feature>

<!-- Lines in these comments are guidance for the planner; delete them, do not copy them. -->
<!-- Refuse a problem.md whose Status line is not "no design decided", or that records the work as
shipped: there is nothing to plan. -->

Base: `<short sha>` on branch `<branch>`, <date>. Every code anchor below is `symbol` plus
`path.ts:NN` at that SHA; when they disagree, the symbol wins.

## Scope

The ask, verbatim: "<the user's words>".

In scope: <the decided items, one line each, pointing at the `problem.md` section that decided them>.
Out of scope: <what a reader might expect and must not do>.

## Facts

Repo facts come from `specs/<date>-<topic>/problem.md` at SHA `<short sha>` only. A fact that is
not in that file is a question, not an inference.

<!-- Anchors go inside the workstream steps, copied from problem.md; do not restate the file here.
File locations, test-file names and each module's package.json scripts are not facts: confirm them
with grep or glob and cite what you find in the workstream that needs them. -->

## Constraints

<!-- One line per rule every workstream obeys: the contract for missing or malformed input, the
helpers fields are read through, where thresholds live, what is deleted rather than kept, repo
conventions the implementer would otherwise guess (TDD, ABOUTME headers, no compat shims). -->

- <rule>

## Workstreams

<!-- One block per codex workstream. The block is the whole brief: the implementer reads nothing
else. -->

### `<id>` — <one sentence: what changes and where>

1. <change> — `symbol` `path.ts:NN`. <Values, ordering, edge cases.>
2. <change> — …

Tests: <named cases, one per behaviour: the drop, the keep, each boundary, each missing input>.
Files: `path/a.ts`, `path/b.ts`.

## UX workstream (Fable)

<!-- The single workstream the Fable UX agent implements on the session branch: files it owns,
surfaces it changes, whether each surface depends on the backend lane. When any frontend change
is UX-changing, every frontend change in the plan belongs here, not in a codex workstream: the
lanes never share the frontend. Write `No UX lane.` and
drop the wire-contract item, the empty JSON block and the UX checklist section when nothing a user
sees changes. -->

## Dependencies

- Between workstreams: shared files and the disjoint functions each owns; which side of a merge
  conflict to keep; order, if any.
- Backend → UX: the wire contract the UX workstream codes against, quoted from `problem.md` as a
  real response body:

```json
{ }
```

## Invariants

<!-- What no workstream may change, each with the test that proves it (a suite that must stay
green unedited, a call site that keeps its signature). -->

- <invariant> — `path/test.ts`

## UX checklist per surface

<!-- Delete this whole section, heading included, when the UX workstream reads `No UX lane.`
Otherwise one surface per heading, one objective assertion per line (element exists, computed
style, console clean, navigation happened). These become the probe. -->

### <surface>

- <assertion>

## New files

<!-- Every file that does not exist yet, marked `(new)` on the same line so the path checker
skips it. Name test files too. -->

- `path/to/new-file.ts` (new)

## Checks

<!-- Exactly one command line; the engine runs the same command in every worktree and after
merge. Call each module's test script with a path filter; never name the runner (modules may
run different runners). Chain modules with && when more than one is touched. -->

- `<command>`

After merge, run by the orchestrator: <the full suite and anything the single command omits>.

## Live checks

<!-- After staging deploy, then again after production. Each line is a command and the value it
must show; feature-specific, not the generic deploy scripts. -->

- `<command>` — expect <value>

## Outcome

<!-- Filled in by the orchestrator at phase end: staging SHA, production SHA, PR, thread
dispositions, live-check results. -->
