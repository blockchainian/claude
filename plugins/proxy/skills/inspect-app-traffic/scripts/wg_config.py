#!/usr/bin/env python3
"""Emit the WireGuard client config (and a QR) for a mitmproxy wireguard-mode capture.

mitmproxy writes ~/.mitmproxy/wireguard.conf with a server_key and a client_key when it
first starts in wireguard mode. This turns those into a config the phone's WireGuard app
imports: the client_key is the phone's PrivateKey, the server's public key is derived from
server_key, AllowedIPs is 0.0.0.0/0 so the whole phone routes through the tunnel, and the
Endpoint is this Mac on the LAN.

The X25519 public-key derivation is done here in pure Python on purpose: the system Python
has no `cryptography` module and the `wg` tool is not always installed, and one small,
dependency-free routine is simpler than requiring either. AllowedIPs of 0.0.0.0/0 means the
phone sends ALL its traffic here while the tunnel is on — pair it with a host filter on the
capture so unrelated traffic is passed through and never saved, and tell the user to turn
the tunnel off when done.

Prints the config text on stdout. With --qr and qrencode installed, also writes a PNG.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import subprocess
import sys
from pathlib import Path

P = 2**255 - 19
A24 = 121665


def _decode_scalar(b: bytes) -> int:
    a = bytearray(b)
    a[0] &= 248
    a[31] &= 127
    a[31] |= 64
    return int.from_bytes(a, "little")


def _cswap(swap: int, a: int, b: int) -> tuple[int, int]:
    dummy = (-swap) & ((1 << 255) - 1) & (a ^ b)
    return a ^ dummy, b ^ dummy


def x25519(scalar: bytes, u_bytes: bytes) -> bytes:
    """RFC 7748 Montgomery ladder scalar multiplication on Curve25519."""
    k = _decode_scalar(scalar)
    u = int.from_bytes(u_bytes, "little") % P
    x2, z2, x3, z3, swap = 1, 0, u, 1, 0
    for t in reversed(range(255)):
        kt = (k >> t) & 1
        swap ^= kt
        x2, x3 = _cswap(swap, x2, x3)
        z2, z3 = _cswap(swap, z2, z3)
        swap = kt
        aa = (x2 + z2) % P
        bb = (x2 - z2) % P
        cc = (x3 + z3) % P
        dd = (x3 - z3) % P
        da = (dd * aa) % P
        cb = (cc * bb) % P
        x3 = pow((da + cb) % P, 2, P)
        z3 = (u * pow((da - cb) % P, 2, P)) % P
        aa2 = pow(aa, 2, P)
        bb2 = pow(bb, 2, P)
        x2 = (aa2 * bb2) % P
        e = (aa2 - bb2) % P
        z2 = (e * ((aa2 + A24 * e) % P)) % P
    x2, x3 = _cswap(swap, x2, x3)
    z2, z3 = _cswap(swap, z2, z3)
    return (x2 * pow(z2, P - 2, P) % P).to_bytes(32, "little")


def public_key(private_b64: str) -> str:
    priv = base64.b64decode(private_b64)
    pub = x25519(priv, (9).to_bytes(32, "little"))
    return base64.b64encode(pub).decode()


def lan_ip() -> str:
    try:
        iface = subprocess.check_output(["route", "-n", "get", "default"], text=True)
        m = re.search(r"interface:\s*(\S+)", iface)
        if m:
            ip = subprocess.check_output(["ipconfig", "getifaddr", m.group(1)], text=True).strip()
            if ip:
                return ip
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass
    return "<your-mac-lan-ip>"


def build_config(conf_path: Path, endpoint: str) -> str:
    data = json.loads(conf_path.read_text())
    client_priv = data["client_key"]
    server_pub = public_key(data["server_key"])
    return "\n".join([
        "[Interface]",
        f"PrivateKey = {client_priv}",
        "Address = 10.0.0.1/32",
        "DNS = 10.0.0.53",
        "",
        "[Peer]",
        f"PublicKey = {server_pub}",
        "AllowedIPs = 0.0.0.0/0, ::/0",
        f"Endpoint = {endpoint}",
        "",
    ])


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--conf", default=os.path.expanduser(
        os.environ.get("MITMPROXY_CONFDIR", "~/.mitmproxy") + "/wireguard.conf"),
        help="path to mitmproxy's wireguard.conf")
    ap.add_argument("--endpoint", help="host:port the phone connects to; default is this Mac's LAN IP on 51820")
    ap.add_argument("--qr", help="write a QR PNG to this path")
    args = ap.parse_args()

    conf = Path(args.conf)
    if not conf.exists():
        print(f"{conf} not found — start a wireguard-mode capture first so mitmproxy writes it",
              file=sys.stderr)
        return 1

    endpoint = args.endpoint or f"{lan_ip()}:51820"
    config = build_config(conf, endpoint)
    print(config)

    if args.qr:
        try:
            subprocess.run(["qrencode", "-t", "PNG", "-s", "12", "-m", "4", "-l", "L",
                            "-o", args.qr], input=config, text=True, check=True)
            print(f"# QR written to {args.qr}", file=sys.stderr)
        except (subprocess.CalledProcessError, FileNotFoundError):
            print("# qrencode unavailable; import the text config manually "
                  "(brew install qrencode for a QR)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
