---
name: browse
description: Drive a persistent headless Chromium from the shell with the `browse` CLI — navigate, click, fill, run JS, read console and network logs, take screenshots, assert element states — about 100ms per command after a ~3s first start. Use when a script or a UI probe needs a real browser, when asked to open a page, take a screenshot, test a flow or read a page's console errors, or when several probes must run in parallel, one tab each. NOT for driving the user's own Chrome window (use claude-in-chrome) and NOT for heap-snapshot leak hunting (use heap-snapshot-leaks).
---

# Browse

`browse` is a small CLI in front of a persistent headless Chromium. The first
command starts a daemon in about 3 seconds and every later command takes about
100 ms. Cookies, localStorage, tabs and logins persist between calls until the
daemon stops.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `BROWSE_TAB` | Pins every command to one tab; see One tab per client | No | the caller's shell |
| `BROWSE_HEADED` | `1` starts the daemon headed, like `--headed` | No | shell profile |
| `BROWSER_DISPLAY` | Display for the headed window; see Headed mode and proxies | No | shell, or `~/.config/web/.env` |
| `BROWSE_PROXY_USER`, `BROWSE_PROXY_PASS` | Proxy credentials for `--proxy`, instead of putting them in its URL | No | shell |
| `CHROMIUM_PROFILE` | Profile directory of the headed browser (default `~/.gstack/chromium-profile`) | No | shell |
| `BROWSE_STATE_FILE` | State file that picks which daemon a client talks to (default `.gstack/browse.json` under the git root or working directory) | No | shell |
| `BROWSE_IDLE_TIMEOUT` | Milliseconds of inactivity before the daemon shuts down, losing its tabs and logins (default 1800000, 30 min) | No | shell |
| `CLAUDE_PLUGIN_DATA` | Parent directory of the launcher's builds | No | set by Claude Code |

## Setup

In Claude Code, `browse` is this plugin's `bin/browse` launcher, which Claude
Code appends to the Bash tool's PATH after the user's own entries. Check which
one resolves:

```bash
B=$(command -v browse) && echo "$B"
```

If the path printed is not `<plugin install path>/bin/browse`, another `browse`
earlier on PATH shadows the launcher, so set `B` to the plugin's `bin/browse`
explicitly. Outside Claude Code, call `<plugin install path>/bin/browse`
directly; a script can take it in a variable such as `BROWSE_BIN`.

The first call after an install or a plugin update builds the CLI. The build
takes several seconds, needs `bun`, `git` and network access, and writes its
output to stderr, so stdout carries only browse's own output. A new build also
restarts each running daemon on its next command, which loses every tab and
login in it; while other sessions use the daemon, warn the user before that
first call.

## One tab per client (parallel runs)

Each git root, or working directory outside a repo, has one daemon that every
session working there shares, while sessions in other repos get their own. All
of them share the headed browser's profile (`CHROMIUM_PROFILE`). Never kill a
daemon with `pkill -f browse` or `pkill terminal-agent`: a pkill hits every
repo's daemon and drops every session's tabs and logins.

A client without a tab of its own drives whatever tab is active and races every
other client. To run in parallel, open a tab and pin every command to it with
`BROWSE_TAB`:

```bash
export BROWSE_TAB=$("$B" newtab --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["tabId"])')
trap '"$B" closetab "$BROWSE_TAB" >/dev/null 2>&1' EXIT
"$B" goto http://localhost:3004
"$B" console --clear      # clears this tab's console entries only
"$B" network             # this tab's requests only
```

Pinned, `console`, `console --errors`, `console --clear`, `network` and
`network --clear` see only that tab's entries; unpinned, they see every tab.
`--tab-id <N>` on a single command overrides `BROWSE_TAB`.

Three things are shared across tabs and are not safe to run in parallel:
- the login: tabs share one browser context, so `localStorage.clear()` or a
  logout in one tab logs every tab out;
- `network --capture`: the whole daemon runs one capture at a time;
- `download`, `scrape`, `archive` and `chain`: they resolve the active tab after
  an await, so overlapping requests can land them on another client's tab.

## Core QA patterns

### Verify a page loads
```bash
$B goto https://app.com
$B text                          # page text
$B console --errors              # JS errors and warnings
$B network                       # requests with status
$B is visible ".hero"
```

