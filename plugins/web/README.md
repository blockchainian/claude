# web

Inspect web apps from Claude Code: find memory leaks by diffing V8 heap
snapshots, and check a built page against a design reference.

## Skills

| Skill | What it does |
|---|---|
| `heap-snapshot-leaks` | Capture two heap snapshots around a repeated action and diff them into a ranked report of the constructors that grew and the DOM nodes left detached |
| `check-web-design` | Diff a built web page against a design reference and report the off-by colours, positions, and missing/extra elements |

## How it works

Two `uv run` scripts, each declaring its own dependencies:

- `capture_heap_snapshot.py` connects to a Chrome started with
  `--remote-debugging-port`, picks a tab, forces a GC, and streams a
  `HeapProfiler.takeHeapSnapshot` to a `.heapsnapshot` file. It sends no `Origin`
  header, so `--remote-allow-origins` is not required.
- `diff_heap_snapshots.py` parses two snapshots — no browser — and reports
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

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install web@blockchainian
```

## Requirements

- `uv` on PATH (both skills; the scripts declare their own dependencies)
- for `heap-snapshot-leaks`: Google Chrome, started with `--remote-debugging-port`
