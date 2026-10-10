---
name: inspect-app-traffic
description: Capture and decode the HTTP and WebSocket traffic of one target web or mobile app with mitmproxy. Use when asked to see what API calls an app makes, intercept or sniff a web or iPhone app's requests, reverse-engineer an app's API or WebSocket protocol, or set up mitmproxy with Zero Omega. One shared proxy on port 8080; each capture is a filtered view of its stream.
---

# Inspect app traffic

One long-lived mitmdump, the hub, serves the HTTP proxy and, when asked, WireGuard on the fixed
port 8080, and every session and agent shares it. A website on this Mac reaches it through one
Zero Omega profile pointed at `127.0.0.1:8080`; an iPhone reaches it through one WireGuard tunnel.
A capture is a record, not a process: `start` notes a start time and the target hosts, and the hub
writes every matching flow into that capture's own file. Two agents capturing two apps at once
therefore share one proxy, one port and one Zero Omega profile, each reading only its own file.
Never start a second proxy.

## Skill directory

Set `SKILL_DIR` to the absolute directory of this loaded `SKILL.md` in every shell call, not the
caller's working directory or a host-specific plugin variable; if the loaded path is unavailable,
stop and report it. Shell variables may not persist between calls, so wherever this document writes
`$CAP` or `$PROXY_DATA_DIR`, type the actual value read from a previous command's JSON, or run the
whole sequence as one command.

```bash
SKILL_DIR="/absolute/path/to/loaded/skill"
```

Every script prints JSON on stdout; parse it and act on it.

## Environment variables

| Variable | Purpose | Required | Set in |
| --- | --- | --- | --- |
| `PROXY_DATA_DIR` | Root of the per-capture flow files (`cap/<capture>.mitm`); default `~/.local/share/proxy` | Optional | Shell environment |
| `PROXY_STATE_DIR` | Root of the running hub's state and the capture registry; default `~/.local/state/proxy` | Optional | Shell environment |
| `PROXY_CONFIG_DIR` | mitmproxy config directory holding the CA and `wireguard.conf`; default `~/.mitmproxy` | Optional | Shell environment |

## Setup

```bash
"$SKILL_DIR/scripts/setup.sh"
```

Exit 0 means mitmdump is installed and its CA is generated and trusted in the System keychain, so
skip the rest of setup. Exit 1 lists the remaining steps in order. You can run the CA-generation
step (`setup.sh --generate-ca`). The user must run the other two: `brew install mitmproxy`, which
needs the network, and the `sudo security add-trusted-cert …` line, because sudo cannot read a
password here; ask them to run it in a real terminal or type it here prefixed with `!`. Re-run
`setup.sh` and confirm exit 0 before capturing. A missing `qrencode` is reported but does not block
readiness; it only makes the WireGuard QR.

An iPhone must also trust the CA, once. With the WireGuard tunnel on, have the user open
`http://mitm.it` in Safari, install the profile, then turn the mitmproxy CA on in **Settings >
General > About > Certificate Trust Settings**. Without that, TLS interception fails on the phone.

## Start a capture

```bash
"$SKILL_DIR/scripts/capture.mjs" start --label myapp --hosts myapp.com
```

`start` brings the hub up if it is not running, detaches it and opens a capture; the `capture` id in
its JSON is `$CAP` for every later command. Run it normally, without a background-task flag, keep
the session alive for the capture, and confirm the hub is up with `status` or `check` before driving
the app. If the host cannot keep the detached process alive, report that and have the user run the
same command in a persistent terminal; do not keep launching replacement hubs.

`--hosts` scopes what the readers show; it does not filter interception, since the hub keeps
everything. List the app's domains; subdomains match automatically, so `myapp.com` also covers
`api.myapp.com` and `www.myapp.com`. Omit it to read every host in the capture's window. Add
`--wireguard` only for a phone capture.

### Mac website through Zero Omega

Point one Zero Omega profile at `127.0.0.1:8080` and enable it for the target site, either with an
auto-switch rule for the app's domains or for the whole browser while you work. Enabling it is a
manual step in the extension's UI: have the user do it before capturing. The rule must cover every
host the app calls, and the API often sits on a different subdomain from the page, so a rule for
the bare domain alone captures nothing; use a wildcard such as `*.myapp.com` or route the whole site.

Use the user's own logged-in Chrome, because the sites worth capturing are login-gated. Drive it
with the current host's connected Chrome tools after checking they can reach that profile, or have
the user click through the app. Never use a throwaway Chrome (`--user-data-dir`): it has no logins
and no Zero Omega profile. If the browser shows `NET::ERR_CERT_AUTHORITY_INVALID` or an HSTS block,
the CA is not trusted; go back to setup, since HSTS sites forbid clicking through.

### iPhone app through WireGuard

