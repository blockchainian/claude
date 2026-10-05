// ABOUTME: The X account check behind `verify x`: one GraphQL Viewer call with the stored
// ABOUTME: auth_token + ct0 through the account's residential exit, mapped to an account status.
import { fetch } from "undici";
import { dispatcherFor } from "./proxy-check.mjs";
import * as config from "./config.mjs";

const UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const EXPIRED_CODES = new Set([32, 89, 215]); // not authenticated / invalid or expired token
const SUSPENDED = 64;
const LOCKED = 326;

function requireEnv(key) {
  const value = process.env[key];
  if (!value) throw new Error(`${key} is required in ~/.config/secrets-manager/.env`);
  return value;
}

export function viewerUrl(queryId) {
  const variables = encodeURIComponent(JSON.stringify({ withCommunitiesMemberships: true }));
  return `https://x.com/i/api/graphql/${queryId}/Viewer?variables=${variables}&features=%7B%7D`;
}

// Maps one Viewer response to a status for the row's `username`. X answers a dead token with HTTP 200
// and auth errors, and a suspension or lock with an error code at any HTTP status, so the error codes
// are read first. Anything that does not say which account this is, or whether it works, throws.
export function classifyViewer({ status, body }, username) {
  const codes = new Set((body?.errors ?? []).map((e) => e.code));
  if (codes.has(SUSPENDED)) return "restricted";
  if (codes.has(LOCKED)) return "escalated";
  if (status === 404) throw new Error("Viewer answered 404: X_VIEWER_QUERY_ID is stale, re-extract it from x.com's main.js");
  if (status !== 200 && !codes.size) throw new Error(`Viewer inconclusive: HTTP ${status}`);
  const user = body?.data?.viewer?.user_results?.result;
  if (user?.__typename === "UserUnavailable" && /suspend/i.test(user.reason ?? "")) return "restricted";
  const handle = user?.core?.screen_name ?? user?.legacy?.screen_name;
  if (handle) {
    if (handle.toLowerCase() !== username.toLowerCase()) throw new Error(`stored token signs in as @${handle}, not @${username}`);
    return "active";
  }
  if ([...codes].some((c) => EXPIRED_CODES.has(c))) return "expired";
  throw new Error(`Viewer inconclusive: HTTP ${status}, error codes ${[...codes].join(",") || "none"}`);
}

function xDispatcher(proxyUrl) {
  if (!proxyUrl) throw new Error("RESIDENTIAL_PROXY_URL is required; X is never called from the home IP");
  return dispatcherFor(proxyUrl);
}

// Checks one `x` row. A row without a token pair has no session to check, so it is expired.
export async function checkX({ row, opts = {} }) {
  if (!row.auth_token || !row.ct0) return "expired";
  const url = viewerUrl(requireEnv("X_VIEWER_QUERY_ID"));
  const dispatcher = xDispatcher(config.proxyFor(row.username, { rotate: opts["rotate-proxy"] }));
  const res = await fetch(url, {
    dispatcher,
    headers: {
      authorization: `Bearer ${requireEnv("X_BEARER")}`,
      "x-csrf-token": row.ct0,
      cookie: `auth_token=${row.auth_token}; ct0=${row.ct0}`,
      "x-twitter-auth-type": "OAuth2Session",
      "x-twitter-active-user": "yes",
      "User-Agent": UA,
    },
  });
  const text = await res.text();
  let body = null;
  try {
    body = JSON.parse(text);
  } catch {
    /* not JSON: classified by status alone */
  }
  return classifyViewer({ status: res.status, body }, row.username);
}
