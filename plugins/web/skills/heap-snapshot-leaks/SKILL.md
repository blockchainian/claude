---
name: heap-snapshot-leaks
description: Find memory leaks in a running web app by diffing V8 heap snapshots taken around a repeated action. Use when a page's memory grows over time, a single-page app slows the longer it runs, or you suspect detached DOM nodes, dangling listeners, or retained components after navigating away. Captures snapshots from Chrome over the DevTools protocol and reports which constructors grew and what stayed detached.
---

# Heap snapshot leaks

A web leak is what the heap keeps that a clean baseline did not: a constructor whose live instance
count and retained bytes climb with each repeat of an action, or DOM nodes the page still
references after they left the document ("detached"). You take one heap snapshot before and one
after repeating that action, then diff them into a ranked list of suspects.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call that runs a
script, not to the caller's working directory or a host-specific plugin variable. If the loaded
path is unavailable, stop and report it before running a script.

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `WEB_DATA_DIR` | Data root for the throwaway Chrome debug profile; default `$HOME/.local/share/web` | No | shell |

## Setup

The scripts use only Node builtins, so there is nothing to install. Capturing needs Node 22+ on
`PATH` for the built-in `WebSocket`; diffing runs on Node 18.18+. Any recent Google Chrome works,
started with a debug port as below.

## Find a leak

Start Chrome with a debug port and a separate profile, then open the page under test in it:

```bash
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --remote-debugging-port=9222 \
  --user-data-dir="${WEB_DATA_DIR:-$HOME/.local/share/web}/heap-snapshot-leaks/profile"
```

The separate profile keeps the run off your normal one and is safe to delete. It carries no
logins, so if the page sits behind one, log in in that window before the baseline. The capture
script sends no `Origin` header, so Chrome does not need `--remote-allow-origins`. Run
`"$SKILL_DIR/scripts/capture-heap-snapshot.mjs" --list` to confirm the tab is visible.

Take the baseline snapshot with the page at rest:

```bash
"$SKILL_DIR/scripts/capture-heap-snapshot.mjs" \
  --url-contains myapp --out leaks/before.heapsnapshot
```

Then do the suspect action about 10 to 20 times, such as opening and closing a modal or routing
away and back; repetition is what separates a real leak from a one-off allocation. Drive it with
the host's connected Chrome tools (Claude's `claude-in-chrome` or Codex's connected Chrome) after
confirming they can reach the same debug-profile tab, or else have the user do it by hand in that
window. Take the second snapshot the same way to `leaks/after.heapsnapshot`; the capture forces a
garbage collection first, so what remains is genuinely retained. Diff the two:

```bash
"$SKILL_DIR/scripts/diff-heap-snapshots.mjs" \
  --before leaks/before.heapsnapshot --after leaks/after.heapsnapshot \
  [--top 25] [--min-size-delta 50000]
```

Read the report, fix the cause, and repeat the loop until the suspects and the detached count stop
climbing with the number of repeats.

## Reading the report

- `summary.suspects`: constructors that both grew in count and retained more bytes, worst first.
  These are the leak candidates; a count that climbs in step with the repeats (10 repeats, about 10
  more instances) is the tell.
- `growth[]`: every constructor ranked by retained-byte change, with `countBefore`, `countAfter`,
  `countDelta` and `sizeDeltaBytes`. Raise `--min-size-delta` to cut noise.
- `detached`: DOM nodes still referenced after leaving the document, as `before`, `after`, `delta`
  and `topTypes`. A rising `delta` means the page holds views it removed, usually through a
  listener or closure that keeps them alive.
- `totalSelfSizeDeltaBytes` and `nodeCountDelta`: the overall drift, which should be near zero
  across a repeated action.

Common patterns point to these causes:

- Growing `Detached HTMLxxxElement` or `EventListener`: a listener not removed on unmount or
  teardown.
- Growing framework component, fiber or scope constructors: components retained after they should
  unmount; check subscriptions, timers and closures over `this`.
- Growing `Array`, `Map` or `Object` with no ceiling: an unbounded cache.

## Tests

```bash
npm run test:web
```
