# web

Inspect web apps from Claude Code. Today: find memory leaks by diffing V8 heap
snapshots captured from a running Chrome over the DevTools protocol.

## Skills

| Skill | What it does |
|---|---|
| `find-leaks` | Capture two heap snapshots around a repeated action and diff them into a ranked report of the constructors that grew and the DOM nodes left detached |

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

## Install

```
/plugin marketplace add blockchainian/claude
/plugin install web@blockchainian
```

## Requirements

- Google Chrome, started with `--remote-debugging-port` (any recent version)
- `uv` on PATH

## Related

Design-matching a built screen against a reference is the platform-neutral
`check-design` skill in the `mobile` plugin; it works for web captures too.
