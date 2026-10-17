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

## Task tabs

- `Active` lists every non-inactive status; `Inactive` lists inactive tasks. Tab and Shift-Tab cycle the two tabs.
- Each provider initially loads its ten most recently updated inactive tasks; `Show more` loads another page per provider. Tab counts include only loaded tasks.
- Selection follows a top-level task to Inactive; a child leaving the tab selects its visible parent, otherwise the usual fallback.
- Tabs start on Active; saved fields for removed status tabs are ignored.

## Details panel

- The assistant reply section is `Update`; double-click its body to copy it with the `Copied update` toast.
- The activity section is `Last activity`.

## Log panels

- The last clicked panel owns keyboard focus, even if it accepts no keys; clicking the prompt returns focus to its input.

- The log list selects one event, oldest first, newest at the bottom; the detail shows that event.
- Selected rows use the task cursor style across the column, including both padding cells.
- A list wheel step selects one adjacent event and keeps it visible; clicking a row selects it immediately. The detail wheel scrolls only its content.
- Selecting the newest event follows arriving events. Older selections stay on their event and show the border's Latest/New jump; clicking it selects newest and follows again.
- Switching tasks resets the log selection to newest and resumes following.
- Double click, text selection and list/detail search remain independent of following.

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
