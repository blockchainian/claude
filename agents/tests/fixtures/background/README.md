Claude Code 2.1.296 samples copied from local session files on 2026-10-13:

- `records.jsonl`: Bash launch and structured background result for `birls4onj`.
- `quiet-turn.jsonl`: a later real enqueue, assistant reply, dequeue, user prompt and Bash call from the same session.
- `foreground.jsonl`: a real Bash call and result without `backgroundTaskId`.
- `running.output`: an empty output held open by a running task.
- `exited.output`, `killed.output`, `detached.output`: complete real output files with terminal footers.
- `adopt.json`: the shell adoption record for background session `50517e58`.

Tests relocate output paths and compose these records around the queue stopping point. Liveness observations are injected; the opt-in sleep probe uses actual descriptors and PIDs.
