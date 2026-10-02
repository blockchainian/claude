// ABOUTME: Pure helpers behind the session restriction command: JWT liveness + probe decision.
// ABOUTME: No browser, no network; the browser flow that feeds these lives in login.mjs.

// Unix `exp` from a JWT payload, or null if the token has no readable integer exp.
export function jwtExp(token) {
  const parts = token.split(".");
  if (parts.length < 2) return null;
  let data;
  try {
    data = JSON.parse(Buffer.from(parts[1], "base64url").toString("utf8"));
  } catch {
    return null;
  }
  const exp = data?.exp;
  return Number.isInteger(exp) ? exp : null;
}

// Strip session's JSON-quoting from a localStorage value (`"\"tok\""` -> `tok`). session stores its
// tokens JSON-encoded, so a raw localStorage read is a quoted string. Returns null for an
// empty/absent value, the plain string otherwise.
export function unwrapJsonQuoted(raw) {
  if (!raw || typeof raw !== "string") return null;
  raw = raw.trim();
  if (raw.startsWith('"') && raw.endsWith('"')) {
    try {
      return JSON.parse(raw);
    } catch {
      return raw.replace(/^"+|"+$/g, "");
    }
  }
  return raw || null;
}

// True if the token's exp is at least `skewSeconds` in the future.
export function tokenLive(token, now = null, skewSeconds = 60) {
  if (!token) return false;
  const exp = jwtExp(token);
  if (exp === null) return false;
  const reference = now ?? Math.floor(Date.now() / 1000);
  return exp > reference + skewSeconds;
}

// True if a JSON-quoted stored token's `exp` is still in the future by `skewSeconds`.
export function storedTokenLive(raw, now = null, skewSeconds = 60) {
  return tokenLive(unwrapJsonQuoted(raw) || "", now, skewSeconds);
}

// Classify a `/v2/users/current` probe result (called with a live token) into a bucket:
// - `restricted`: 200 and `isRestricted` is exactly true → mark the session blocked.
// - `ok`: 200 and the flag is false or absent → leave the session as it is. A missing flag is
//   never synthesized as restricted.
// - `forbidden`: 403 → the app API forbids this user even with a valid token — the account is
//   banned (a fresh full re-login still gets 403 while the session token mints fine). Also blocked.
// - `dead_token`: 401 → the token itself was rejected.
// - `error`: anything else (404, 5xx, no response, malformed body) → inconclusive.
export function restrictionDecision(status, body) {
  if (status === 200) {
    if (body && typeof body === "object" && body.isRestricted === true) {
      return ["restricted", "isRestricted=true"];
    }
    return ["ok", "isRestricted not set"];
  }
  if (status === 403) return ["forbidden", "HTTP 403 Forbidden (account banned)"];
  if (status === 401) return ["dead_token", "HTTP 401"];
  return ["error", status != null ? `HTTP ${status}` : "no response"];
}
