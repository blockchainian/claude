---
name: capture-app-traffic
description: Capture and decode the HTTP and WebSocket traffic of one target web or mobile app with mitmproxy. Use when asked to see what API calls an app makes, intercept or sniff a web or iPhone app's requests, reverse-engineer an app's API or WebSocket protocol, or set up mitmproxy with Zero Omega. Scopes the capture to the target app's hosts and passes everything else through untouched.
---

# Capture App Traffic

Capture the network traffic of **one target app** — a website driven from this Mac, or an
iPhone app — decrypt its TLS, and read its REST and WebSocket protocol. mitmproxy sits in
the middle as a trusted CA; this skill owns the run end to end: check setup, start a
target-scoped capture, connect the client, read the flows, tear it down.

The defining constraint is **scope**. A capture is aimed at a named app by its domains. Only
those hosts are decrypted and saved; everything else on the Mac or phone — a bank, iMessage,
other apps — passes through as an opaque tunnel and is never written. That keeps the file
about the one app and keeps unrelated private traffic out of it. Never run unscoped except
for a short, deliberate discovery pass (below), and say so when you do.

## Every name here is a value to paste, not a variable

Each Bash call runs in its own shell; nothing assigned in one command survives to the next.
Where this document writes `$RUN`, `$PORT`, `$SKILL_DIR` or `$PROXYMAN_DIR`, it means the
actual value — read it out of the JSON a previous command printed and type it in full.
`$SKILL_DIR` is the absolute path of this loaded skill folder, which you already know; do not
derive it from the target app's working directory, since an installed plugin lives elsewhere.
All scripts print JSON on stdout — parse stdout, act on the values.

## Running many at once

Two sessions or agents can capture two different apps at the same time. Each capture is a
**run** with its own listen port, its own output directory under `$PROXYMAN_DIR` (default
`/tmp/proxyman`), and its own mitmdump process; ports are held with lock files so a second
run never lands on a taken one. The one shared resource is the mitmproxy CA in `~/.mitmproxy`
— installed once, reused by every run. WireGuard mode is the exception: it needs UDP 51820
and the shared server key, so only one WireGuard capture runs at a time (proxy captures are
unlimited and can run alongside it). Point `$PROXYMAN_DIR` at a durable directory to keep
captures across a reboot — the default under `/tmp` is cleared then.

## 0. Setup (skip if already set up)

```bash
"$SKILL_DIR/scripts/setup.sh"
```

Exit 0 means mitmdump is installed and the CA is generated and trusted in the System
keychain — an existing user; **skip the rest of this section**. Exit 1 lists the remaining
steps, each a command to run in order. Two of them the user must run, not you:
`brew install mitmproxy` (network) and the `sudo security add-trusted-cert ...` line (sudo
cannot read a password through this harness — ask the user to paste it into a real terminal,
or to type it here prefixed with `!`). The CA-generation step (`setup.sh --generate-ca`) you
can run yourself. Re-run `setup.sh` after the user reports done, and confirm exit 0 before
capturing. A missing `qrencode` is reported but does not block readiness — it only makes the
WireGuard QR; without it `wg_config.py` prints the config as text to import by hand.

**iPhone, one extra time:** after the Mac trusts the CA, the phone must trust it too. With
the WireGuard tunnel on (see below), open `http://mitm.it` in Safari — it is served over the
tunnel — install the iOS profile, then **Settings > General > About > Certificate Trust
Settings** and toggle the mitmproxy CA on. Without that toggle, TLS interception fails on the
phone.

## 1. Know the target's hosts

Scoping needs the app's domains. For a website you usually know them — but check the network
tab or a discovery pass for the API and WebSocket hosts, which often sit on a different
domain than the page (`axiom.trade` served its API from `api2.axiom.trade` and its socket
from a `cluster*.axiom.trade`). For a mobile app you rarely know them up front.

