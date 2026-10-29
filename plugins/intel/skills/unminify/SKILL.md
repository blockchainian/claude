---
name: unminify
description: Organize JavaScript files by dependencies, restore syntax and rename minified identifiers with scope-safe rewrites.
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
node "$SKILL_DIR/scripts/cli.mjs" rename-files <path>... \
  --concurrency 20 --max-input-tokens 100000
node "$SKILL_DIR/scripts/cli.mjs" restore-syntax <path>...
node "$SKILL_DIR/scripts/cli.mjs" rename-bindings <path>... \
  --concurrency 20 --max-input-tokens 100000 --min-symbol-length 4
node "$SKILL_DIR/scripts/cli.mjs" sync-imports <path>... --min-symbol-length 4
```

Each command selects parseable `.js`/`.mjs` files and applies the ignore rules first.
Selection checks parseability in the syntax worker pool, returning only a boolean and
parse time. The same pool reads and indexes repository fragment context in bounded
windows, returning only per-file binding summaries. The main thread merges summaries
in file order within `selection_plan`. Git root and ignore rules are loaded once per run;
chunk siblings are selected together with the same rules.
All commands accept `--ignore <file>` and `--chunks <file>`. `rename-files` and `rename-bindings` accept
`--concurrency` and `--max-input-tokens`; `--min-symbol-length` belongs to
`rename-bindings` and `sync-imports`. Other options fail with `invalid option`. A missing
or unknown command prints usage and exits nonzero. Run the four commands in order. `rename-files` treats the supplied paths after ignores as
the entire code set; it neither expands chunk siblings nor adds outside dependencies.
The other three stages resume independently and may process modules in batches in either order.

The model is fixed to `gpt-6-luna` with low reasoning effort. It reads the user's Codex login
from `$CODEX_HOME/auth.json`, defaulting to `~/.codex/auth.json`. Run `codex login` if required.
Never print credentials.

`restore-syntax` runs all 13 first-batch syntax steps in registry order, then merges
duplicate external imports, in every selected file and all selected chunk members.
File syntax passes run in a worker pool sized by `os.availableParallelism()`. Chunk
fragments run independently; import merging analyzes each whole chunk, with separate
chunks running in parallel. The `check-syntax` stage uses the same pool. Workers
return source and edit counts; the main thread retains the existing checkpoints and
atomic writes, preserving serial output bytes and interruption recovery.
Only converted source ranges are replaced; surrounding
text stays byte-identical. Inserted statement blocks use the surrounding indentation
step; extracted expression statements drop parentheses only when statement grammar
and evaluation semantics allow it. Fragment files retain program binding names, declaration kinds
and evaluation order. Their numeric `void` expressions are restored only when the Git-root
binding index (or the supplied input set outside Git) contains no program binding named
`undefined`, and the current file passes binding and dynamic-scope checks. Direct
API calls without index context retain numeric `void` in fragments.

`rename-bindings` checks every selected file and its entire chunk before creating a model client:
if rerunning syntax restoration or import merging would change any file, it lists every
such file and requires `restore-syntax`. It only names bindings, resolves collisions,
checks rewrites and writes results; it never restores syntax or follows imports.
Successful prechecks are cached per file or chunk using source hashes, chunk order, ignored
members, fragment flags, shadowed syntax globals and pipeline versions. Unchanged groups
reuse that verdict on resume; failed checks are never cached.
Chunks totaling at least 256 KiB run in separate worker threads. Each worker owns one chunk
and exits after it finishes. The worker count is bounded by available cores, one worker per
8 GiB of machine memory and a maximum of eight; each worker has a 3 GiB old-generation heap
limit. Smaller groups share one main-thread consumer. All model requests use the same global
request slots. Packed replies append their original batch records together and sync them
before continuing, preserving journal IDs and durable resume.
It asks for names of bindings shorter
than `--min-symbol-length`, including default and namespace import locals but excluding
named imports. It resolves naming conflicts, checks each completed file and writes it atomically. Naming
instructions prohibit duplicate names in the same scope and reserve `is`/`has`/`should`/`can`
prefixes for boolean variables and functions returning booleans. A conflicting suggestion
gets one more model request with the occupied names and binding context before numeric
suffixes are used. These requests obey the input token budget and are journaled for resume.
Top-level
batches finish before function batches. Within each file and stage, pending batches are greedily
packed in order within `--max-input-tokens` (default 100000), with no planning name-count cap.
Naming requests keep their planned names together. When a reply reaches its output
limit, only complete JSON string pairs are accepted, and missing names are requested
once with the same rendered context. Names still missing keep their original bindings.
Combined replies are journaled under the original batch IDs; a batch with no accepted
names stays unfinished for resume. Existing replies replay unchanged, and attempt
telemetry retains the original batch IDs.
Planning and packing count the actual instructions, prompt and rendered context using
`js-tiktoken` 1.0.21 with `o200k_base`. Oversized contexts are shortened; an irreducible
batch that still cannot fit fails before sending. Placeholders are unique across
the request; original batch IDs stay stable across resume. Collision replies use
stable binding-specific journal IDs.
`sync-imports` follows imports without requesting model names or restoring syntax.
Its import summaries, edits and file checks use the same syntax worker pool with bounded
in-flight files. Workers release each file's Babel objects after extracting compact binding,
reference and edit records. The main thread resolves proposals once per chunk, reuses
unchanged file records during validation and releases completed chunk records before continuing.
Run it after naming, including an interrupted naming run. It covers the current selection
and discovers all completed ordinary and chunk journals in the target Git root, including files outside this batch.
It verifies their current contents against the recorded results before following imports;
changed or missing files are skipped and reported. Historical files receive only import
following, never model naming. Ordinary files pass rewrite checks and journaled atomic
writes; chunks pass whole-chunk checks and transactional writes. Ignored files are never
written. Running selections in batches produces the same final file bytes as running
them together. Unfinished files supply their current export names but remain untouched
so their journals can resume. External named import aliases
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

## Rename files

`rename-files` checkpoints six stages in `.unminify/rename-files/`: dependency graph,
file summaries, recursive communities, directory names, file names and file moves.
Only summary requests include source code. Later requests use summaries and directory paths.
Summary tasks are greedily packed using the complete tokenized input and share
`--concurrency` slots. Every accepted per-file result is synced immediately. The same Codex
client, model and reasoning effort serve all naming requests.

Webpack dependencies come from numeric calls through the wrapper factory's third parameter;
ordinary dependencies come from relative imports and re-exports. Edges include only selected
files. Seeded Leiden recursively partitions each induced graph. High incoming-degree outliers
use the IQR rule; they form a separate recursive group only when removing them yields at least
two communities with higher modularity. A graph with no further structure is one leaf directory.
Directory counts and depth follow the graph without directory-tree limits.

Directories receive names from the bottom up. Files receive names from their summaries and
full directory paths. Names are lowercase kebab-case basenames; file extensions are preserved.
Collisions receive one request containing occupied names, then numeric suffixes if necessary.
The tree is placed under the selected files' common parent directory. Existing destination
entries are reserved. After every name is durable, a checked transaction moves selected files,
rewrites their relative imports and re-exports, and updates selected paths in `--chunks`.
Relative references outside the selection retain their targets. Ignored files remain untouched.
The final JSON summary includes `tree`, `files`, `directories`, `max_depth`, requests and tokens.

Resume with the same paths, ignore rules, chunks and input-token budget. Saved stages and
accepted model replies are reused. Source and manifest changes reject a pending plan;
a durable move intent recovers interrupted writes without further model calls. The saved
original paths remain valid resume arguments after files have moved.

`rename-files` refuses to run if any later command directory exists. Later commands refuse
an unfinished `rename-files` directory. Before writing logs, every command rejects legacy
`.unminify/` layouts and asks for migration; it never moves old logs automatically.

| Command | Logs and checkpoints under `.unminify/` |
|---|---|
| `rename-files` | `rename-files/`: stages, accepted replies, move plan, per-file move status, `requests.jsonl`, `performance.jsonl` |
| `restore-syntax` | `restore-syntax/syntax/`, `restore-syntax/imports/`, `restore-syntax/performance.jsonl` |
| `rename-bindings` | `rename-bindings/files/`, `rename-bindings/chunks/`, `rename-bindings/check-syntax/`, `rename-bindings/requests.jsonl`, `rename-bindings/performance.jsonl` |
| `sync-imports` | `sync-imports/`, including chunk transactions and `performance.jsonl` |

Legacy entries are root-level `syntax/`, `merge-imports/`, `chunks/`, `syntax-preflight/`,
`imports/`, `requests.jsonl`, `performance.jsonl`, or ordinary file journals. Move these logs
to the corresponding command directories before running a command. `check-syntax` timings
use the phase name `check_syntax`.

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
node "$SKILL_DIR/scripts/cli.mjs" restore-syntax <path>... --chunks <manifest.json>
node "$SKILL_DIR/scripts/cli.mjs" rename-bindings <path>... --chunks <manifest.json> \
  --concurrency 20 --max-input-tokens 100000 --min-symbol-length 4
node "$SKILL_DIR/scripts/cli.mjs" sync-imports <path>... --chunks <manifest.json> \
  --min-symbol-length 4
```

