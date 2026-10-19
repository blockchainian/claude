# AGENTS.md

> The user runs the installed `agents`, never the source.

This directory is `agents`, the Rust terminal dashboard for Claude Code and Codex sessions, with prompt input and Claude background task creation.

## Modules

- src/app.rs -- the terminal loop: input, refresh and actions
- src/model.rs -- sessions as the dashboard sees them
- src/providers/ -- session readers and controls: `claude`, `codex`
- src/sessions/ -- merged snapshots and filesystem notifications
- src/processes.rs -- running processes and their open files, read through libproc
- src/log.rs -- task log events and filters
- src/usage.rs -- a transcript's token usage in time bins, for the usage chart
- src/activity.rs -- when a transcript's turns ran, for the activity strip
- src/jsonl.rs -- a growing transcript read by complete lines, forward or backward
- src/tasks.rs -- task worktrees, environment files and workspace trust
- src/ui/ -- rendering, prompt input, drafts and the task log panel
- tests/ -- real payload fixtures and rendered UI snapshots

## Development

- A rendering change updates the snapshots it moves in `tests/ui_snapshots/`.

## Tools

### Install

- `cargo install --path . --locked` — builds and installs the current source

## Checks

The gate every change passes:

- **tests**: `cargo nextest run`
- **lint**: `cargo clippy --all-targets`
- **build**: `cargo build --release`

## Tests

> Run coverage tests only when you need coverage.

- Run the relevant tests.
- **coverage tests**: `cargo llvm-cov nextest --no-fail-fast --html`

## Version control

- Commit and push, then install the binary in the same turn.
