// ABOUTME: Tests the X25519 public-key derivation in wg-config.mjs against a known mitmproxy
// ABOUTME: key pair and the RFC 7748 section 6.1 vector, with no `wg` tool or npm package.
import { describe, test } from "node:test";
import assert from "node:assert/strict";
import { publicKey } from "../scripts/wg-config.mjs";

describe("publicKey", () => {
  test("derives the public key of a real mitmproxy server_key", () => {
    // A real mitmproxy wireguard.conf server_key and the public key it must derive to.
    assert.equal(
      publicKey("a879Q4ZT38XWOH5ytymSgpdTNG6+COW9qQpgUhz11ao="),
      "RLenMqaBPEuRiMSej4/yJICN8/6KsBzuW+Z/rDFrSAQ=");
  });

  test("matches the RFC 7748 section 6.1 vector", () => {
    // RFC 7748 6.1: Alice's private key -> Alice's public key.
    const priv = Buffer.from("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a", "hex");
    const pub = Buffer.from("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a", "hex");
    assert.equal(publicKey(priv.toString("base64")), pub.toString("base64"));
  });
});
