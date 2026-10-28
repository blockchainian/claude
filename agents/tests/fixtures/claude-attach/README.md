# Claude attach fixtures

Claude Code 2.1.299, real temporary trusted git repositories, Haiku background sessions, and 200-column × 50-row attach PTYs:

- `idle.screen.txt` and `busy.screen.txt`: verbatim tmux screen captures. The empty input prompt contains a non-breaking space after `❯`.
- `interrupted.screen.txt`: Claude Code 2.1.302 tmux capture after Esc interrupted a two-line prompt before any reply and restored it to the input box; the working directory is shown as `~/project`.
- `attach.raw`: unmodified tmux terminal output across the capture attempts.
- `queued.screen.txt`: actual busy submission screen, including the input hint `Press up to edit queued messages`.
- `queued-command.json`: one actual transcript attachment delivered at a tool boundary after Enter accepted input while busy. Its attachment type is `queued_command`; its prompt and session ID identify the delivered input.
- `model-command.raw`: Claude Code 2.1.302 attach output from a temporary Haiku session after `/model claude-opus-5-5`. The header changes to Opus 5.5 and the editor clears without a `Switch model?` dialog. Only working directories are replaced with `/project` or `~/project`.
- `model-footer.screen.txt`: the editor and status rows from a read-only attach to session `c2345efd-183f-414b-a67a-98e0fd4587ad`, Claude Code 2.1.302. Its long transcript has scrolled the header off screen; the status row shows `Opus 5.5 medium`.
- `switch-model-dialog.screen.txt` and `change-effort-dialog.screen.txt`: tmux screen captures of `claude attach` on a Haiku background session, Claude Code 2.1.302, after `/model claude-opus-5-5` and `/effort high`. The dialog replaces the input box and starts with a `▔` border line (the effort one carries the effort label inside the border); chat text above stays on screen. The working directory is replaced with `/project`.
- `chat-text-with-dialog-markers.screen.txt`: the same session idle after replies that quote `Switch model?`, `Enter to confirm`, `Do you want to proceed?` and `Allow once` as chat text, with the normal input box.
