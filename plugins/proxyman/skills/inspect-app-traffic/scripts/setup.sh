#!/usr/bin/env bash
# ABOUTME: Detects whether this Mac is ready to capture traffic with mitmproxy, and
# ABOUTME: prints the exact remaining steps (install, CA generate, CA trust) if not.
#
# Idempotent state check. Reports JSON on stdout: whether mitmdump is installed, whether the
# mitmproxy CA exists, and whether that CA is trusted in the System keychain. Exit 0 means
# fully ready and setup can be skipped; exit 1 means one or more steps remain, each printed
# as a ready-to-run command. The one step that needs a real TTY (the sudo trust of the CA)
# is never run here — it is printed for the user to run, because sudo cannot read a password
# through this harness.

set -euo pipefail

CONFDIR="${MITMPROXY_CONFDIR:-$HOME/.mitmproxy}"
CA_PEM="$CONFDIR/mitmproxy-ca-cert.pem"

have_mitmdump=false
ca_present=false
ca_trusted=false
have_qrencode=false

command -v mitmdump >/dev/null 2>&1 && have_mitmdump=true
command -v qrencode >/dev/null 2>&1 && have_qrencode=true
[ -f "$CA_PEM" ] && ca_present=true

# The CA counts as trusted only if it is present in the System keychain. add-trusted-cert
# puts it there; a CA only in the login keychain does not gain system trust.
if $ca_present && security find-certificate -c mitmproxy /Library/Keychains/System.keychain >/dev/null 2>&1; then
  ca_trusted=true
fi

steps=()
$have_mitmdump || steps+=("brew install mitmproxy")

# Generate the CA by starting mitmdump once so it writes ~/.mitmproxy, then stopping it.
# Only possible once mitmdump is installed.
if $have_mitmdump && ! $ca_present; then
  steps+=("\"$0\" --generate-ca")
fi

if ! $ca_trusted; then
  steps+=("sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain \"$CA_PEM\"  # run in a real terminal; needs your password")
fi

# qrencode is not required — wg_config.py falls back to printing the config as text — but the
# WireGuard QR is the easy path, so recommend it. It does not gate readiness.
$have_qrencode || steps+=("brew install qrencode  # optional: for the WireGuard QR")

ready=false
$have_mitmdump && $ca_present && $ca_trusted && ready=true

# --generate-ca: start mitmdump briefly on a throwaway port to force CA generation, then stop.
if [ "${1:-}" = "--generate-ca" ]; then
  if ! command -v mitmdump >/dev/null 2>&1; then
    echo '{"error":"mitmdump not installed; run: brew install mitmproxy"}'
    exit 1
  fi
  mitmdump -q --listen-port 0 --set confdir="$CONFDIR" >/dev/null 2>&1 &
  pid=$!
  for _ in $(seq 1 50); do
    [ -f "$CA_PEM" ] && break
    sleep 0.1
  done
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" 2>/dev/null || true
  if [ -f "$CA_PEM" ]; then
    printf '{"generated":true,"ca":"%s"}\n' "$CA_PEM"
    exit 0
  fi
  echo '{"generated":false,"error":"mitmdump did not write the CA"}'
  exit 1
fi

# Emit the state report as JSON plus, on stderr, the human-readable next steps.
printf '{"ready":%s,"mitmdump":%s,"caPresent":%s,"caTrusted":%s,"qrencode":%s,"caPath":"%s"}\n' \
  "$ready" "$have_mitmdump" "$ca_present" "$ca_trusted" "$have_qrencode" "$CA_PEM"

if $ready; then
  echo "Ready. Existing setup detected — skip setup and go straight to capture." >&2
  $have_qrencode || echo "Note: qrencode is missing — 'brew install qrencode' for the WireGuard QR (optional)." >&2
  exit 0
fi

echo "Setup needed. Remaining steps, in order:" >&2
i=1
for s in "${steps[@]}"; do
  echo "  $i. $s" >&2
  i=$((i + 1))
done
echo "iPhone: the phone must also trust the CA, and this can only be done once a WireGuard" >&2
echo "  capture is running — with the tunnel on, open http://mitm.it in Safari (it is served" >&2
echo "  over the tunnel), install the profile, then Settings > General > About > Certificate" >&2
echo "  Trust Settings and toggle the mitmproxy CA on. It cannot be verified from the Mac." >&2
exit 1
