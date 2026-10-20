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

Before planning any names, the command restores numeric `void` expressions and minified
booleans in every selected file. Only converted source ranges are replaced; surrounding
text stays byte-identical. Fragment files retain their program bindings and declaration
layout. Their numeric `void` expressions are restored only when the Git-root binding
index (or the supplied input set outside Git) contains no program binding named
`undefined`, and the current file passes binding and dynamic-scope checks. Direct
API calls without index context retain numeric `void` in fragments.

The command selects parseable `.js`/`.mjs` files, asks for names of non-import bindings shorter than `--min-symbol-length`,
resolves naming conflicts, checks each completed file and writes it atomically. Top-level
batches finish before function batches. Within each file and stage, pending batches are greedily
packed in order within `--max-input-tokens` (default 100000), with no name-count cap.
Planning and packing count the actual instructions, prompt and rendered context using
`js-tiktoken` 1.0.21 with `o200k_base`. Oversized contexts are shortened; an irreducible
batch that still cannot fit fails before sending. Placeholders are unique across
the request; replies and resume records stay keyed by the original batch IDs.
At the end of every run, including interrupted runs, imports in journal-completed files
follow export names without another model request. Unfinished files supply their current
export names but remain untouched so their journals can resume. Public export names stay unchanged.

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
