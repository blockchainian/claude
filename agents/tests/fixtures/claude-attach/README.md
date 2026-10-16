# Claude attach fixtures

Claude Code 2.1.299, real temporary trusted git repositories, Haiku background sessions, and 200-column × 50-row attach PTYs:

- `idle.screen.txt` and `busy.screen.txt`: verbatim tmux screen captures. The empty input prompt contains a non-breaking space after `❯`.
- `attach.raw`: unmodified tmux terminal output across the capture attempts.
- `queued.screen.txt`: actual busy submission screen, including the input hint `Press up to edit queued messages`.
- `queued-command.json`: one actual transcript attachment delivered at a tool boundary after Enter accepted input while busy. Its attachment type is `queued_command`; its prompt and session ID identify the delivered input.

The ignored `real_attach_prompt_paste_busy_steer_interrupt_model` test exercises multiline bracketed paste, busy input acceptance, queued-command delivery in the same session, interrupt, and model changes with global defaults restored. Every background job it starts is removed; both global configuration files are restored byte-for-byte, including original absence.
