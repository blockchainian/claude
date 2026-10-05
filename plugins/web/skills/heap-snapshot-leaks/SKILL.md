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

Two scripts, both plain Node (no npm install; Node builtins only):
- `scripts/capture-heap-snapshot.mjs` — pulls one `.heapsnapshot` from a running
  Chrome over the DevTools protocol. Needs Node 22+ (global `WebSocket`).
- `scripts/diff-heap-snapshots.mjs` — diffs two snapshots; reads no browser.

## Script location

Resolve this skill's directory from the absolute path of the loaded `SKILL.md`.
In each shell call that runs a script, set `SKILL_DIR` to that directory:

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Use the actual installed path, not the caller's working directory or a
host-specific plugin environment variable. Shell variables may not persist
between tool calls; repeat the assignment in each call. If the loaded path is
unavailable, stop and report it before running a script.

## The loop

1. **Start Chrome with a debug port** and open the page under test:

   ```
   "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
     --remote-debugging-port=9222 --user-data-dir=/tmp/leakchrome
   ```

   A separate `--user-data-dir` keeps it off your normal profile. The capture
   script connects without an `Origin` header, so `--remote-allow-origins` is not
   needed. Confirm the tab is visible: `capture-heap-snapshot.mjs --list`.
   That profile carries no logins: if the page under test is behind one, log in
   in that window before taking the baseline snapshot.

2. **Baseline snapshot**, at rest:

   ```
   "$SKILL_DIR/scripts/capture-heap-snapshot.mjs" \
     --url-contains myapp --out leaks/before.heapsnapshot
   ```

3. **Do the suspect action N times** (open and close the modal, route away and
   back, ~10–20×) — drive it with the current host's connected Chrome tools
   (such as Claude's `claude-in-chrome` or Codex's connected Chrome), or by hand.
   Confirm those tools can access the same debug-profile tab before using them;
   if they cannot, have the user perform the action in that window. Repetition
   is what separates a real leak from one-off allocation.

4. **Second snapshot** the same way, to `leaks/after.heapsnapshot`. The capture
   forces a GC first, so what remains is genuinely retained.

5. **Diff:**

   ```
   "$SKILL_DIR/scripts/diff-heap-snapshots.mjs" \
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
- Node 22+ on PATH (the capture script uses the built-in `WebSocket`; the diff
  script runs on Node 18.18+). No npm dependencies.
