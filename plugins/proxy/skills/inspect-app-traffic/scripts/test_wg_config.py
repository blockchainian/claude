#!/usr/bin/env python3
"""Tests for the pure-Python X25519 public-key derivation in wg_config.py.

Pins the derivation against a known mitmproxy key pair and against the RFC 7748 section 6.1
test vector, so a regression in the Montgomery ladder is caught without needing the
`cryptography` module or `wg` installed.
"""

import base64
import importlib.util
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "wg_config", Path(__file__).with_name("wg_config.py"))
assert _spec and _spec.loader
wg = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wg)


class PublicKey(unittest.TestCase):
    def test_known_mitmproxy_pair(self):
        # A real mitmproxy wireguard.conf server_key and the public key it must derive to.
        self.assertEqual(
            wg.public_key("a879Q4ZT38XWOH5ytymSgpdTNG6+COW9qQpgUhz11ao="),
            "RLenMqaBPEuRiMSej4/yJICN8/6KsBzuW+Z/rDFrSAQ=")

    def test_rfc7748_vector(self):
        # RFC 7748 6.1: Alice's private key -> Alice's public key.
        priv = bytes.fromhex(
            "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
        pub = bytes.fromhex(
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
        self.assertEqual(wg.public_key(base64.b64encode(priv).decode()),
                         base64.b64encode(pub).decode())


if __name__ == "__main__":
    unittest.main()
