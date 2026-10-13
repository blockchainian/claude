# agents

`agents` shows every Claude Code session on this machine in the layout of `codex agents`: live sessions from the session registry, inactive ones from their transcripts, grouped by project, status or model, with the same hotkeys.

## Build and run

```sh
cargo build --release
./target/release/agents
```

## Sources

- `$CLAUDE_CONFIG_DIR` (default `~/.claude`) `/sessions/<pid>.json` — live sessions; an entry counts only when its pid is alive and its `procStart` matches the process.
- `$CLAUDE_CONFIG_DIR/projects/*/<session-id>.jsonl` — transcripts; read in chunks, with the full event history cached during Task Log search.
- `$CLAUDE_CONFIG_DIR/agents.json` — pins, grouping and an optional `theme`: a `.tmTheme` path whose accents color the session titles, as in Codex. Without it the titles use Catppuccin Latte on a light terminal and Mocha on a dark one, Codex's defaults.

Changes to the registry and transcripts refresh the dashboard after a 50 ms debounce. A full refresh every 5 seconds checks process liveness; unavailable filesystem notifications fall back to 1-second polling.

## Checks

```sh
cargo test
cargo clippy --all-targets
cargo build --release
```