### Test a user flow
```bash
$B goto https://app.com/login
$B snapshot -i                   # interactive elements with @e refs
$B fill @e3 "user@test.com"
$B fill @e4 "password"
$B click @e5                     # submit
$B snapshot -D                   # diff: what changed after submit?
$B is visible ".dashboard"
```

### Visual evidence
```bash
$B snapshot -i -a -o /tmp/annotated.png   # labeled screenshot
$B screenshot /tmp/bug.png                # full-page screenshot
$B screenshot /tmp/card.png --selector .card
```

After a screenshot, Read the PNG so the user can see it. Two behaviours
silently invalidate screenshots:
- **`hover` scrolls its target into view.** Before a rest-state shot, hover only
  something already visible, and check `$B js "window.scrollY"`.
- **The tab persists across sessions.** Start every pass with an explicit
  `$B goto <url>`, never a bare `reload` or `screenshot`.

### Find clickable elements without ARIA roles
```bash
$B snapshot -C                   # divs with cursor:pointer, onclick, tabindex
$B click @c1
```

### Assert element states
```bash
$B is visible ".modal"
$B is enabled "#submit-btn"
$B is checked "#agree-checkbox"
$B is focused "#search-input"
$B js "document.body.textContent.includes('Success')"
```

### Responsive layouts
```bash
$B responsive /tmp/layout        # mobile + tablet + desktop screenshots
$B viewport 375x812
$B viewport 480x600 --scale 2    # 2x deviceScaleFactor; invalidates @refs
```

### Uploads and dialogs
```bash
$B upload "#file-input" /path/to/file.pdf
$B dialog-accept "yes"           # set up the handler before the trigger
$B click "#delete-button"
$B dialog                        # what appeared
```

### Local HTML
```bash
$B goto file:///tmp/report.html
$B load-html /tmp/tweet.html     # setContent; URL stays about:blank
```

## Logins and handoff

Logins live in the daemon's browser context and die with the daemon. To keep
one, save the browser state (cookies and tab URLs) and load it later; saved
states live per working directory, in `.gstack/` under the git root:

```bash
$B state save signed-in
$B state load signed-in
```

When a page needs a human for a CAPTCHA, MFA or OAuth consent, hand it off:

```bash
$B handoff "Stuck on CAPTCHA at login page"   # opens a visible Chrome here
# ask the user to finish, then:
$B resume
```

## Headed mode and proxies

The headed window opens on the display `BROWSER_DISPLAY` names, set in the
environment or in `~/.config/web/.env`. The value matches any part of the name
macOS gives the screen, in any case (for example `Built-in`), and empty means
the main display. New tabs open in the background of that window, so the
browser does not take focus from the user's app.

`--headed` and `--proxy` apply only when the daemon starts. Switching a running
daemon to another config takes `browse disconnect`, which drops the daemon's
tabs and logins, so while other sessions use the daemon, ask the user first.

```bash
browse --headed goto https://example.com
browse --proxy socks5://user:pass@host:1080 goto https://example.com
browse download "https://protected.example.com/file" /tmp/file.bin --navigate
```

## Most-used commands

| Command | What it does |
|---------|--------------|
| `goto <url>` | Navigate (also `file://` paths) |
| `snapshot -i` | Accessibility tree with @e refs (`-D` diff, `-C` cursor-interactive @c refs, `-a -o <png>` annotated shot) |
| `click <sel>` / `fill <sel> <val>` / `hover <sel>` | Interact — CSS selectors or @refs |
| `text` / `html [sel]` | Page text / HTML |
| `js "<expr>"` | Run JavaScript, result to stdout |
| `is <state> <sel>` | Assert visible/hidden/enabled/disabled/checked/editable/focused |
| `console [--errors\|--clear]` / `network [--clear]` | Console log / requests with status |
| `screenshot <path>` | Full-page PNG (`--selector <sel>` for one element) |
| `wait <sel>` / `wait --load` | Wait for an element / for the page load |
| `viewport WxH` | Set viewport (`--scale 2` for retina) |
| `newtab [url] [--json]` / `tab <id>` / `tabs` / `closetab [id]` | Tabs |

Before using a command or snapshot flag that is not in this table, Read
`sections/command-list.md` next to this file in full: it documents every
command, its arguments and every snapshot flag.
