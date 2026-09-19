---
name: inspect-app-traffic
description: Capture and decode the HTTP and WebSocket traffic of one target web or mobile app with mitmproxy. Use when asked to see what API calls an app makes, intercept or sniff a web or iPhone app's requests, reverse-engineer an app's API or WebSocket protocol, or set up mitmproxy with Zero Omega. One shared proxy on port 8080; each capture is a filtered view of its stream.
---

# Inspect App Traffic

Capture the HTTP and WebSocket traffic of a target app — a website driven from this Mac
through the Zero Omega extension, or an iPhone over WireGuard — decrypt its TLS, and read its
REST and WebSocket protocol.

**One shared hub.** A single long-lived mitmdump — the hub — serves the HTTP proxy and,
when asked, WireGuard, on the fixed port **8080**. Every session and agent shares it: the
browser needs only **one** Zero Omega profile, pointed at 8080, and the phone needs only one
tunnel.

**A capture is a record, not a process.** `start` opens a capture with a start time and its
target hosts; the hub then fans every flow into the file of each active capture whose hosts
and window it matches. So each capture has its own file — a read touches only that app's data,
not the whole hub — and two agents capturing two apps at once run over one proxy, one port,
one Zero Omega profile, with a file per app. No second proxy, no read that pays for another
capture's traffic.

## Every name here is a value to paste, not a variable

Each Bash call runs in its own shell, so a variable assigned in one command is empty in the
next. Where this document writes `$CAP`, `$SKILL_DIR` or `$PROXY_DIR`, paste the actual value
— read it out of the JSON a previous command printed and type it in full, or run the whole
sequence as one command.

`$SKILL_DIR` is the absolute path of this loaded skill folder, which you already know; do not
derive it from the target app's working directory. All scripts print JSON on stdout — parse
stdout, act on it. The hub and the per-capture flow files live under `$PROXY_DIR` (default
`/tmp/proxy`); point it at a durable directory to keep captures across a reboot.

## 0. Setup (skip if already set up)

```bash
"$SKILL_DIR/scripts/setup.sh"
```

Exit 0 means mitmdump is installed and the CA is generated and trusted in the System keychain
— an existing user; **skip the rest of this section**. Exit 1 lists the remaining steps in
order. Two the user must run, not you: `brew install mitmproxy` (network) and the
`sudo security add-trusted-cert …` line (sudo cannot read a password through this harness —
ask the user to run it in a real terminal, or type it here prefixed with `!`). The
CA-generation step (`setup.sh --generate-ca`) you can run. Re-run `setup.sh` and confirm exit
0 before capturing. A missing `qrencode` is reported but does not block readiness — it only
makes the WireGuard QR.

**iPhone, one extra time:** after the Mac trusts the CA, the phone must trust it too. With the
WireGuard tunnel on (below), open `http://mitm.it` in Safari — served over the tunnel —
install the profile, then **Settings > General > About > Certificate Trust Settings** and
toggle the mitmproxy CA on. Without that, TLS interception fails on the phone.

## 1. Start a capture

```bash
"$SKILL_DIR/scripts/capture.py" start --label myapp --hosts myapp.com
```

This brings the hub up on 8080 if it is not already running, and opens a capture. Read its
`capture` id from the JSON — that is `$CAP` for every command below.

- `--hosts` is the read scope, not an interception filter (the hub keeps everything). List
  the app's domains; subdomains are matched automatically, so `myapp.com` also covers
  `api.myapp.com` and `www.myapp.com`. Omit it to read every host in the window.
- `--wireguard` also brings up the phone tunnel (below). Add it only for a phone capture.

### Mac website — Zero Omega (one profile, forever)

