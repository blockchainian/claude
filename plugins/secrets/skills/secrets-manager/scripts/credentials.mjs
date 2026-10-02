// ABOUTME: Reads the colon-separated Google credential files the user drops into the google dir.
// ABOUTME: One account per line; sync logs in whatever accounts it finds that need it.

import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const clean = (s) => (s && s.trim()) || null;

// Parse one credential line into `{email, password, totp_secret, app_password}`, or null if blank.
// Two shapes are accepted:
//   1. Positional `email:password:totp_secret:app_password` (last two optional; trailing empty = none).
//   2. A "2FA-as-a-service" vendor drop that embeds the TOTP seed in a URL and carries extra fields
//      (recovery email, an inbox-viewer URL) with no store column, e.g.
//      `email:password:recovery@x:http://host/view/<id>:https://2fa.fb.tools/<BASE32>`.
// The vendor shape is detected by its TOTP URL and takes only email, password and the base32 seed;
// add another `TOTP_URL_PATTERNS` entry to support a new vendor's URL. Throws on a line with no password.
const TOTP_URL_PATTERNS = [/2fa\.fb\.tools\/([A-Za-z2-7]{16,})/];
export function parseLine(line) {
  line = line.trim();
  if (!line) return null;
  for (const pattern of TOTP_URL_PATTERNS) {
    const seed = line.match(pattern);
    if (!seed) continue;
    const [email, password] = line.split(":");
    if (!email?.trim() || !password?.trim()) {
      throw new Error(`line is missing email or password: ${JSON.stringify(line)}`);
    }
    return { email: email.trim(), password: password.trim(), totp_secret: seed[1], app_password: null };
  }
  const parts = line.split(":");
  if (parts.length < 2) throw new Error(`line has no password: ${JSON.stringify(parts[0])}`);
  const email = parts[0].trim();
  const password = parts[1].trim();
  if (!email || !password) throw new Error(`line is missing email or password: ${JSON.stringify(line)}`);
  return {
    email,
    password,
    totp_secret: clean(parts[2]),
    app_password: clean(parts[3]), // Google app password (16 chars); not used by web login
  };
}

function credentialFiles(dir) {
  return readdirSync(dir)
    .filter((f) => f.endsWith(".txt"))
    .sort()
    .map((f) => join(dir, f));
}

// Write `appPassword` into field 4 of the credential line for `email`. Rewrites in place the *.txt
// file that holds the account, preserving the email, password and TOTP fields and every other
// line. A line shorter than four fields is padded so the app password lands in the right column.
// Returns true when a line was updated, false when no file carries the email.
export function setAppPassword(googleDir, email, appPassword) {
  for (const path of credentialFiles(googleDir)) {
    const lines = readFileSync(path, "utf8").split(/\r?\n/);
    if (lines.at(-1) === "") lines.pop();
    let changed = false;
    lines.forEach((raw, i) => {
      if (!raw.trim()) return;
      const parts = raw.split(":");
      if (parts[0].trim() !== email) return;
      while (parts.length < 4) parts.push("");
      parts[3] = appPassword;
      lines[i] = parts.slice(0, 4).join(":");
      changed = true;
    });
    if (changed) {
      writeFileSync(path, lines.join("\n") + "\n");
      return true;
    }
  }
  return false;
}

// Write `totpSecret` into field 3 of the credential line for `email`, the mirror of
// `setAppPassword`. Pads a shorter line so the secret lands in the right column and keeps any
// existing app password in field 4. Returns true when a line was updated, false when no file
// carries the email.
export function setTotpSecret(googleDir, email, totpSecret) {
  for (const path of credentialFiles(googleDir)) {
    const lines = readFileSync(path, "utf8").split(/\r?\n/);
    if (lines.at(-1) === "") lines.pop();
    let changed = false;
    lines.forEach((raw, i) => {
      if (!raw.trim()) return;
      const parts = raw.split(":");
      if (parts[0].trim() !== email) return;
      while (parts.length < 3) parts.push("");
      parts[2] = totpSecret;
      lines[i] = parts.slice(0, 4).join(":");
      changed = true;
    });
    if (changed) {
      writeFileSync(path, lines.join("\n") + "\n");
      return true;
    }
  }
  return false;
}

// Parse every account line in every *.txt file in the directory, sorted by filename. A missing
// directory yields no credentials; a malformed line throws, naming its file and line number, so a
// typo is loud rather than a silently skipped account. A later file's account overrides an
// earlier one with the same email.
export function loadCredentials(googleDir) {
  if (!existsSync(googleDir)) return [];
  const byEmail = new Map();
  for (const path of credentialFiles(googleDir)) {
    const lines = readFileSync(path, "utf8").split(/\r?\n/);
    lines.forEach((raw, i) => {
      let cred;
      try {
        cred = parseLine(raw);
      } catch (e) {
        throw new Error(`${path.split("/").pop()}:${i + 1}: ${e.message}`);
      }
      if (cred) byEmail.set(cred.email, cred);
    });
  }
  return [...byEmail.values()];
}
