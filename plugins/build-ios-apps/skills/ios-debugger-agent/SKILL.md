---
name: ios-debugger-agent
description: Build, run, and debug iOS apps on Simulator with XcodeBuildMCP. Use when launching an app, inspecting simulator UI or logs, or diagnosing runtime behavior.
---

# iOS Debugger Agent

## Overview
Use XcodeBuildMCP to build and run the current project scheme on a booted iOS simulator, interact with the UI, and read captured logs. Prefer the MCP tools for simulator control, logs, and view inspection.

Tools are namespaced by the plugin. Every name below is the full tool name.

## Core Workflow
Follow this sequence unless the user asks for a narrower action.

### 1) Discover the booted simulator
- Call `mcp__plugin_build-ios-apps_xcodebuildmcp__list_sims` and select the simulator with state `Booted`.
- If none are booted, ask the user to boot one (do not boot automatically unless asked).

### 2) Set session defaults
- Call `mcp__plugin_build-ios-apps_xcodebuildmcp__session_set_defaults` with:
  - `projectPath` or `workspacePath` (whichever the repo uses)
  - `scheme` for the current app
  - `simulatorId` from the booted device
  - Optional: `configuration: "Debug"`, `useLatestOS: true`

### 3) Build + run (when requested)
- Call `mcp__plugin_build-ios-apps_xcodebuildmcp__build_run_sim`.
- **If the build fails**, check the error output and retry (optionally with `preferXcodebuild: true` in the session defaults) or escalate to the user before attempting any UI interaction.
- **After a successful build**, verify the app launched by calling `mcp__plugin_build-ios-apps_xcodebuildmcp__snapshot_ui` or `mcp__plugin_build-ios-apps_xcodebuildmcp__screenshot` before proceeding to UI interaction.
- If the app is already built and only launch is requested, use `mcp__plugin_build-ios-apps_xcodebuildmcp__launch_app_sim`.
- If bundle id is unknown:
  1) `mcp__plugin_build-ios-apps_xcodebuildmcp__get_sim_app_path`
  2) `mcp__plugin_build-ios-apps_xcodebuildmcp__get_app_bundle_id`

## UI Interaction & Debugging
UI actions target an `elementRef` from the latest snapshot, not coordinates or labels.

- **Snapshot**: `mcp__plugin_build-ios-apps_xcodebuildmcp__snapshot_ui` before tapping or typing. It returns `elementRef` targets and lists which actions each one supports.
- **Tap**: `mcp__plugin_build-ios-apps_xcodebuildmcp__tap` with an `elementRef` that lists `tap` in its snapshot targets.
- **Type**: `mcp__plugin_build-ios-apps_xcodebuildmcp__type_text` with the field's `elementRef`; set `replaceExisting` to overwrite contents.
- **Batch**: `mcp__plugin_build-ios-apps_xcodebuildmcp__batch` for several taps on one screen that need no assertion between them.
- **Gestures**: `mcp__plugin_build-ios-apps_xcodebuildmcp__gesture` with a `preset` for scrolls and edge swipes.
- **Screenshot**: `mcp__plugin_build-ios-apps_xcodebuildmcp__screenshot` for visual confirmation.

Re-snapshot after navigation, scrolling, or a sheet change — refs from a stale snapshot will not resolve.

## Logs & Console Output
`build_run_sim` and `launch_app_sim` capture runtime logs automatically and return the log file path in their response. Read that file directly and summarize the important lines; there is no separate start/stop log-capture tool.

## Reporting
- Say which simulator and scheme were used.
- Summarize build result, launch result, and any UI steps taken.
- Quote the relevant log lines rather than the whole capture.
