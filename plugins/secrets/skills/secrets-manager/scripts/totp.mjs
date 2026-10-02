// ABOUTME: Computes the current TOTP code from an account's shared secret (RFC 6238).
// ABOUTME: SHA1, 30-second step, 6 digits — the Google Authenticator defaults; node:crypto only.

import { createHmac } from "node:crypto";

function base32Decode(secret) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  const clean = secret.replace(/=+$/, "").toUpperCase().replace(/\s+/g, "");
  const out = [];
  let bits = 0;
  let value = 0;
  for (const char of clean) {
    const idx = alphabet.indexOf(char);
    if (idx === -1) continue;
    value = (value << 5) | idx;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((value >>> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

// The 6-digit TOTP for this secret, optionally at a fixed Unix time for tests.
export function computeTotp(secret, forTime = null, { step = 30, digits = 6 } = {}) {
  const t = forTime ?? Math.floor(Date.now() / 1000);
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(t / step)));
  const hmac = createHmac("sha1", base32Decode(secret)).update(counter).digest();
  const offset = hmac[hmac.length - 1] & 0x0f;
  const bin =
    ((hmac[offset] & 0x7f) << 24) |
    ((hmac[offset + 1] & 0xff) << 16) |
    ((hmac[offset + 2] & 0xff) << 8) |
    (hmac[offset + 3] & 0xff);
  return String(bin % 10 ** digits).padStart(digits, "0");
}
