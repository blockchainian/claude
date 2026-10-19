# unminify

Rename short JavaScript bindings with model suggestions. Scripts select bindings, resolve
scope conflicts, check each rewrite and replace files atomically. The model only suggests
names; formatting stays unchanged outside identifier edits and required shorthand expansion.

## Install

For Codex installation, see [Codex installation](../../README.md#codex).

In Claude Code:

```
/plugin marketplace add blockchainian/claude
/plugin install unminify@blockchainian
```

Requires Node.js 18.18 or later, Git, and a ChatGPT account signed in with `codex login`.
Install script dependencies in `skills/unminify/scripts` with `npm ci`.

## Run

Invoke the `unminify` skill with a directory, or run its CLI:

```bash
node skills/unminify/scripts/cli.mjs <dir> \
  --concurrency 20 --max-ids 60 --context-bytes 8000 --short-len 3
```

The directory must be inside a Git repository. The command rewrites `.js` and `.mjs` files
in place using `gpt-6-luna` with low reasoning effort. Imports do not consume model requests;
a second pass follows the exporting file's binding names while keeping public names intact.
Files that cannot be parsed are skipped. Each file must pass the rewrite checks before writing.
Use Git to inspect and commit the result.

## Ignore files

The default is `.ignore` at the target repository root. Use `--ignore <file>` to select another
file. Paths are relative to that ignore file, using `.gitignore` syntax: directories, wildcards,
`!` exceptions and whole-line `#` comments. Put comments on their own lines.

```gitignore
# Dependencies and generated output
node_modules/
dist/
*.bundle.js
!keep.bundle.js
```

## Resume

Every accepted reply is appended and synced to `.unminify/<relative-path>.jsonl` at the
repository root. A file stays unchanged until all its batches finish and its rewrite passes.
Run the same command with the same batch options after interruption: completed files are
skipped and unfinished files reuse recorded replies. Keep source files unchanged while resuming;
a changed source or changed batch options is rejected. To start over, restore the source with
Git and remove its journal deliberately. Decide whether to ignore or commit `.unminify/`.

Quota exhaustion stops new requests and drains in-flight replies before exiting nonzero.
Wait for quota to reset and rerun the same command. For an authentication failure, run
`codex login` first. Credentials come from `$CODEX_HOME/auth.json` (default `~/.codex/auth.json`)
and are never written into journals. HTTP attempt logs live in `.unminify/requests.jsonl`.

Output is JSON lines with file statuses and a final summary of file counts, HTTP model
attempts, input/cached/output/reasoning tokens, and first-to-last quota percentage changes.
Total tokens are input plus output; cached and reasoning counts are subsets. Missing quota
headers are reported as `没测到`. Rejected rewrites retain their source and record a reason.

## Tests

From the marketplace root:

```bash
npm run test:unminify
npm run validate
```

The regular suite uses fixed naming tables and local services without a real model.
Run the separate end-to-end script with local sample paths to use the real login:

```bash
node plugins/unminify/skills/unminify/scripts/e2e.mjs \
  /path/to/snippet.js /path/to/lazyCache.js /path/to/lazyCache.wakaru.js
```

It copies samples into a temporary Git repository, runs the CLI, prints before/after source,
request counts and tokens, and keeps the temporary repository for inspection. Samples stay
outside this marketplace repository. This script is excluded from `npm test`.
