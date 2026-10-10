# Plan: <feature>

<!-- Ship reads these sections. Workstreams, Dependencies and Checks are required; the rest are
optional and skipped when absent. Anything else in the file is context for the implementers. -->

## Workstreams

<!-- One `###` block per codex workstream, ending in a `Files:` line. No file may appear on two
workstreams' `Files:` lines (check-overlap.sh). Every path must exist in the repo, or carry
`(new)` on its own line (check-paths.sh). `Rules:` is optional: what the code does at the edges
the steps do not cover — empty, full, duplicate, out-of-range or malformed input, and what happens
when a read, a write or a call fails. A rule left out is one the implementer has to ask about. -->

### `<id>` — <what changes and where>

1. <change> — `symbol` `path.ts:NN`
Rules: <edge case> → <behaviour>; <failure> → <behaviour>.
Tests: <named cases>.
Files: `path/a.ts`, `path/b.ts`.

## UX workstreams

<!-- Optional. Same block shape, one `ux-implementer` agent each. Omit the section for no UX workstream. -->

### `<id>` — <what changes and where>

1. <change> — `symbol` `path.tsx:NN`
Surfaces: <screen or route>, backend dependency: <none | workstream id>.
Files: `path/a.tsx`.

## Dependencies

<!-- A workstream whose Checks cannot pass without what another workstream produces — a module,
a crate, a table, a schema, a generated file — is listed here after that one; ship starts it when
that one merges, so its worktree already carries the dependency. Workstreams not listed here start
at once. -->

- <order between workstreams, or `none`>
- <wire contract the UX workstream codes against, as a real response body>

## New files

- `path/to/new-file.ts` (new)

## Checks

<!-- One command per line, run in this order, stopping at the first red: in every worktree before
merge and in the session tree after. -->

- `<command>`
- `<command>`

## Deploy

<!-- Optional. Commands that print JSON with the deployed `sha` (and any other field the Deploy
checks need, such as `url`) and exit non-zero on failure.
Without a staging line, UX checks run after the last merge; without a production line, the
merge is the ship. -->

- staging: `<command>`
- production: `<command>`

## UX checks

<!-- Optional. One probe command per line: it drives the UI, prints a verdict JSON and exits
non-zero on failure. The command names its own target (a staging URL, a local dev server, a
simulator). Ship runs them after the staging deploy when Deploy has one, else after the last merge. -->

- `<command>` — <surface it covers>

## Deploy checks

<!-- Optional. Run after each deploy, against what was deployed: staging, then production. Each
command gets `DEPLOY_ENV` (the deploy line's label) and every top-level string field of the
deploy's JSON as `DEPLOY_<FIELD>` (`DEPLOY_SHA` always; `DEPLOY_URL` when the deploy prints a
`url`). For behaviour a probe cannot see: routes, webhooks, migrations, env-dependent paths. -->

- `<command>` — expect <value>
