# AGENTS.md

> Before working here, read `README.md`.  
> The user runs the installed `agents`, never the source.

This directory is `agents`, the Rust terminal dashboard that lists every Claude Code and Codex session on this machine in the layout of `codex agents`.

## Modules

- src/app.rs -- the terminal loop: input, refresh and actions
- src/model.rs -- sessions as the dashboard sees them
- src/providers/ -- one reader per agent: `claude`, `codex`
- src/sessions/ -- merged snapshots and filesystem notifications
- src/log.rs -- task log events and filters
- src/ui/ -- rendering, input and the task log panel
- tests/ -- real payload fixtures and rendered UI snapshots

## Development

- A rendering change updates the snapshots it moves in `tests/ui_snapshots/`.

## Checks

The gate every change passes:

- **tests**: `cargo test`
- **lint**: `cargo clippy --all-targets`
- **build**: `cargo build --release`

## Tests

- Run the relevant tests.

## Version control

- Commit and push, then install the binary in the same turn.

## Tools

### Install

> Install into `~/.cargo/bin`, the `agents` on PATH.

- `cargo install --path . --locked` — builds and installs the current source
