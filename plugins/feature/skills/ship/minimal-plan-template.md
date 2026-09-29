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

<!-- One command per line, run in this order, stopping at the first red: in every worktree before
merge and in the session tree after. -->

- `<command>`
- `<command>`

## Deploy

<!-- Optional. Commands that print JSON with the deployed `sha` and exit non-zero on failure.
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

<!-- Optional. Run after each deploy, against what was deployed: staging, then production. For
behaviour a probe cannot see: routes, webhooks, migrations, env-dependent paths. -->

- `<command>` — expect <value>
