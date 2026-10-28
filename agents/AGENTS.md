# AGENTS.md

> The user runs the installed `agents`, never the source.
> Re-run a slow test terminated due to timeout separately. Fix it if it still fails.

This directory is `agents`, the terminal command center for Claude Code and Codex sessions.

## Modules

- src/app.rs -- the terminal loop: input, refresh and actions
- src/model.rs -- sessions as the dashboard sees them
- src/providers/ -- session readers and controls: `claude`, `codex`
- src/sessions/ -- merged snapshots and filesystem notifications
- src/processes.rs -- running processes and their open files, read through libproc
- src/event.rs -- task events and filters
- src/usage.rs -- a transcript's token usage in time bins, for the usage chart
- src/activity.rs -- transcript turn membership for the event list
- src/jsonl.rs -- a growing transcript read by complete lines, forward or backward
- src/tasks.rs -- task worktrees, environment files and workspace trust
- src/line_edit/ -- the readline-style line editor: commands, keymap, kill buffer, layout and placeholders
- src/ui/ -- rendering, prompt input, drafts and the task event panels
- tests/ -- real payload fixtures and rendered UI snapshots

## Development

### Writing tests

> Do not write UI tests. Write unit tests instead.

Each test should complete in under 1s, test one product behavior only, and use minimal test data or fixtures only. Do not:

- test multiple unrelated product behaviors in one test
- use unnecessarily large test data or fixtures
- use combinatorial tests, i.e. all possible combinations of N behaviors that can be tested in O(n) if done separately.

### Fixing slow tests

Change test code only, and keep every assertion's coverage without faking the flow. Make a slow test faster by:

- splitting unrelated behaviors, unless the parts stay slow or the total time grows
- cutting redundant records and rounds
- covering each dimension once instead of every combination
- removing unneeded waits and sleeps

Time the test alone with `cargo nextest run --bin agents -E 'test(=<path>)'` before and after the change. Never relax the nextest timeout.

## Tools

### Install

- `cargo install --path . --locked` — builds and installs the current source

## Checks

The gate every change passes:

- **lint**: `cargo clippy --all-targets`
- **tests**: `cargo nextest run`
- **build**: `cargo build --release`

## Tests

> Run coverage tests only when you need coverage.

- Run the relevant tests.
- **coverage tests**: `cargo llvm-cov nextest --no-fail-fast --html`

## Version control

- Commit and push, then install the binary in the same turn.