```bash
"$SKILL_DIR/scripts/capture.mjs" start --label myapp --hosts myapp.com --wireguard
"$SKILL_DIR/scripts/wg-config.mjs" --qr "$PROXY_DATA_DIR/myapp-qr.png"
```

`wg-config.mjs` prints the client config and writes the QR. Send the QR to the user (see Reporting)
and have them import it and turn the tunnel on. The whole phone routes through the Mac while the
tunnel is on, so `--hosts` is what scopes the read to the app. "Unable to create tunnel" on the
phone means the user declined the VPN permission. The phone must be on the same LAN as the Mac,
with the router's AP or client isolation off, or it cannot reach the Mac.

## Confirm traffic is arriving

Once the user has enabled the proxy and loaded the app, run `check` before investing in a drive:

```bash
"$SKILL_DIR/scripts/capture.mjs" check "$CAP"
```

It cannot see whether Zero Omega is on, only what reaches the hub:

- `clientsConnected` 0: nothing is routed; the proxy is not enabled or the phone tunnel is off.
- Connected, `tlsFailed` above 0 and no requests: the client rejects the mitmproxy certificate.
  The CA is not trusted (on the phone, install it via `mitm.it` and turn it on in Certificate Trust
  Settings) or the app pins its certificate.
- Connected, no requests and no TLS failures: the routing (the Zero Omega rule or the tunnel) is not
  sending the app's hosts; widen `--hosts` or the routing rule.
- Requests flowing: the capture works.

## Read the capture

```bash
"$SKILL_DIR/scripts/capture.mjs" read "$CAP" --kind flows     # one line per request
"$SKILL_DIR/scripts/capture.mjs" read "$CAP" --kind ws        # WebSocket frames; --wsmax <chars> widens them
"$SKILL_DIR/scripts/capture.mjs" read "$CAP" --kind hosts     # host tally
"$SKILL_DIR/scripts/capture.mjs" read "$CAP" --kind origins   # callers of each host
```

For a request or response body, read the capture's file with mitmproxy's flow language:

```bash
mitmdump -q -nr "$PROXY_DATA_DIR/cap/$CAP.mitm" '~u /api/trade & ~s' --set flow_detail=3 2>/dev/null
```

A WebSocket flow is written to the file only when it closes, so read its frames after the socket
ends or after `down`. Skip or truncate a base64 or binary body or frame rather than printing it,
because a large base64 blob in tool output can trigger a false safety refusal.

When two apps share a host, such as an auth provider, RPC or analytics host, its flows land in both
captures and `--hosts` cannot separate them. Split them by caller instead: each web app sends its own
`Origin` or `Referer`, and some shared auth providers add a per-app id header.

```bash
"$SKILL_DIR/scripts/capture.mjs" read "$CAP" --kind origins                         # list callers
"$SKILL_DIR/scripts/capture.mjs" read "$CAP" --kind origins --source app-a.example  # one app's calls
```

## Close a capture and stop the hub

```bash
"$SKILL_DIR/scripts/capture.mjs" status               # the hub and the open captures
"$SKILL_DIR/scripts/capture.mjs" stop "$CAP" --wipe   # close a capture and delete its file
"$SKILL_DIR/scripts/capture.mjs" down --wipe          # stop the hub and delete all capture files
```

Capture files hold unredacted tokens and are never cleaned automatically, so leave nothing behind:
run `stop "$CAP" --wipe` once you have read what you need (plain `stop` keeps the file for a later
read), leave the hub running while any capture is open, and end the session with `down --wipe`.
After a WireGuard capture, tell the user to turn the tunnel off. Leave the CA installed, because
re-trusting it is the slow part; only if the user asks to undo setup entirely, have them run
`sudo security delete-certificate -c mitmproxy /Library/Keychains/System.keychain` in their terminal.

## Safety and privacy

- A capture's file holds the app's auth tokens, cookies and JWTs; treat it as a secret. Never paste
  tokens into a report or send a raw flow file to an external service; quote request shapes, not
  credentials.
- The hub sees only what Zero Omega or the tunnel routes to it, so the file holds what the user
  chose to route. Never leave a WireGuard tunnel on after capturing.
- Stay read-only against the app: capture traffic, do not drive the app through transactions.
  Reaching a screen is the user's job or another skill's.
- Certificate pinning defeats a proxy, and a pinned app's TLS simply fails. Bypassing it needs
  Frida or objection and is out of scope; report the app as pinned rather than retrying.

## Reporting

For a phone capture the user is often on another device, so deliver the WireGuard QR with the
current host's image or file-delivery tool. In Claude, load `SendUserFile` with `ToolSearch`
(`select:SendUserFile`) if it is deferred; in Codex, use the local-image display tool and a
clickable file link. If delivery is unavailable, say so and give the local paths of the QR and the
client config. Report the target hosts and the flow file, and give findings as endpoint shapes and
WebSocket message formats, never tokens.
