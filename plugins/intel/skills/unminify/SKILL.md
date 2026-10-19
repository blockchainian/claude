---
name: unminify
description: Rename minified JavaScript identifiers with batched model suggestions and scope-safe rewrites. Use when asked to give short variables meaningful names in a JavaScript directory, or resume an interrupted unminify run. Formatting and syntax restructuring are outside this skill.
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

Install dependencies with `(cd "$SKILL_DIR/scripts" && npm ci)` if needed. Use the target
directory supplied by the user; it must be inside a Git repository. Check its Git status
before rewriting so existing work is visible.

```bash
node "$SKILL_DIR/scripts/cli.mjs" <dir> \
  --concurrency 20 --max-ids 60 --context-bytes 8000 --short-len 3
```

The model is fixed to `gpt-6-luna` with low reasoning effort. It reads the user's Codex login
from `$CODEX_HOME/auth.json`, defaulting to `~/.codex/auth.json`. Run `codex login` if required.
Never print credentials.

The command selects parseable `.js`/`.mjs` files, asks for names of short non-import bindings,
resolves naming conflicts, checks each completed file and writes it atomically. Top-level
batches finish before function batches. After all files finish, imports follow export names
without another model request. Public export names stay unchanged.

## Ignore and resume

Read the target Git root's `.ignore`, or supply `--ignore <file>`. Rules use `.gitignore`
syntax relative to the ignore file: directories, wildcards, `!` exceptions and whole-line
`#` comments; do not use trailing comments. The plugin supplies no project-specific ignore list.

Accepted replies are synced immediately to `.unminify/<relative-path>.jsonl` at the target
Git root. HTTP attempt logs are in `.unminify/requests.jsonl`. Rerun the same command and batch
options after interruption. Completed files are skipped; unfinished files reuse recorded
replies against the unchanged original source. Do not delete journals during a resumable run.
If source or batch options changed, stop and report the mismatch rather than discard records.
The user decides whether `.unminify/` should be ignored or committed.

On `USAGE_LIMIT`, the command stops issuing requests, drains running replies and exits
nonzero. Wait for quota reset, then rerun the same command. On `AUTH`, run `codex login`,
then rerun. A failed rewrite keeps the file intact and records the reason; report that reason.

## Report

Parse the JSON status lines and final summary. Report changed, skipped and rejected files,
request count, token totals and first-to-last quota percentage changes; if quota headers
were absent, report `没测到`. Inspect the Git diff and leave committing target source to the
caller. Do not run the live-model `scripts/e2e.mjs` unless the user requested it.