Selecting one chunk member includes the entire chunk in syntax restoration and the
`check-syntax` stage. Model naming targets only bindings declared in selected files,
including program and function bindings; sibling declarations receive no model names.
References to selected program bindings follow their renamed declarations throughout
nonignored siblings. Import following also updates relative imports when the exporting
binding has been renamed, including declarations and references in chunk siblings.
External named imports are also restored throughout those siblings.
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
Current and historical chunks use their recorded selection and
the same import authorization, so running together or in batches produces identical bytes.
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
as their baseline. Ordinary files have independent `.unminify/restore-syntax/imports/` checkpoints;
chunk syntax and merges use independent group checkpoints in `.unminify/restore-syntax/syntax/chunks/`.

Each chunk stage writes its own checked group transaction. `restore-syntax` validates
syntax and merges before committing the group. `rename-bindings` names program and
function bindings and commits checked names. `sync-imports` follows imports in a separate
checked transaction. Chunk binding, global, export-interface and source-text checks gate
naming and import writes. A rejected stage leaves that stage's input intact. Each source
fragment must be independently parseable; slices through a token or function body are rejected.

Chunk journals live in `.unminify/rename-bindings/chunks/`. They store accepted replies and durable
whole-chunk write intents, including rewritten source, to finish a partially written
transaction without model requests. Recovery validates every member before writing any
remaining member and runs before selection or syntax. Manifest membership, ignore
membership, batch options and syntax pipeline must match when resuming; source changes
fail explicitly. Selected paths are tracked separately from output options. A changed
selection first finishes any pending selection using its saved replies and source
fingerprints, then processes additional selected files from the committed chunk.
Completed selections are skipped; newly selected declarations still receive naming.
Files already owned by ordinary rename journals cannot be silently moved into chunk
journals. Historical completed chunks can follow imports even when the current command
does not supply their manifest, using their recorded selection and sibling authorization.

