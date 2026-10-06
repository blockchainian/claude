# proxy

Capture and decode the HTTP and WebSocket traffic of a target web or mobile app with
[mitmproxy](https://mitmproxy.org), through one shared proxy that every session reuses.

One skill, `inspect-app-traffic`, owns the run end to end: check setup, start a capture,
connect the client (a Mac browser via the Zero Omega extension, or an iPhone via a WireGuard
tunnel), read the flows, tear it down.

The skill works in Claude Code and Codex. Browser automation must use the user's
logged-in Chrome profile with Zero Omega enabled; the proxy scripts do not depend
on either host's browser tools.

## Install

For Codex installation, see
[Codex installation](../../README.md#codex).

In Claude Code:

```
/plugin marketplace add blockchainian/claude
/plugin install proxy@blockchainian
```

## What it gives you

- **One shared hub.** A single long-lived mitmdump serves the HTTP proxy and, when asked,
  WireGuard, on the fixed port 8080. The browser needs only one Zero Omega profile; the phone
  one tunnel. The hub records everything routed to it into one flow file under `PROXY_DATA_DIR`
  (default `~/.local/share/proxy`); the running hub and the capture registry live under
  `PROXY_STATE_DIR` (default `~/.local/state/proxy`).
- **Captures are views, not processes.** `start` notes the moment and the target hosts; the
  readers show only that capture's window, scoped to its hosts. Two agents capturing two apps
  at once are two records over one hub, separated at read time — no second proxy, no second
  Zero Omega profile.
- **Shared-domain attribution.** When two apps share a host (e.g. a shared auth provider), `origins.py`
  splits the capture by caller (`Origin`/`Referer`/app-id).
- **Setup that skips itself.** `setup.sh` detects an existing user (mitmdump installed, CA
  generated and trusted) and exits 0; otherwise it prints the exact remaining steps.
- **Readers over the hub file.** `flowlog.py` (one line per request), `wslog.py` (WebSocket
  frames), `hosts.py` (host tally), `origins.py` (callers), each scoped by `since`/`host`.
- **WireGuard config, no dependencies.** `wg-config.mjs` derives the client config and a QR
  from mitmproxy's keys with node:crypto X25519, needing neither an npm package nor the
  `wg` tool.

## Requirements

- macOS with [Homebrew](https://brew.sh) (`brew install mitmproxy`; `setup.sh` guides this).
- `qrencode` for the WireGuard QR (optional; `brew install qrencode`).
- iPhone captures need the phone on the same LAN with router AP/client isolation off.

## Tests

```
node --test skills/inspect-app-traffic/tests/*.mjs
```

Or from the marketplace root: `npm run test:proxy`.

`capture.test.mjs` covers the pure logic and the hub orchestration without launching mitmdump:
the host regex matches an app's domains and subdomains but not lookalikes; the port check
detects a wildcard listener and allows a TIME_WAIT port; and `start`/`stop`/`status`/`down`/
`check` act on capture records over a faked hub. `wg-config.test.mjs` pins the node:crypto
X25519 derivation against a known mitmproxy key pair and the RFC 7748 vector. The hub
lifecycle, the fan-out into per-capture files, and the caller scoping are verified in a live
capture.

## Environment Variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `PROXY_DATA_DIR` | Captured flow files; default ~/.local/share/proxy | Optional | Shell environment |
| `PROXY_STATE_DIR` | The running hub and the capture registry; default ~/.local/state/proxy | Optional | Shell environment |
| `PROXY_CONFIG_DIR` | mitmproxy certificates/config; default ~/.mitmproxy | Optional | Shell environment |
