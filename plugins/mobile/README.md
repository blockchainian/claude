# mobile

Drive iOS apps from Claude Code — build and run them on the simulator, walk the
UI, profile CPU, prove memory leaks, and capture whole app screens from a
connected iPhone.

## Skills

| Skill | Target | What it does |
|---|---|---|
| `ios-debugger-agent` | Simulator | Build, run and launch an app on a booted simulator via XcodeBuildMCP, drive the UI, capture logs |
| `ios-ettrace-performance` | Simulator | Capture symbolicated ETTrace flamegraphs for one focused flow and report the hot stacks |
| `ios-memgraph-leaks` | Simulator | Capture and compare `.memgraph` files to root-cause leaks with before/after evidence |
| `ios-take-screenshot` | Simulator or real device | Capture a whole app screen, including everything below the fold, as one stitched PNG |
| `check-mobile-design` | Simulator or real device | Diff a built iOS screen against a design reference and report the off-by colours, positions, and missing/extra views |

Target matters when choosing a skill: the first three drive a booted simulator
through XcodeBuildMCP. `ios-take-screenshot` drives either, XcodeBuildMCP for a
simulator and appium-mcp for a phone, and picks by what the request asks for.

The two profiling skills build on `ios-debugger-agent` for the build, launch,
and UI-driving steps.

`check-mobile-design` shares its `check_design.py` engine byte-identically with
the `web` plugin's `check-web-design`, and a repo test fails if the two copies
drift. Maintainer note: the two SKILL.md files also mirror each other in the
sections "Reading `diff.json`", "Tolerances" and "Requirements", which differ
only in the platform nouns (view/element, screen/page). No test covers the
prose — change both copies together.

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

Tools are namespaced `mcp__plugin_mobile_xcodebuildmcp__*`.

`appium-mcp` drives a physical iPhone, which XcodeBuildMCP cannot do — its UI
automation is simulator-only:

```
npx -y appium-mcp@latest
NO_UI=true
```

Tools are namespaced `mcp__plugin_mobile_appium-mcp__*`. The plugin's
`phone-session-gate` hook denies `appium_session_management` `create` unless the phone is
claimed through `ios-take-screenshot`'s claim script and no session is open on it, since
WebDriverAgent serves one session and a second create ends the first. Device-specific
capabilities are not in `.mcp.json`, since they differ per machine; pass them
when creating a session, or set `CAPABILITIES_CONFIG` to a local file.

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install mobile@blockchainian
```

## Requirements

- macOS with Xcode and the iOS Simulator installed
- `npx` on PATH
- `ettrace` for the profiling skill: `brew install emergetools/homebrew-tap/ettrace`
- for `ios-take-screenshot` against a simulator: nothing beyond a booted simulator
- for `ios-take-screenshot` against a real iPhone: an Apple Developer account,
  WebDriverAgent signed onto the device, and both Developer Mode and
  Settings -> Developer -> Enable UI Automation turned on

## Tests

```
python3 skills/ios-take-screenshot/scripts/test_claim_simulator.py
python3 skills/ios-take-screenshot/scripts/test_discover_ios_setup.py
python3 skills/ios-take-screenshot/scripts/test_frame_diff.py
python3 skills/ios-take-screenshot/scripts/test_stitch_screens.py
```

`test_claim_simulator.py` covers the simulator claim: one run holds a simulator, a
second claim is refused with the holder named, only the holder releases, and capture
refuses `booted`, an unclaimed simulator, and one held by another run.
`test_discover_ios_setup.py` covers the simulator report: the pick is named in full, a
shutdown UDID is reported as such, and two booted simulators ask for `--device`.

`test_frame_diff.py` covers the frame comparison: a known scroll offset must be reported as
scrolled, with and without the chrome flags, a static frame carrying a changed value must
not, and frames of different sizes are refused.

`test_stitch_screens.py` builds synthetic pages and asserts the stitch reconstructs them row
for row: a vertical page with fixed chrome on both edges, including a pinned header carrying
a live-updating value; the same page rotated, for the horizontal axis; a page whose large
navigation title collapses after the first slice; and a still screen holding one
sideways-scrolling carousel, which must be found and cropped to; two identical slices must
be refused. Run it after any change to the stitcher.

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
