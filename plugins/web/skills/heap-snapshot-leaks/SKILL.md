---
name: heap-snapshot-leaks
description: Find memory leaks in a running web app by diffing V8 heap snapshots taken around a repeated action. Use when a page's memory grows over time, a single-page app slows the longer it runs, or you suspect detached DOM nodes, dangling listeners, or retained components after navigating away. Captures snapshots from Chrome over the DevTools protocol and reports which constructors grew and what stayed detached.
---

# Heap Snapshot Leaks

A web leak is what the heap keeps that a clean baseline did not: a constructor
whose live instance count and retained bytes climb with each repeat of an
action, or DOM nodes the page still references after they left the document
("detached"). This skill captures two heap snapshots around that action and
diffs them into a ranked report of the suspects.

Two scripts, both `uv run` (they declare their own deps):
- `scripts/capture_heap_snapshot.py` — pulls one `.heapsnapshot` from a running
  Chrome over the DevTools protocol.
- `scripts/diff_heap_snapshots.py` — diffs two snapshots; reads no browser.

## The loop

1. **Start Chrome with a debug port** and open the page under test:

   ```
   "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
     --remote-debugging-port=9222 --user-data-dir=/tmp/leakchrome
   ```

   A separate `--user-data-dir` keeps it off your normal profile. The capture
   script connects without an `Origin` header, so `--remote-allow-origins` is not
   needed. Confirm the tab is visible: `capture_heap_snapshot.py --list`.
   That profile carries no logins: if the page under test is behind one, log in
   in that window before taking the baseline snapshot.

2. **Baseline snapshot**, at rest:

   ```
   uv run "${CLAUDE_PLUGIN_ROOT}/skills/heap-snapshot-leaks/scripts/capture_heap_snapshot.py" \
     --url-contains myapp --out leaks/before.heapsnapshot
   ```

3. **Do the suspect action N times** (open and close the modal, route away and
   back, ~10–20×) — drive it with `claude-in-chrome` or by hand. Repetition is
   what separates a real leak from one-off allocation.

4. **Second snapshot** the same way, to `leaks/after.heapsnapshot`. The capture
   forces a GC first, so what remains is genuinely retained.

5. **Diff:**

   ```
   uv run "${CLAUDE_PLUGIN_ROOT}/skills/heap-snapshot-leaks/scripts/diff_heap_snapshots.py" \
     --before leaks/before.heapsnapshot --after leaks/after.heapsnapshot \
     [--top 25] [--min-size-delta 50000]
   ```

6. **Read the report, fix, repeat** until the suspects and the detached count
   stop climbing with N.

## Reading the report

- `summary.suspects` — constructors that both grew in count and retained more
  bytes: the leak candidates, worst first. A count that climbs in step with N
  (10 repeats → ~10 more) is the tell.
- `growth[]` — every constructor by retained-byte change: `countBefore/After`,
  `countDelta`, `sizeDeltaBytes`. Raise `--min-size-delta` to cut noise.
- `detached` — DOM nodes still referenced after leaving the document:
  `before`, `after`, `delta`, and `topTypes`. A rising `delta` means the page
  holds views it removed — usually a listener or a closure keeping them alive.
- `totalSelfSizeDeltaBytes`, `nodeCountDelta` — the overall drift; near zero
  across a repeated action is the goal.

## Interpreting common leaks

- Growing `Detached HTMLxxxElement` / `EventListener` → a listener not removed
  on unmount/teardown.
- Growing framework component/fiber/scope constructors → components retained
  after they should unmount; check subscriptions, timers, closures over `this`.
- Growing `Array` / `Map` / `Object` with no ceiling → an unbounded cache.

## Requirements

- Google Chrome, started with `--remote-debugging-port` (any recent version).
- `uv` on PATH; the scripts declare their own dependencies.
