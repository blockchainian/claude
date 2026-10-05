---
name: ios-debugger-agent
description: Build, run, and debug iOS apps on Simulator with XcodeBuildMCP. Use when launching an app, inspecting simulator UI or logs, or diagnosing runtime behavior.
---

# iOS Debugger Agent

## Overview
Use XcodeBuildMCP to build and run the current project scheme on a booted iOS simulator, interact with the UI, and read captured logs. Prefer the MCP tools for simulator control, logs, and view inspection.

Tool names below are unprefixed. Discover the tools exposed by the `xcodebuildmcp`
server through the current host's tool inventory or tool search, then call their
actual registered names. Claude Code and Codex use different namespace prefixes;
do not construct one. If a required tool is unavailable, report it before proceeding.

## Core Workflow
Follow this sequence unless the user asks for a narrower action.

### 1) Discover the booted simulator
- Call `list_sims` and select the simulator with state `Booted`.
- If none are booted, boot one with `boot_sim`.

### 2) Set session defaults
- Call `session_set_defaults` with:
  - `projectPath` or `workspacePath` (whichever the repo uses)
  - `scheme` for the current app
  - `simulatorId` from the booted device
  - Optional: `configuration: "Debug"`, `useLatestOS: true`
- Then call `session_show_defaults` once per session, before the first build or run, and check that the project/workspace, scheme and `simulatorId` are the ones you set. The defaults belong to the XcodeBuildMCP server, which every agent in this session shares, so they can already hold another project or simulator, and nothing warns you.

### 3) Build + run (when requested)
- Call `build_run_sim`.
- **If the build fails**, check the error output and retry (optionally with `preferXcodebuild: true` in the session defaults) or escalate to the user before attempting any UI interaction.
- **After a successful build**, verify the app launched by calling `snapshot_ui` or `screenshot` before proceeding to UI interaction.
- If the app is already built and only launch is requested, use `launch_app_sim`.
- If bundle id is unknown:
  1) `get_sim_app_path`
  2) `get_app_bundle_id`

## UI Interaction & Debugging
UI actions target an `elementRef` from the latest snapshot, not coordinates or labels.

- **Snapshot**: `snapshot_ui` before tapping or typing. It returns `elementRef` targets and lists which actions each one supports.
- **Tap**: `tap` with an `elementRef` that lists `tap` in its snapshot targets.
- **Type**: `type_text` with the field's `elementRef`; set `replaceExisting` to overwrite contents.
- **Batch**: `batch` for several taps on one screen that need no assertion between them.
- **Gestures**: `gesture` with a `preset` for scrolls and edge swipes.
- **Screenshot**: `screenshot` for visual confirmation.

Re-snapshot after navigation, scrolling, or a sheet change — refs from a stale snapshot will not resolve.

## Logs & Console Output
`build_run_sim` and `launch_app_sim` capture runtime logs automatically and return the log file path in their response. Read that file directly and summarize the important lines; there is no separate start/stop log-capture tool.

## Reporting
- Say which simulator and scheme were used.
- Summarize build result, launch result, and any UI steps taken.
- Quote the relevant log lines rather than the whole capture.