## Ignore and resume

Read the target Git root's `.ignore`, or supply `--ignore <file>`. Rules use `.gitignore`
syntax relative to the ignore file: directories, wildcards, `!` exceptions and whole-line
`#` comments; do not use trailing comments. The plugin supplies no project-specific ignore list.

Syntax checkpoints live separately in `.unminify/restore-syntax/syntax/<relative-path>.syntax.jsonl`.
They record pipeline fingerprints and durable write intents, without rename offsets or
model replies. Rerun `restore-syntax` to complete syntax before invoking `rename-bindings`.
Existing rename journals retain ownership of their files and are resumed without
rerunning syntax. An unfinished syntax checkpoint rejects changed source or pipeline options.

Accepted replies are synced immediately to `.unminify/rename-bindings/files/<relative-path>.jsonl` at the target
Git root. `requests.jsonl` inside each model command directory appends each model or token-refresh HTTP attempt;
`performance.jsonl` inside each command directory appends run, file, phase and shared 429 pause boundaries.
Both use a run ID and epoch-millisecond timestamps. Attempts include queue/send/headers/end
times, retry cause, in-flight and queued counts, file/batch IDs/name count/phase, body bytes and HTTP/2
session ID (changes on reconnect), plus status, usage, `estimated_input_tokens` and safe rate headers. Credentials,
auth-file contents and source text are never logged in these telemetry files.

Keep the same output and batch options after interruption; the selected paths may change.
Completed files in each stage are skipped; unfinished naming files reuse recorded replies
against the unchanged
original source. Do not delete journals during a resumable run.
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
`selection_plan` covers file selection and naming plans; ordinary-file duplicate import
merging is recorded separately as `merge_imports`.
Phase times merge overlapping intervals within each phase; different phases can overlap.
Per-file `cpu_ms` counts local elapsed milliseconds for parsing, planning, rendering,
resolving/applying names and rewrite checks, including child-process time. Nested steps are
inclusive, so do not add them together. `written_at` is null when no write completed.

Analyze recorded logs without calling a model:

```bash
node "$SKILL_DIR/scripts/perf.mjs" <target-repo> --command rename-bindings \
  --window-seconds 10 --slow-files 10
# --command selects any of the four commands (default: rename-bindings).
# Add --run-id <id> to select an earlier run; the default is the latest start.
```

The JSON report shows sends, completions, end-of-window and peak in-flight counts, mean
completed-attempt duration and 429 responses per window, phase times and slowest files.
It also reports model HTTP-attempt count and `names_per_request` median/mean (retries included).
It compares total `estimated_input_tokens` with server `input_tokens`, including retries;
server framing and schema accounting may differ from the input-text estimate.
An interrupted run uses its recorded boundaries; incomplete trailing JSONL records are ignored.
Do not run the live-model `scripts/e2e.mjs` unless the user requested it.
