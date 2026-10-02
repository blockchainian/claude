# web

Inspect web apps from Claude Code: find memory leaks by diffing V8 heap
snapshots, and check a built page against a design reference.

## Skills

| Skill | What it does |
|---|---|
| `heap-snapshot-leaks` | Capture two heap snapshots around a repeated action and diff them into a ranked report of the constructors that grew and the DOM nodes left detached |
| `check-web-design` | Diff a built web page against a design reference and report the off-by colours, positions, and missing/extra elements |

## How it works

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

```
/plugin marketplace add blockchainian/claude
/plugin install web@blockchainian
```

## Requirements

- `uv` on PATH (for `check-web-design`; its script declares its own dependencies)
- Node 22+ for `heap-snapshot-leaks` (no npm install needed)
- for `heap-snapshot-leaks`: Google Chrome, started with `--remote-debugging-port`

## Tests

```
node --test skills/heap-snapshot-leaks/tests/*.mjs
uv run skills/check-web-design/tests/test_check_design.py
```

Or from the marketplace root: `npm run test:web`.