Point **one** Zero Omega profile at **`127.0.0.1:8080`** and enable it for the target site
(an auto-switch rule for the app's domains, or the whole browser while you work on it). Use
the user's own logged-in Chrome — the sites worth capturing are login-gated and the Claude
Chrome extension drives that profile. A throwaway Chrome (`--user-data-dir`) has no logins
and no extension; do not use it. **Zero Omega's routing is a manual step — the extension
cannot toggle it (its UI is a `chrome-extension://` page the browser tools cannot reach).**

The Zero Omega auto-switch rule must cover **every** host the app calls — the API often sits
on a different subdomain (`api.myapp.com`) than the page (`www.myapp.com`). A rule
matching only the bare domain misses the API, and nothing is captured. Use a wildcard
(`*.myapp.com`) or route the whole site.

If the browser shows `NET::ERR_CERT_AUTHORITY_INVALID` or an HSTS block, the CA is not trusted
— go back to setup; HSTS sites forbid clicking through.

### iPhone app — WireGuard

```bash
"$SKILL_DIR/scripts/capture.py" start --label pump --hosts pump.fun --wireguard
"$SKILL_DIR/scripts/wg_config.py" --qr "$PROXY_DIR/pump-qr.png"
```

`start --wireguard` brings the hub up serving WireGuard too; `wg_config.py` prints the client
config and writes a QR. Send the QR (see Reporting), have the user import it and toggle the
tunnel on. The whole phone routes through the Mac while the tunnel is on, so `--hosts` is what
scopes the read to the app; tell the user to turn the tunnel off when done. If the phone says
"unable to create tunnel", the user declined the VPN permission. The phone must be on the same
LAN with the router's AP/client isolation **off**, or it cannot reach the Mac.

## 2. Confirm traffic is arriving

After the user enables the proxy and loads the app once:

```bash
"$SKILL_DIR/scripts/capture.py" check "$CAP"
```

It cannot read Zero Omega's on/off state, but it sees what reaches the hub. `clientsConnected`
0 means nothing is routed — the proxy is not enabled (or the phone tunnel is off). Connected
with `tlsFailed` > 0 and 0 requests means the client is **rejecting the mitmproxy certificate**
— the CA is not trusted (on the phone: install via `mitm.it`, then Certificate Trust Settings)
or the app pins its cert. Connected with 0 requests and no TLS failures means what routes to the
hub — Zero Omega for the browser, the WireGuard tunnel for the phone — is not sending the
app's hosts (widen `--hosts`, or the routing rule does not cover them). Requests flowing means
it is working. Run it before investing in a drive.

Capturing is passive: traffic can come from the user clicking through the app or from the
Claude Chrome extension driving it. Either way, enable Zero Omega first.

## 3. Read the capture

```bash
"$SKILL_DIR/scripts/capture.py" read "$CAP" --kind flows     # one line per request
"$SKILL_DIR/scripts/capture.py" read "$CAP" --kind ws        # websocket frames
"$SKILL_DIR/scripts/capture.py" read "$CAP" --kind hosts     # host tally
"$SKILL_DIR/scripts/capture.py" read "$CAP" --kind origins   # callers of each host
```

`--kind ws` takes `--wsmax <chars>` to widen frame bodies. For a request or response body,
read the capture's own file directly with mitmproxy's flow language:

```bash
mitmdump -q -nr "$PROXY_DIR/cap/$CAP.mitm" '~u /api/trade & ~s' --set flow_detail=3 2>/dev/null
```

A WebSocket flow is written to the capture's file when it closes, so read its frames after
the socket ends or after `down` — a socket still open mid-capture is not in the file yet.

## 4. Concurrent captures and shared domains

When two apps share a host — a common auth provider, RPC, or analytics host — that host's
flows are written to **both** captures' files (both match it). `--hosts` cannot separate apps
on the *same* host. Split by **caller** with origins:

```bash
"$SKILL_DIR/scripts/capture.py" read "$CAP" --kind origins                       # list callers
"$SKILL_DIR/scripts/capture.py" read "$CAP" --kind origins --source app-a.example  # one app's calls
```

Each web app sends a distinct `Origin`/`Referer`, and some shared auth providers carry a per-app
id header, so a shared host separates cleanly at read time.

## 5. Close a capture, and stop the hub

```bash
"$SKILL_DIR/scripts/capture.py" status               # the hub and the open captures
"$SKILL_DIR/scripts/capture.py" stop "$CAP" --wipe   # close a capture AND delete its file
"$SKILL_DIR/scripts/capture.py" down --wipe          # stop the hub and delete all capture files
```

Capture files hold unredacted tokens and are **not** cleaned automatically. Leave nothing
behind: `stop "$CAP" --wipe` once you have read what you need (plain `stop` keeps the file for
a later read), and end the session with `down --wipe`. Leave the hub running while any capture
is active; `down` when the whole session is over — and for a WireGuard capture tell the user to
turn the tunnel off. If the user asks to undo setup entirely, the CA is removed with
`sudo security delete-certificate -c mitmproxy /Library/Keychains/System.keychain` (their
terminal). Leave the CA installed otherwise; re-trusting it is the slow part.

## Safety and privacy

- A capture's file holds that app's traffic, unredacted — auth tokens, cookies, JWTs.
  Treat it as a secret; `down --wipe` deletes them all. Never paste tokens into a report or send a
  raw flow file to an external service; quote request shapes, not credentials.
- The hub captures only what Zero Omega (or the tunnel) routes to it, so what lands in the
  file is what the user chose to route. Never leave a WireGuard tunnel on after capturing — it
  routes the whole phone through the Mac.
- Read-only against the app. This skill captures traffic; it does not drive the app through
  transactions. Reaching a screen is the user's to do, or another skill's.
- Cert pinning defeats a proxy: a pinned app's TLS simply fails. That needs Frida/objection
  and is out of scope — report it as pinned rather than retrying.
- **iOS simulators are off-limits unless the user explicitly asks.** Do not add the CA to a
  simulator keychain to make a capture work.

## Reporting

When a capture is on a phone, the user is often on another device — send the WireGuard QR with
`SendUserFile` rather than only printing a path. `SendUserFile` is a deferred tool: load it with
`ToolSearch` ("select:SendUserFile") before calling it. State the target hosts, the flow file, and
for the findings give the endpoint shapes and WebSocket message formats — never the tokens.

