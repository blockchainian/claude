---
description: Ask Grok to review the current diff or a branch diff.
argument-hint: "[--base <ref>] [focus]"
disable-model-invocation: true
allowed-tools: Bash(node:*)
---

!node "${CLAUDE_PLUGIN_ROOT}/scripts/grok-companion.mjs" review "$ARGUMENTS"
