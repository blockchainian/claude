# Rust heredoc fixture

`claude-rust-heredoc.jsonl` is one complete assistant record from a real Claude transcript.

- Session: `f8366c3f-29b3-47b2-8e0e-f1a163231eb6`
- Transcript line: 617
- Tool: `Bash`
- Command: `cat > src/main.rs <<'EOF'` inside a dprint benchmark invocation

Tests parse its recorded heredoc body. Comment and syntax-error cases prepend text to that same body.
