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
- Selection follows a top-level task from Inactive to Active, but stays on Active when it becomes Inactive, including while renaming; a child leaving the tab selects its visible parent, otherwise the usual fallback.
- Tabs start on Active; saved fields for removed status tabs are ignored.

## Panel inputs

- Every one-line input in a panel (task search, log search, log detail search and a new task's branch) takes its panel's bottom row and draws through `render_input_row`: a cyan label, the plain text and the terminal cursor, with no row highlight.

## Details panel

- The status and elapsed time sit on the rule above the panel; the task's name sits centered on the rule below, between the pull request or a new task's branch on the left and the project folder on the right, and double-clicking it edits the name in place on that rule. The task list's `r` key opens the same editor. Enter saves a nonempty name; Esc keeps the original name.
- Sections run Last activity, Background, Update, Prompt and its queued prompts, Needs input, then Tasks.
- The assistant reply section is `Update`; double-click its body to copy it with the `Copied update` toast.
- The activity section is `Last activity`.

## New task

- `n` opens the name editor on the details panel's lower rule. A nonempty name starts the wizard: repository, branch, then prompt. The draft is a bag of fields; every wizard transition opens its first missing field. Only Enter with prompt text can create a task. Tasks require a Git working tree and an explicit branch choice; choosing the checked-out branch runs in that checkout without a worktree. A non-Git default directory stays unconfirmed and opens the repository browser; plain folders can be navigated but cannot be confirmed.
- Repository, branch, model and effort border controls and a title double-click edit one field, return to the unnamed draft's name editor or the named draft's prompt, and never create. A repository picked before naming is already confirmed when the wizard continues. Changing repository primary roots clears the branch choice; selecting the same repository keeps it. Prompt text and images survive every selector.
- A named draft is an Active task row with status `new`, between Needs input and Working. Selecting another row, changing tabs, an attention jump or quitting preserves it; selecting it restores its prompt and focuses the prompt box. Unnamed drafts are modal and discarded on leave. Draft rows support deletion through the existing confirmation dialog; provider task operations are unsupported.
- Named drafts are saved in `agents.json` under `state.drafts` on field changes, leave and quit. Entries load independently; invalid entries do not discard other preferences. Image attachments keep their existing files and restore by path; missing images are dropped. Loading repairs missing directories and branches and interrupted creation. Errors stay in the details panel until the next wizard transition.
- Esc and right-click close one level: new-branch typing, selector, name edit, then deletion confirmation. Cancelling a named draft's name edit keeps its name; cancelling its initial name abandons it. With nothing open, Esc on a named draft confirms deletion. Creation freezes controls internally while its row stays `new`; failures restore editing with the prompt intact, and the arriving session replaces the draft and becomes selected.

## Log panels

- The last clicked panel owns keyboard focus, even if it accepts no keys; clicking the prompt returns focus to its input.
- Esc in the prompt input clears its text; on an empty input it stops the task's running turn, the same as `x`, and once the stop lands returns that turn's prompt to the input. In a new task, Esc follows the draft cancellation rules above.

- The log list selects one event, oldest first, newest at the bottom; the detail shows that event.
- Selected rows use the task cursor style across the column, including both padding cells.
- A list wheel step selects one adjacent event and keeps it visible; clicking a row selects it immediately. In the detail panel, wheel and navigation keys scroll and unfold sections before moving the log selection at a directional boundary; returning to an unchanged event restores its scroll state.
- Selecting the newest event follows arriving events. Older selections stay on their event and show the border's New button only when new events arrive; clicking it selects newest and follows again.
- Switching tasks or sending a prompt resets the log selection to newest and resumes following.
- Double click, text selection and list/detail search remain independent of following.
- Log details are a sequence of sections, one per message or tool call, with no merged event title. Every section starts with a plain title row that scrolls with its body; sections have one blank row between them. Tool titles put the subagent type (or tool display name) on the left and description, name or tool title in the center; task notifications use `Task <id>` on the left. Message titles put the sender on the left and kind in the center. Single-tool and message sections use the event time; merged tool sections use each call’s duration. Subagent types, names and descriptions appear only in titles.
- Log detail times share the list’s time-column geometry at the panel’s right edge. Title rows place details on the left, the title on the panel midline and the time in its column; titles move left to clear the time column and truncate only when wider than the available space. Body groups stay left of the time column and use the same panel-midline placement: prose wraps at the reading width, while preformatted lines use the full available width and truncate with an ellipsis. List rows also center on the panel midline, moving left to clear the time column.
- Shell blocks containing `&&` chains draw a two-cell display prefix on every line: `┌ `, `│ ` and `└ ` inside chains, two spaces outside. Blocks without chains have no prefix. Prefixes count toward group width and truncation, but have no selectable or copied bytes.

## Session terminal

- Opening a session terminal shows the running dots without a cursor until a published frame has visible text and the provider considers it ready, with a two-second limit.

- `o`, or a right click on the log detail, runs the selected session's program in the log detail panel: `codex resume` for Codex, `claude attach` for a Claude background job, and for a Claude session with no live process `claude --resume <id> --bg` followed by `claude attach`. A Claude session running in another terminal shows `Running in a terminal · /bg` instead.
- A right click on the terminal switches back to the log detail; the dashboard always keeps the right button. Switching back, selecting another task, quitting or the program exiting closes the terminal, and returning to the task shows the log detail: Codex clients are ended, since the app-server owns the turn, and Claude attach clients detach with Ctrl-Z. Client cleanup runs in the background; reopening the same session waits for it, and quitting waits for all clients to be reaped.
- The focused terminal takes every key and paste. A left click on another panel moves focus away; a left click on the terminal moves it back. Every mouse event over the terminal except the right button goes to the program as far as its mouse reporting asks, and is dropped otherwise.
- While a Claude session's terminal is open, prompts and interrupts from the prompt box are typed into it instead of opening a second attach client.

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
