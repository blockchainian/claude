// ABOUTME: Tests the TikTok login's browser-free helpers: the session predicate over a cookie jar
// ABOUTME: and the ISP slot a login is bound to. No browser, no network.

import { test } from "node:test";
import assert from "node:assert/strict";

import { PassThrough } from "node:stream";

import { sessionCookies, loginProxy, promptEmailCode } from "../scripts/tiktok-login.mjs";

const cookie = (name, domain, value = "v") => ({ name, value, domain, path: "/" });

test("sessionCookies returns the tiktok.com cookies once a sessionid is among them", () => {
  const jar = [cookie("sessionid", ".tiktok.com"), cookie("msToken", "www.tiktok.com"), cookie("NID", ".google.com")];
  assert.deepEqual(sessionCookies(jar).map((c) => c.name), ["sessionid", "msToken"]);
});

test("sessionCookies is null without a sessionid, or with an empty one", () => {
  assert.equal(sessionCookies([cookie("msToken", ".tiktok.com")]), null);
  assert.equal(sessionCookies([cookie("sessionid", ".tiktok.com", "")]), null);
  assert.equal(sessionCookies([cookie("sessionid", ".example.com")]), null);
});

test("loginProxy binds a login to the pool's last slot, or the slot the account already has", () => {
  const env = { ISP_PROXY_URL: "http://u:p@isp.example:8000", ISP_PROXY_COUNT: "10" };
  assert.deepEqual(loginProxy(env), { slot: 10, url: "http://u:p@isp.example:8010" });
  assert.deepEqual(loginProxy(env, 3), { slot: 3, url: "http://u:p@isp.example:8003" });
  assert.deepEqual(loginProxy({ ISP_PROXY_URL: "http://isp.example:8000" }), { slot: 1, url: "http://isp.example:8001" });
});

test("loginProxy refuses to run without the ISP pool", () => {
  assert.throws(() => loginProxy({}), /ISP_PROXY_URL/);
});

test("the code prompt answers with the typed code, and with nothing once it is called off", async () => {
  const typed = new PassThrough();
  const answered = promptEmailCode("bob1", undefined, { input: typed, output: new PassThrough() });
  typed.write(" 123456 \n");
  assert.equal(await answered, "123456");

  const off = new AbortController();
  const waiting = promptEmailCode("bob1", off.signal, { input: new PassThrough(), output: new PassThrough() });
  off.abort();
  assert.equal(await waiting, null);
});
