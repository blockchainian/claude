# build-ios-apps

Drive iOS apps on the simulator from Claude Code — build and run them, walk the
UI, profile CPU, and prove memory leaks.

## Skills

- `build-ios-apps:ios-debugger-agent` — build/run/launch an app on a booted
  simulator via XcodeBuildMCP, drive the UI, capture logs
- `build-ios-apps:ios-ettrace-performance` — capture symbolicated ETTrace
  flamegraphs for one focused flow and report the hot stacks
- `build-ios-apps:ios-memgraph-leaks` — capture and compare `.memgraph` files to
  root-cause leaks with before/after evidence

The two profiling skills build on `ios-debugger-agent` for the build, launch,
and UI-driving steps.

## MCP server

The plugin ships one server, `XcodeBuildMCP`, launched on demand:

```
npx -y xcodebuildmcp@latest mcp
XCODEBUILDMCP_ENABLED_WORKFLOWS=simulator,ui-automation,debugging,logging
```

Tools are namespaced `mcp__plugin_build-ios-apps_XcodeBuildMCP__*`.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install build-ios-apps@blockchainian
```

## Requirements

- macOS with Xcode and the iOS Simulator installed
- `npx` on PATH
- `ettrace` for the profiling skill: `brew install emergetools/homebrew-tap/ettrace`

## Attribution

Forked from OpenAI's `build-ios-apps` Codex plugin
(<https://github.com/openai/plugins>, MIT, Copyright (c) OpenAI).

Changes in this fork:

- Ported to the Claude Code plugin format: manifest moved from
  `.codex-plugin/plugin.json` to `.claude-plugin/plugin.json`, MCP config
  inlined under `mcpServers`, and the Codex-only `agents/openai.yaml`
  display-name files dropped.
- Dropped the five Swift-authoring skills (`ios-app-intents`,
  `swiftui-liquid-glass`, `swiftui-performance-audit`, `swiftui-ui-patterns`,
  `swiftui-view-refactor`), keeping the three simulator-runtime skills.
