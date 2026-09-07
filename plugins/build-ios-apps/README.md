# build-ios-apps

Drive iOS apps from Claude Code — build and run them on the simulator, walk the
UI, profile CPU, prove memory leaks, and capture whole app screens from a
connected iPhone.

## Skills

| Skill | Target | What it does |
|---|---|---|
| `ios-debugger-agent` | Simulator | Build, run and launch an app on a booted simulator via XcodeBuildMCP, drive the UI, capture logs |
| `ios-ettrace-performance` | Simulator | Capture symbolicated ETTrace flamegraphs for one focused flow and report the hot stacks |
| `ios-memgraph-leaks` | Simulator | Capture and compare `.memgraph` files to root-cause leaks with before/after evidence |
| `ios-take-screenshot` | **Real device** | Capture a whole app screen, including everything below the fold, as one stitched PNG |

Target matters when choosing a skill: the first three drive a booted simulator
through XcodeBuildMCP, while `ios-take-screenshot` drives a physical iPhone
through appium-mcp and does not work against a simulator.

The two profiling skills build on `ios-debugger-agent` for the build, launch,
and UI-driving steps.

## MCP servers

The plugin ships two servers, declared in `.mcp.json` and launched on demand.

`xcodebuildmcp`:

```
npx -y xcodebuildmcp@latest mcp
XCODEBUILDMCP_ENABLED_WORKFLOWS=simulator,simulator-management,ui-automation,debugging,device
```

That registers 59 tools. `session-management` is added by the server itself.
`project-discovery`, `utilities`, and `coverage` are deliberately absent: the
`simulator` workflow already re-lists their tools, so enabling them would add
only `get_mac_bundle_id`.

Tools are namespaced `mcp__plugin_build-ios-apps_xcodebuildmcp__*`.

`appium-mcp` drives a physical iPhone, which XcodeBuildMCP cannot do — its UI
automation is simulator-only:

```
npx -y appium-mcp@latest
NO_UI=true
```

Tools are namespaced `mcp__plugin_build-ios-apps_appium-mcp__*`. Device-specific
capabilities are not in `.mcp.json`, since they differ per machine; pass them
when creating a session, or set `CAPABILITIES_CONFIG` to a local file.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install build-ios-apps@blockchainian
```

## Requirements

- macOS with Xcode and the iOS Simulator installed
- `npx` on PATH
- `ettrace` for the profiling skill: `brew install emergetools/homebrew-tap/ettrace`
- for `ios-take-screenshot` against a real iPhone: an Apple Developer account,
  WebDriverAgent signed onto the device, and both Developer Mode and
  Settings -> Developer -> Enable UI Automation turned on

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
  `swiftui-view-refactor`) and the Codex-only `ios-simulator-browser` skill,
  keeping the three simulator-runtime skills.
- Corrected `ios-debugger-agent` against xcodebuildmcp 2.7.0: real tool names
  (`snapshot_ui`, `session_set_defaults`, `elementRef`-based `tap`/`type_text`),
  and log guidance matching the automatic runtime-log capture that
  `build_run_sim`/`launch_app_sim` provide. Upstream still documents
  `describe_ui` and `start_sim_log_cap`/`stop_sim_log_cap`, which no longer
  exist.
- Dropped `logging` from `XCODEBUILDMCP_ENABLED_WORKFLOWS`; xcodebuildmcp removed
  that workflow in 2.5.0 and the server now rejects it as unknown.
- Added the `simulator-management` and `device` workflows, which upstream does
  not enable: simulator location, appearance, statusbar, keyboard and erase
  control, plus build, install, launch, and test on physical devices.
