# Dialog detection by screen structure

- attach.rs: `dialog(screen)` returns the block below the last `▔` line when Claude's input box is absent and a title line exists. `known_dialog`, the `wait_allowing` allow-check, and the SetModel/SetEffort dialog checks (`dialog_shows`) use only that block. The error now carries the dialog title and body.
- Fixtures (live capture, Claude Code 2.1.302, `claude attach` on a disposable Haiku session in a throwaway repo, path scrubbed): switch-model-dialog, change-effort-dialog, chat-text-with-dialog-markers.
- Finding: the effort dialog border line carries text (`▔▔▔ ◐ medium · /effort ▔`), so the border is detected by its first char, not by being all `▔`.
- Finding: model-command.raw has transient frames with a `▔` line and nothing below; the title requirement makes them not-a-dialog.
- Unverified: permission ("Allow once", "Do you want to proceed?"), sign-in prompts in attach mode. Trust prompt captured in plain `claude` uses a `─` top rule, and its text is "Quick safety check", not "Do you trust"; `claude --bg` refuses untrusted workspaces so attach never sees it. These prompts are only detected if they use a `▔` border in attach; otherwise the stage waits to the timeout instead of failing fast.
- Tests: full `cargo test` in agents/ green (2002 passed). Clippy -D warnings on --all-targets has 12 pre-existing errors in other files; none left in attach.rs.
