# Plan: <feature>

<!-- Ship reads these sections. Workstreams, Dependencies and Checks are required; the rest are
optional and skipped when absent. Anything else in the file is context for the implementers. -->

## Workstreams

<!-- One `###` block per codex workstream, ending in a `Files:` line. No file may appear on two
workstreams' `Files:` lines (check-overlap.sh). Every path must exist in the repo, or carry
`(new)` on its own line (check-paths.sh). -->

### `<id>` — <what changes and where>

1. <change> — `symbol` `path.ts:NN`
Tests: <named cases>.
Files: `path/a.ts`, `path/b.ts`.

## UX workstreams

<!-- Optional. Same block shape, one `ux-implementer` agent each. Omit the section for no UX lane. -->

### `<id>` — <what changes and where>

1. <change> — `symbol` `path.tsx:NN`
Surfaces: <screen or route>, backend dependency: <none | workstream id>.
Files: `path/a.tsx`.

## Dependencies

- <order between workstreams, or `none`>
- <wire contract the UX lane codes against, as a real response body>

## New files

- `path/to/new-file.ts` (new)

## Checks

<!-- Exactly one command; ship runs it in every worktree before merge and in the session tree after.
Chain with && for more than one. -->

- `<command>`

## UX checks

<!-- Optional. Against staging, after deploy. One objective assertion per line; ship turns each
into a probe. -->

### <surface>

- <assertion>

## Live checks

<!-- Optional. Against staging, then production, after deploy. For backend behaviour a probe
cannot see: routes, webhooks, migrations, env-dependent paths. -->

- `<command>` — expect <value>
