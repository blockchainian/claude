# agent-command-center

`claude-agents` shows every Claude Code session on this machine in the layout of `codex agents`: live sessions from the session registry, inactive ones from their transcripts, grouped by project, status or model, with the same hotkeys.

## Build and run

```sh
cargo build --release
./target/release/claude-agents
```

## Sources

- `$CLAUDE_CONFIG_DIR` (default `~/.claude`) `/sessions/<pid>.json` — live sessions; an entry counts only when its pid is alive and its `procStart` matches the process.
- `$CLAUDE_CONFIG_DIR/projects/*/<session-id>.jsonl` — transcripts; read in head and tail chunks only.
- `$CLAUDE_CONFIG_DIR/agent-command-center.json` — pins, grouping and an optional `theme`: a `.tmTheme` path whose accents color the session titles, as in Codex. Without it the titles use Catppuccin Latte on a light terminal and Mocha on a dark one, Codex's defaults.

## Checks

```sh
cargo test
```