**Discovery pass** (only when you don't know the hosts): start a capture with **no**
`--hosts`, drive only the target app for a minute, stop, and list what it talked to:

```bash
"$SKILL_DIR/scripts/capture.py" start --mode proxy --label discover
# ... connect the client, use the app briefly, then stop (section 4) ...
mitmdump -q -nr "$PROXYMAN_DIR/$RUN/flows.mitm" -s "$SKILL_DIR/scripts/hosts.py"
```

`hosts.py` prints each host and its flow count, most-frequent first. Pick the app's own API
and socket hosts from that list — ignore analytics, crash reporters and CDNs unless they are
the point. An unscoped capture holds unrelated traffic, so treat its file as throwaway and
re-capture scoped to the hosts you found.

## 2. Start a scoped capture

### Mac website (Zero Omega)

```bash
"$SKILL_DIR/scripts/capture.py" start --mode proxy --hosts "axiom.trade,api2.axiom.trade" --label axiom
```

Read `proxyLocal` (e.g. `127.0.0.1:9080`) from the JSON. In the Zero Omega extension, create
a proxy profile pointing at that host and port and switch it on **for the target site only**
— an auto-switch rule for the app's domains, not a global proxy, so the rest of your browsing
is untouched. The capture's host filter is a second line of defence: even a global proxy
setting would only ever save the target hosts.

If the browser shows `NET::ERR_CERT_AUTHORITY_INVALID` or an HSTS block with no bypass, the
CA is not trusted yet — go back to setup. HSTS sites (axiom among them) forbid clicking
through, so the CA must be trusted; there is no skip. As a throwaway alternative for one
site, launch Chrome with `--ignore-certificate-errors --user-data-dir=/tmp/chrome-proxy
--proxy-server=127.0.0.1:$PORT`, which sidesteps the keychain entirely.

### iPhone app — WireGuard

WireGuard is the iPhone path: it captures the whole phone, including background and
non-proxy-aware traffic, and the host filter is what keeps the capture to the target app.

```bash
"$SKILL_DIR/scripts/capture.py" start --mode wireguard --hosts "pump.fun,api.pump.fun" --label pump
"$SKILL_DIR/scripts/wg_config.py" --qr "$PROXYMAN_DIR/$RUN/wg-qr.png"
```

`wg_config.py` prints the client config and writes a QR. Send the QR to the user (see
Reporting), have them import it into the WireGuard app and toggle the tunnel on. **Only one
WireGuard capture at a time**; a second `start` exits 3 naming the holder. AllowedIPs is
`0.0.0.0/0`, so while the tunnel is on the whole phone routes through the Mac — the host
filter is what stops unrelated traffic being saved, and the tunnel must be turned off when
done. If the phone says "unable to create tunnel", the user declined the VPN permission; it
is not a config problem.

The phone must be on the same LAN as the Mac, with the router's AP/client isolation turned
**off** — with it on the phone cannot reach the Mac at all, and `mitm.it` will not load. That
was the first thing to check when nothing connected.

## 3. Watch it live (optional)

The capture writes to its flow file continuously; you do not need a live view. When you want
one, tail the flow file as it grows — every reader below works on a partial file. Prefer this
over `mitmweb`: mitmweb prints its auth token only to a real TTY, so it is awkward to drive
from here.

## 4. Stop, then read

```bash
"$SKILL_DIR/scripts/capture.py" status                 # list runs, running or not, and sizes
"$SKILL_DIR/scripts/capture.py" stop "$RUN"            # stop this run, report file + size
```

Read the saved `.mitm` file with the addons — `.mitm` is mitmproxy's native binary flow
format, re-readable only by mitmproxy tools, which is why these go through `mitmdump -nr`:

```bash
# One line per request: method, status, host+path, content-type.
mitmdump -q -nr "$PROXYMAN_DIR/$RUN/flows.mitm" -s "$SKILL_DIR/scripts/flowlog.py"

# WebSocket frames; WSHOST filters to one host, WSMAX sets the truncation width.
WSHOST=pump.fun mitmdump -q -nr "$PROXYMAN_DIR/$RUN/flows.mitm" -s "$SKILL_DIR/scripts/wslog.py"

# Host tally, to confirm the scope held or to pick hosts after a discovery pass.
mitmdump -q -nr "$PROXYMAN_DIR/$RUN/flows.mitm" -s "$SKILL_DIR/scripts/hosts.py"
```

For a specific request or response body, filter with mitmproxy's flow language and dump it —
e.g. one endpoint's response:

```bash
mitmdump -q -nr "$PROXYMAN_DIR/$RUN/flows.mitm" '~u /api/trade & ~s' \
  --set flow_detail=3 2>/dev/null
```

## 5. Clean up

Stop every run when the investigation is done, and for a WireGuard run tell the user to turn
the tunnel off. Delete a run's directory when you no longer need the flows. If the user is
done with mitmproxy entirely and asks to undo setup, the CA is removed with
`sudo security delete-certificate -c mitmproxy /Library/Keychains/System.keychain` (their
terminal — sudo). Leave the CA installed otherwise; re-trusting it is the slow part of setup.

## Safety and privacy

- Captures hold **unredacted** auth tokens, cookies and JWTs — anything the app sends. Treat
  a flow file as a secret. Never paste tokens into a report or send a raw flow file to an
  external service; quote only the shape of a request, not its credentials.
- The host filter is what keeps a capture to the target app. Run unscoped only for a short
  discovery pass, and never leave a WireGuard tunnel on after capturing — it routes the whole
  phone through the Mac.
- Read-only against the app. This skill captures traffic; it does not drive the app. When the
  app must be driven to reach a screen, that is the user's to do, or another skill's — do not
  tap through transactions to generate traffic.
- Cert pinning defeats a proxy: a pinned app rejects the mitmproxy CA and its TLS simply
  fails. That needs Frida/objection to bypass and is out of scope here — report it as pinned
  rather than retrying.
- **iOS simulators are off-limits unless the user explicitly asks.** Do not add the CA to a
  simulator keychain or otherwise touch a simulator to make a capture work.

## Reporting

When a capture is on a phone, the user is often on another device — send the WireGuard QR and
any screenshot-worthy artifact with `SendUserFile` rather than only printing a path. State
the target hosts, which mode, the flow file path and size, and for the protocol findings give
the endpoint shapes and WebSocket message formats — never the tokens.

## Tests

`scripts/test_capture.py` covers target scoping and isolation: the host filter matches an
app's domains and subdomains but not lookalikes and treats dots literally, two concurrent
runs receive different ports, and a port whose holder has died is reused. `scripts/test_wg_config.py`
pins the pure-Python X25519 derivation against a known mitmproxy key pair and the RFC 7748
vector. Run both with `python3 scripts/test_capture.py` and `python3 scripts/test_wg_config.py`
after changing either script.
