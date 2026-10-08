---
name: browse
description: Drive a persistent headless Chromium from the shell with the `browse` CLI — navigate, click, fill, run JS, read console and network logs, take screenshots, assert element states — about 100ms per command after a ~3s first start. Use when a script or a UI probe needs a real browser, when asked to open a page, take a screenshot, test a flow or read a page's console errors, or when several probes must run in parallel, one tab each. NOT for driving the user's own Chrome window (use claude-in-chrome) and NOT for heap-snapshot leak hunting (use heap-snapshot-leaks).
---

# browse

A persistent headless Chromium behind a small CLI. The first command starts a
daemon (~3s); every later command is ~100ms. Cookies, localStorage, tabs and
logins persist between calls until the daemon stops.

This is gstack's `browse` (MIT), vendored at a pinned commit with this skill's
patches applied on top — see `UPSTREAM.md`.

## Setup

In Claude Code, `browse` is this plugin's `bin/browse` launcher, which Claude
Code puts on the Bash tool's PATH after the user's own PATH. Check which one
resolves:

```bash
B=$(command -v browse) && echo "$B"
```

If that is not `<plugin install path>/bin/browse`, another `browse` earlier on
PATH shadows it; set `B` to the plugin's `bin/browse` explicitly. Outside Claude
Code, call `<plugin install path>/bin/browse` directly (scripts can take it in a
variable such as `BROWSE_BIN`).

The launcher builds the patched CLI on first use into
`${CLAUDE_PLUGIN_DATA:-~/.claude/plugins/data/web-blockchainian}/browse/<build id>/`,
where the build id is a digest of `vendor/gstack/`, `patches/` and `scripts/build.sh`. The first call
takes a few seconds and needs `bun` on PATH; build output goes to stderr only.
A new build id means a new binary, and the running daemon restarts on its next
command, losing every tab and login in it. Warn the user before the first call
after a plugin update while other sessions use the daemon.

## One tab per client (parallel runs)

Every session on the machine shares one daemon. Without a tab of its own, a
client drives whatever tab is active and races every other client. To run in
parallel, open a tab and pin every command to it with `BROWSE_TAB`:

```bash
export BROWSE_TAB=$("$B" newtab --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["tabId"])')
trap '"$B" closetab "$BROWSE_TAB" >/dev/null 2>&1' EXIT
"$B" goto http://localhost:3004
"$B" console --clear      # clears this tab's console entries only
"$B" network             # this tab's requests only
```

A pinned `console`, `console --errors`, `console --clear`, `network` and
`network --clear` see only that tab's entries (this skill's patch); unpinned they
see every tab. `--tab-id <N>` on one command overrides `BROWSE_TAB`.

Shared across tabs, so not safe to run in parallel:
- the login: tabs share one browser context, so `localStorage.clear()` or a
  logout in one tab logs every tab out;
- `network --capture`: one capture at a time for the whole daemon;
- `download`, `scrape`, `archive` and `chain` resolve the active tab after an
  await, so overlapping requests can land them on another client's tab.

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

After a screenshot, Read the PNG so the user can see it.

Two behaviors that silently invalidate screenshots:
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

Both are scoped to files under the working directory or `$TMPDIR`.

## Logins and handoff

Logins live in the daemon's browser context and die with the daemon. Save and
restore the browser state (cookies and tab URLs) per working directory
(`.gstack/` under the git root):

```bash
$B state save signed-in
$B state load signed-in
```

When a page needs a human (CAPTCHA, MFA, OAuth consent), hand off:

```bash
$B handoff "Stuck on CAPTCHA at login page"   # opens a visible Chrome here
# ask the user to finish, then:
$B resume
```

## Headed mode and proxies

Headed windows open on the display `BROWSER_DISPLAY` names (any part of its
name, any case, e.g. `Color LCD`; empty means the main display), set in the
environment or in `~/.config/web/.env`. Windows that open elsewhere are moved
there (needs Accessibility trust for the daemon), and new tabs open in the
background, so the browser does not take focus from the user's app.

`--headed` and `--proxy` apply only when the daemon starts; with a daemon
already running in another config, browse refuses and asks for
`browse disconnect` first.

```bash
browse --headed goto https://example.com
browse --proxy socks5://user:pass@host:1080 goto https://example.com
browse download "https://protected.example.com/file" /tmp/file.bin --navigate
```

Pass proxy credentials in the URL or in `BROWSE_PROXY_USER` /
`BROWSE_PROXY_PASS`, never both.

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

Before using any command or snapshot flag not in this table, Read
`sections/command-list.md` next to this file in full; it is the reference for
every command, its arguments, and every snapshot flag.
