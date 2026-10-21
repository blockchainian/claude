---
name: unminify
description: Restore JavaScript syntax and rename minified identifiers with scope-safe rewrites.
---

# Unminify JavaScript

## Skill directory

Set `SKILL_DIR` to the absolute directory containing this loaded `SKILL.md` in each shell
call. Resolve it from the loaded skill path, not the caller's working directory or a
host-specific plugin variable.

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Run

Install dependencies with `(cd "$SKILL_DIR/scripts" && npm ci)` if needed. Pass the one or
more target directories or files supplied by the user; they must be inside one Git repository
and run together in one process sharing one rate limit. A directory selects its files; a file
is selected the same way. Check its Git status
before rewriting so existing work is visible.

```bash
node "$SKILL_DIR/scripts/cli.mjs" <path>... \
  --concurrency 20 --max-input-tokens 100000 --min-symbol-length 4
```

The model is fixed to `gpt-6-luna` with low reasoning effort. It reads the user's Codex login
from `$CODEX_HOME/auth.json`, defaulting to `~/.codex/auth.json`. Run `codex login` if required.
Never print credentials.

Without `--chunks`, before planning any names the command runs all 13 first-batch syntax steps in registry
order in every selected file. Only converted source ranges are replaced; surrounding
text stays byte-identical. Inserted statement blocks use the surrounding indentation
step; extracted expression statements drop parentheses only when statement grammar
and evaluation semantics allow it. Fragment files retain program binding names, declaration kinds
and evaluation order. Their numeric `void` expressions are restored only when the Git-root
binding index (or the supplied input set outside Git) contains no program binding named
`undefined`, and the current file passes binding and dynamic-scope checks. Direct
API calls without index context retain numeric `void` in fragments.

The command selects parseable `.js`/`.mjs` files and asks for names of bindings shorter
than `--min-symbol-length`, including default and namespace import locals but excluding
named imports. It resolves naming conflicts, checks each completed file and writes it atomically. Naming
instructions prohibit duplicate names in the same scope and reserve `is`/`has`/`should`/`can`
prefixes for boolean variables and functions returning booleans. A conflicting suggestion
gets one more model request with the occupied names and binding context before numeric
suffixes are used. These requests obey the input token budget and are journaled for resume.
Top-level
batches finish before function batches. Within each file and stage, pending batches are greedily
packed in order within `--max-input-tokens` (default 100000), with no name-count cap.
Planning and packing count the actual instructions, prompt and rendered context using
`js-tiktoken` 1.0.21 with `o200k_base`. Oversized contexts are shortened; an irreducible
batch that still cannot fit fails before sending. Placeholders are unique across
the request; original batch IDs stay stable across resume. Collision replies use
stable binding-specific journal IDs.
At the end of every run, including interrupted runs, imports in journal-completed files
follow export names without another model request. Unfinished files supply their current
export names but remain untouched so their journals can resume. External named import aliases
restore the imported name deterministically when it is at least `--min-symbol-length`
characters long; shorter imported names stay unchanged and never go to the model.
When the imported name is occupied by a same-scope binding or would change a reference's
binding, restoration first tries the source's last path segment in camelCase plus the
capitalized imported name (for example, `node:path/posix` + `join` becomes `posixJoin`).
Strip `node:` before taking the last segment; scoped packages also use the last segment.
Duplicate imports with the exact same `(source, imported)` identity retain numeric suffixes
when they alone occupy the imported name; normal runs merge these duplicates before naming.
If the source-prefixed name also collides, use the existing numeric suffix resolver. Unoccupied imported
names retain their existing allocation order; unused enclosing or nested names do not collide.
Canonically spaced aliases collapse to shorthand when the restored local equals the imported name; other
spacing and comments are preserved. Public export names stay unchanged.

## Explicit chunks

Use `--chunks <manifest.json>` when files share one program scope without importing
one another. The command validates the manifest against `scripts/chunks.schema.json`
(JSON Schema 2020-12). It reads no recovery mapping. Recovered fragment headers
(`// Recovered from <chunk> lines A-B`) determine original import order.

```json
{
  "version": 1,
  "chunks": [{ "id": "example", "files": ["src/first.js", "src/last.js"] }]
}
```

Paths use forward slashes and are relative to the manifest's directory. Every file must
exist inside the target Git root, every chunk ID must be unique, and each actual file can
belong to only one chunk, including path or symlink aliases. Unlisted files retain the
ordinary file workflow.

```bash
node "$SKILL_DIR/scripts/cli.mjs" <path>... --chunks <manifest.json> \
  --chunk-siblings --concurrency 20 --max-input-tokens 100000 --min-symbol-length 4
```

By default, a name is eligible only when its declaration and all its references are in
the selected files. Add `--chunk-siblings` to update references in unselected sibling
files of the same chunk. Model naming still starts with selected declarations; import
following may also update an outside import declaration used by a selected sibling,
and restore external named imports used only within unselected chunk siblings.
Ignored files are never rewritten. A name is frozen if more than one chunk member
declares it, any declaration or reference is ignored, or it is a known environment or
JavaScript global. Direct `eval` or `with` freezes all binding names in the chunk, including
function locals and import aliases, because their dynamic references cannot be indexed. The `planned` JSON event reports frozen-name counts by reason.

