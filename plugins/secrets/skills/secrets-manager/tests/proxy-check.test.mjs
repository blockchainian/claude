// ABOUTME: Tests the proxy exit check run before a browser opens: a working tunnel passes whatever
// ABOUTME: the target answers, a dead exit throws ProxyExitDown without leaking the proxy credentials.

import { test } from "node:test";
import assert from "node:assert/strict";

import { assertExitUp, ProxyExitDown } from "../scripts/proxy-check.mjs";

const PROXY = "http://customer-abc-sessid-123:secretpw@proxy.example:7777";

test("an exit that tunnels to the probe target passes, whatever status the target answers", async () => {
  const seen = [];
  await assertExitUp(PROXY, { fetchImpl: async (url, init) => (seen.push([url, Boolean(init.dispatcher)]), new Response(null, { status: 204 })) });
  assert.deepEqual(seen, [["https://www.gstatic.com/generate_204", true]]);
});

test("an exit that fails every attempt throws ProxyExitDown naming the host, never the password", async () => {
  let calls = 0;
  const fetchImpl = async () => {
    calls += 1;
    throw new TypeError("fetch failed", { cause: new Error("Proxy response (522) !== 200 when HTTP Tunneling") });
  };
  const err = await assertExitUp(PROXY, { fetchImpl }).catch((e) => e);
  assert.ok(err instanceof ProxyExitDown);
  assert.equal(calls, 2);
  assert.match(err.message, /proxy exit down/);
  assert.match(err.message, /proxy\.example:7777/);
  assert.match(err.message, /522/);
  assert.doesNotMatch(err.message, /secretpw|customer-abc/);
});

test("one failed attempt followed by a good one passes", async () => {
  let calls = 0;
  await assertExitUp(PROXY, { fetchImpl: async () => { if (++calls === 1) throw new TypeError("fetch failed"); return new Response("ok"); } });
  assert.equal(calls, 2);
});
