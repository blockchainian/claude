// ABOUTME: Tests for the kit's ISP-pool HTTP helper: which pool slot a request leaves from and that
// ABOUTME: it never runs without the pool configured.

import { test } from "node:test";
import assert from "node:assert/strict";

import { ispSlotUrl } from "../scripts/http.mjs";
import { kit } from "../scripts/adapter.mjs";

const env = { ISP_PROXY_URL: "http://user:pass@isp.example:10000", ISP_PROXY_COUNT: "10" };

test("a request leaves from one of the pool's slots 1..ISP_PROXY_COUNT", () => {
  assert.equal(ispSlotUrl(env, () => 0), "http://user:pass@isp.example:10001");
  assert.equal(ispSlotUrl(env, () => 0.999), "http://user:pass@isp.example:10010");
  assert.equal(ispSlotUrl({ ISP_PROXY_URL: env.ISP_PROXY_URL }, () => 0.5), "http://user:pass@isp.example:10001");
});

test("without ISP_PROXY_URL nothing is sent", () => {
  assert.throws(() => ispSlotUrl({}), /ISP_PROXY_URL/);
});

test("adapters reach the helper through the kit", () => {
  assert.equal(typeof kit.ispFetch, "function");
});