Initial naming proposes each eligible program name once; collisions may trigger a retry.
Context includes declaration excerpts and
sibling reference excerpts, using the same actual token budget and greedy batch packing.
Collision resolution considers all chunk program bindings, visible bindings at every
reference, and unbound globals throughout the chunk. Public properties and export names
stay unchanged: `export { a }` becomes `export { readableName as a }`.
Import locals follow the resolved exported binding's name without model requests, including
references and export locals in sibling files; collision suffixes remain scope-safe.
An unchanged internal exporting binding leaves its chunk import alias unchanged.
External named imports use the same length threshold and deterministic restoration as
ordinary files. Short default and namespace import locals are model candidates, including
their references in authorized sibling files.

Before naming, a separate stage merges duplicate external imports by `(source, imported)`
within each chunk, or within an ordinary single file. External sources do not start with
`.`; named, default and namespace imports are handled, including mixed declarations.
The earliest recovered fragment line keeps its binding; within a file, source order wins.
References, shorthand properties and export locals follow that binding. Any inner binding
that would capture a redirected reference freezes the entire group. Selection, sibling
authorization, ignores and dynamic-scope restrictions still apply. Its dedicated guard
checks every reference's import identity, first external-module load order and all other
bindings, and permits only the planned source edits. Naming checks use the merged code
as their baseline. Ordinary files have independent `.unminify/merge-imports/` checkpoints;
chunk merges replay in memory under the chunk journal's pipeline options and transaction.

A chunk prepares selected-file syntax changes in memory, merges imports, names its program bindings,
then names selected-file function bindings and follows import names. Individual-file checks and chunk binding,
global, export-interface and source-text checks must all pass before any member is written.
A parse or scope-check failure leaves the whole chunk intact. Each source fragment must
be independently parseable; slices through a token or function body are rejected.
Unselected siblings receive only the authorized shared-name edits.

Chunk journals live in `.unminify/chunks/`. They store accepted replies and durable
whole-chunk write intents, including rewritten source, to finish a partially written
transaction without model requests. Recovery validates every member before writing any
remaining member and runs before selection or syntax. Manifest membership, selected paths,
ignore membership, batch options and syntax pipeline must match when resuming; source
changes fail explicitly. Files already owned by ordinary rename journals cannot be
silently moved into chunk journals. Use the same command to resume.

## Ignore and resume

Read the target Git root's `.ignore`, or supply `--ignore <file>`. Rules use `.gitignore`
syntax relative to the ignore file: directories, wildcards, `!` exceptions and whole-line
`#` comments; do not use trailing comments. The plugin supplies no project-specific ignore list.

Syntax checkpoints live separately in `.unminify/syntax/<relative-path>.syntax.jsonl`.
They record pipeline fingerprints and durable write intents, without rename offsets or
model replies. Resume completes syntax before constructing rename plans from the resulting
text. Existing rename journals retain ownership of their files and are resumed without
rerunning syntax. An unfinished syntax checkpoint rejects changed source or pipeline options.

Accepted replies are synced immediately to `.unminify/<relative-path>.jsonl` at the target
Git root. `.unminify/requests.jsonl` appends each model or token-refresh HTTP attempt;
`.unminify/performance.jsonl` appends run, file, phase and shared 429 pause boundaries.
Both use a run ID and epoch-millisecond timestamps. Attempts include queue/send/headers/end
times, retry cause, in-flight and queued counts, file/batch IDs/name count/phase, body bytes and HTTP/2
session ID (changes on reconnect), plus status, usage, `estimated_input_tokens` and safe rate headers. Credentials,
auth-file contents and source text are never logged in these telemetry files.

Rerun the same command and batch
options after interruption. Completed files are skipped; unfinished files reuse recorded
replies against the unchanged original source. Do not delete journals during a resumable run.
If source or batch options changed, stop and report the mismatch rather than discard records.
The user decides whether `.unminify/` should be ignored or committed.

On `USAGE_LIMIT`, the command stops issuing requests, drains running replies and exits
nonzero. Wait for quota reset, then rerun the same command. On `AUTH`, run `codex login`,
then rerun. A failed rewrite keeps the file intact and records the reason; report that reason.

## Report

Parse the JSON status lines and final summary. Report changed, skipped and rejected files,
model HTTP-attempt count (including retries), median and mean names per request, estimated versus server input token totals and first-to-last quota percentage changes; if quota headers
were absent, report `没测到`. Inspect the Git diff and leave committing target source to the
caller. The final summary is also appended to `performance.jsonl`: total and phase elapsed
milliseconds, HTTP-attempt p50/p90/max, time-weighted average and peak in-flight requests,
429 count, shared pause elapsed time and HTTP attempts per minute (including refreshes).
Phase times merge overlapping intervals within each phase; different phases can overlap.
Per-file `cpu_ms` counts local elapsed milliseconds for parsing, planning, rendering,
resolving/applying names and rewrite checks, including child-process time. Nested steps are
inclusive, so do not add them together. `written_at` is null when no write completed.

Analyze recorded logs without calling a model:

```bash
node "$SKILL_DIR/scripts/perf.mjs" <target-repo> \
  --window-seconds 10 --slow-files 10
# Add --run-id <id> to select an earlier run; the default is the latest start.
```

The JSON report shows sends, completions, end-of-window and peak in-flight counts, mean
completed-attempt duration and 429 responses per window, phase times and slowest files.
It also reports model HTTP-attempt count and `names_per_request` median/mean (retries included).
It compares total `estimated_input_tokens` with server `input_tokens`, including retries;
server framing and schema accounting may differ from the input-text estimate.
An interrupted run uses its recorded boundaries; incomplete trailing JSONL records are ignored.
Do not run the live-model `scripts/e2e.mjs` unless the user requested it.
