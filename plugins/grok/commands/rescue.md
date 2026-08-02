---
description: Delegate a coding task to Grok through the grok-rescue subagent.
argument-hint: <task>
allowed-tools: Agent
---

Use the Agent tool with `subagent_type` set to `grok-rescue` and pass this task exactly:

```
$ARGUMENTS
```

Return the subagent result directly. Do not summarize command output that the subagent marks as verbatim.

