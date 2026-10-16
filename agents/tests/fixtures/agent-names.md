# Agent name records

`claude-agent-names.jsonl` and `codex-agent-names.jsonl` contain scrubbed records from local Claude Code and Codex transcripts. Message bodies, prompts, descriptions, names, and socket paths are replaced; unrelated metadata is omitted. The records retain the tool argument keys and message envelope attributes from the transcripts.

Tests derive Task aliases, named handbacks, empty arguments, and target-only lifecycle calls from these records.
