# web

Inspect and drive web apps from Claude Code or Codex: run a persistent headless
Chromium from the shell, find memory leaks by diffing V8 heap snapshots, and check
a built page against a design reference.

## Skills

| Skill | What it does |
|---|---|
| `browse` | Drive a persistent headless Chromium with the `browse` CLI — navigate, interact, run JS, read console and network logs, screenshot — with one tab per client for parallel runs |
| `heap-snapshot-leaks` | Capture two heap snapshots around a repeated action and diff them into a ranked report of the constructors that grew and the DOM nodes left detached |
| `check-web-design` | Diff a built web page against a design reference and report the off-by colours, positions, and missing/extra elements |

## How it works

`browse` is [gstack](https://github.com/garrytan/gstack)'s browser CLI (MIT),
vendored byte-identical at a pinned commit under `skills/browse/vendor/gstack/`.
This plugin's changes to it are patch files in `skills/browse/patches/`.
`bin/browse` is a launcher Claude Code puts on the Bash tool's PATH: it computes a
build id from the vendor tree, the patches and the build script, builds on first use through
`skills/browse/scripts/build.sh` (copy the vendor tree, apply the patches, install,
compile) into `$CLAUDE_PLUGIN_DATA/browse/<build id>/`, and then execs the built
CLI. Concurrent first calls build once; build output goes to stderr. The patch scopes `console` and `network` to
the tab a command is pinned to (`BROWSE_TAB`), so parallel clients sharing the
daemon each read only their own tab's logs. `skills/browse/UPSTREAM.md` lists
what was vendored, what was left out, and how to upgrade.

Two Node scripts (no npm dependencies; the capture script needs Node 22+ for the built-in `WebSocket`):

- `capture-heap-snapshot.mjs` connects to a Chrome started with
  `--remote-debugging-port`, picks a tab, forces a GC, and streams a
  `HeapProfiler.takeHeapSnapshot` to a `.heapsnapshot` file. It sends no `Origin`
  header, so `--remote-allow-origins` is not required.
- `diff-heap-snapshots.mjs` parses two snapshots — no browser — and reports
  per-constructor count and retained-byte growth, detached DOM nodes, and the
  leak suspects.

The comparison is what carries the signal: a leak is a constructor whose live
count and bytes climb with each repeat of an action, or DOM nodes the page still
holds after they left the document.

`check-web-design` diffs a browser screenshot (actual) against a design mock
(desired). Its `check_design.py` engine is platform-neutral and is shared,
byte-identical, with the `mobile` plugin's `check-mobile-design`; a repo test
fails if the two copies ever drift. The web SKILL.md carries the browser capture
recipe (screenshot + DOM `getBoundingClientRect`).

Maintainer note: the two SKILL.md files also mirror each other in the sections
"Reading `diff.json`", "Tolerances" and "Requirements", which differ only in the
platform nouns (element/view, page/screen). No test covers the prose — change
both copies together.

## Install

For Codex installation, see
[Codex installation](../../README.md#codex).

In Claude Code:

```
/plugin marketplace add blockchainian/claude
/plugin install web@blockchainian
```

## Requirements

- `bun` on PATH to build `browse`, and a Playwright Chromium for it
- `uv` on PATH (for `check-web-design`; its script declares its own dependencies)
- Node 22+ for `heap-snapshot-leaks` (no npm install needed)
- for `heap-snapshot-leaks`: Google Chrome, started with `--remote-debugging-port`

## Tests

```
node --test skills/heap-snapshot-leaks/tests/*.mjs
node --test skills/browse/tests/*.mjs
uv run skills/check-web-design/tests/test_check_design.py
```

Or from the marketplace root: `npm run test:web`. Tests that open a visible
Chromium window are skipped unless `BROWSE_HEADED_TESTS=1`.

## Environment Variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `WEB_DATA_DIR` | Root for heap-snapshot-leaks' throwaway Chrome profile; default ~/.local/share/web | Optional | Shell environment |
| `BROWSER_DISPLAY` | Display for browse's headed windows (any part of its name, any case; empty means the main display) | Optional | ~/.config/web/.env |
| `CLAUDE_PLUGIN_DATA` | Root for `bin/browse`'s builds; default ~/.claude/plugins/data/web-blockchainian | Optional | Claude Code, or shell environment |

Application-specific variables belong to the target project, not this plugin.
