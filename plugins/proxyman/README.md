# proxyman

Capture and decode the HTTP and WebSocket traffic of **one target web or mobile app** with
[mitmproxy](https://mitmproxy.org), scoped to that app's hosts so unrelated traffic is passed
through untouched and never saved.

One skill, `inspect-app-traffic`, owns the run end to end: check setup, start a
target-scoped capture, connect the client (a Mac browser via the Zero Omega extension, or an
iPhone via a WireGuard tunnel), read the flows, tear it down.

## What it gives you

- **Target scoping.** A capture is aimed at an app by its domains: only those hosts are
  decrypted (`--allow-hosts`) and saved (`save_stream_filter`); everything else — a bank,
  iMessage, other apps — passes through as an opaque tunnel and is never written.
- **Parallel-safe.** Each capture is a run with its own port, output directory under
  `PROXYMAN_DIR` (default `/tmp/proxyman`), and mitmdump process, held with lock files. Two
  agents can capture two apps at once. WireGuard mode is the one single-holder resource.
- **Setup that skips itself.** `setup.sh` detects an existing user (mitmdump installed, CA
  generated and trusted) and exits 0; otherwise it prints the exact remaining steps.
- **Offline readers.** `flowlog.py` (one line per request), `wslog.py` (WebSocket frames),
  and `hosts.py` (host tally, for discovering an app's hosts) read a saved `.mitm` file.
- **WireGuard config, no dependencies.** `wg_config.py` derives the client config and a QR
  from mitmproxy's keys with a pure-Python X25519, needing neither the `cryptography` module
  nor the `wg` tool.

## Requirements

- macOS with [Homebrew](https://brew.sh) (`brew install mitmproxy`; `setup.sh` guides this).
- `qrencode` for the WireGuard QR (optional; `brew install qrencode`).
- iPhone captures need the phone on the same LAN with router AP/client isolation off.

## Test

```
python3 skills/inspect-app-traffic/scripts/test_capture.py
python3 skills/inspect-app-traffic/scripts/test_wg_config.py
```

Or from the marketplace root: `npm run test:proxyman`.
