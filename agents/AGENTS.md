# AGENTS.md

> Read `README.md` before working here.  
> Install the binary after every change; the user runs the `agents` on PATH, not the source.

`agents` is the Rust terminal dashboard that lists every Claude Code and Codex session on this machine in the layout of `codex agents`.

## Modules

- src/app.rs -- the terminal loop: input, refresh and actions
- src/model.rs -- sessions as the dashboard sees them
- src/providers/ -- one reader per agent: `claude`, `codex`
- src/sessions/ -- merged snapshots and filesystem notifications
- src/log.rs -- task log events and filters
- src/ui/ -- rendering, input and the task log panel
- tests/fixtures/ -- real transcript and rollout payloads
- tests/ui_snapshots/ -- rendered screens the UI tests compare against

## Checks

- **tests**: `cargo test`
- **lint**: `cargo clippy --all-targets`
- **build**: `cargo build --release`

## Tests

- A rendering change updates the snapshots it moves in `tests/ui_snapshots/`.

## Tools

### Install

- `cargo install --path . --locked` — installs the built binary to `~/.cargo/bin/agents`; run it in the same turn as the commit.
