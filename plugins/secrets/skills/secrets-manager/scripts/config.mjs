// ABOUTME: Resolves the local state directory and proxy the skill reads and writes on the Mac.
// ABOUTME: Everything lives under SECRETS_DATA_DIR (default ~/.config/secrets-manager).

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";


// Populate process.env from a KEY=VALUE file without overriding what is already set.
export function loadEnvFile(path) {
  if (existsSync(path)) process.loadEnvFile(path);
}

// ~/.config/secrets-manager/.env carries RESIDENTIAL_PROXY_URL so every run goes through the proxy; it is outside the repository.
loadEnvFile(join(homedir(), ".config/secrets-manager/.env"));

function expandUser(p) {
  return p.startsWith("~") ? join(homedir(), p.slice(1)) : resolve(p);
}

// The root of all local state: the store, credential files, browser profiles and debug captures.
export function statePath() {
  return expandUser(process.env.SECRETS_DATA_DIR || "~/.config/secrets-manager");
}

// The SQLite file holding the Google accounts, every app's sessions and the x table.
export const dbPath = () => join(statePath(), "secrets.sqlite");

// Credential files for one app (`google/*.txt`, `x/*.txt`), one account per line.
export const credentialsDir = (app) => join(statePath(), app);

// Each account's persistent Camoufox profile (the "device" the site sees); one sub-dir per key.
export const profileDirFor = (key) => join(statePath(), "profiles", key);

// Where a failed login step dumps a screenshot and page state for inspection.
export const debugDir = () => join(statePath(), "debug");

// Residential proxy applied to every account.
export function defaultProxy() {
  return process.env.RESIDENTIAL_PROXY_URL || null;
}

// HeroSMS API key for renting phone numbers to receive Google's verification SMS (~/.config/secrets-manager/.env).
export function heroSmsKey() {
  return process.env.HERO_SMS_API_KEY || null;
}

// HeroSMS country ids that must never be rented for a Google-verification number, whatever the price.
// Google accepts a Cameroon (41) number but never delivers the code, and an Indonesia (6) number did
// not deliver either — renting one only burns a rent and a poll cycle. A Philippines (4) and a Kenya
// (8) number each made Google escalate the whole sign-in to a scan-a-QR device check on a fresh
// account, so both are denied too (the QR escalation looks session/IP-wide rather than country-
// specific — every rented number so far triggered it — so these are denied to take them out of the
// picture while the cause is chased elsewhere). The phone step and the `sms` command both filter their
// affordable-country list through this, so one can never be picked even as the cheapest.
export const SMS_COUNTRY_BLACKLIST = [41, 6, 4, 8];

const STICKY_SESSTIME_MIN = 10;

// Pin one account to a sticky exit for proxies that take `sessid`/`sesstime` in the username.
// The supported convention uses a `customer-` prefix; a hash of the account key selects its
// session for ten minutes. `rotate` returns the base URL. Other username conventions and
// usernames already carrying a sessid pass through unchanged.
export function proxyFor(key, { rotate = false } = {}) {
  const base = defaultProxy();
  if (!base) return null;
  if (rotate) return base;
  const parsed = new URL(base);
  const user = decodeURIComponent(parsed.username || "");
  if (!user.startsWith("customer-") || user.includes("sessid")) return base;
  const sessid = createHash("sha256").update(key).digest("hex").slice(0, 12);
  const newUser = `${user}-sessid-${sessid}-sesstime-${STICKY_SESSTIME_MIN}`;
  let netloc = `${newUser}:${decodeURIComponent(parsed.password)}@${parsed.hostname}`;
  if (parsed.port) netloc += `:${parsed.port}`;
  return `${parsed.protocol}//${netloc}`;
}

export const configJsonPath = () => join(homedir(), ".config/secrets-manager/config.json");

export function ispProxyAt(baseUrl, slot) {
  const u = new URL(baseUrl);
  u.port = String(Number(u.port) + slot);
  return u.toString().replace(/\/$/, "");
}
