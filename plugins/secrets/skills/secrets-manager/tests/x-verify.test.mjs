// ABOUTME: Tests for the X account check: how a GraphQL Viewer response maps to active, expired,
// ABOUTME: restricted or escalated, and which responses are left inconclusive (thrown).

import { test } from "node:test";
import assert from "node:assert/strict";

import { classifyViewer, viewerUrl } from "../scripts/x-verify.mjs";

// Shapes below are trimmed from live Viewer responses (2026-10-04): a signed-in account, and a
// made-up auth_token, which X answers with HTTP 200, no user_results and auth errors 215/32.
const signedIn = (screen_name) => ({
  data: { viewer: { is_tfe_restricted_session: false, user_results: { result: { __typename: "User", core: { name: "H", screen_name } } } } },
});
const authError = (code) => ({ code, kind: "Permissions", name: "AuthenticationError", message: "Authentication: Not authenticated" });
const notAuthenticated = {
  data: { viewer: { is_tfe_restricted_session: false } },
  errors: [authError(215), authError(32)],
};

test("a signed-in viewer whose handle matches the row is active", () => {
  assert.equal(classifyViewer({ status: 200, body: signedIn("Zoe86zz5") }, "zoe86zz5"), "active");
});

test("a token X does not authenticate is expired", () => {
  assert.equal(classifyViewer({ status: 200, body: notAuthenticated }, "zoe86zz5"), "expired");
  assert.equal(classifyViewer({ status: 401, body: { errors: [{ code: 89, message: "Invalid or expired token." }] } }, "zoe"), "expired");
});

test("a suspended account is restricted and a locked one is escalated, whatever the HTTP status", () => {
  assert.equal(classifyViewer({ status: 403, body: { errors: [{ code: 64, message: "Your account is suspended" }] } }, "zoe"), "restricted");
  assert.equal(classifyViewer({ status: 200, body: { ...signedIn("zoe"), errors: [{ code: 326, message: "locked" }] } }, "zoe"), "escalated");
  const unavailable = { data: { viewer: { user_results: { result: { __typename: "UserUnavailable", reason: "Suspended" } } } } };
  assert.equal(classifyViewer({ status: 200, body: unavailable }, "zoe"), "restricted");
});

test("responses that do not tell are thrown, not guessed", () => {
  assert.throws(() => classifyViewer({ status: 404, body: null }, "zoe"), /SECRETS_X_VIEWER_QUERY_ID/);
  assert.throws(() => classifyViewer({ status: 429, body: null }, "zoe"), /HTTP 429/);
  assert.throws(() => classifyViewer({ status: 200, body: signedIn("someoneelse") }, "zoe"), /@someoneelse/);
  assert.throws(() => classifyViewer({ status: 200, body: { data: {} } }, "zoe"), /inconclusive/);
});

test("viewerUrl targets the Viewer operation of the configured queryId", () => {
  const url = new URL(viewerUrl("QID123"));
  assert.equal(url.origin + url.pathname, "https://x.com/i/api/graphql/QID123/Viewer");
  assert.ok(url.searchParams.get("variables"));
});
