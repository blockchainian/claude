// ABOUTME: Tests TikTok login helpers and code submission with a minimal fake browser page.
// ABOUTME: Covers session cookies, ISP slots, and terminal/headed codes. No browser, no network.

import { test } from "node:test";
import assert from "node:assert/strict";

import { PassThrough } from "node:stream";

import { sessionCookies, loginProxy, promptEmailCode, signInTiktok } from "../scripts/tiktok-login.mjs";

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

function codeDialog(values) {
  let poll = 0;
  const fields = new Map();
  const clicks = [];
  const codeField = {
    isVisible: async () => true,
    inputValue: async () => values[poll],
    click: async () => {},
  };
  const page = {
    goto: async () => {},
    fill: async (selector, value) => fields.set(selector, value),
    inputValue: async (selector) => fields.get(selector) ?? "",
    isEnabled: async () => true,
    click: async () => clicks.push("credentials"),
    locator: () => ({ first: () => codeField }),
    keyboard: { type: async (value) => { values[poll] = value; } },
    getByRole: (role, { name }) => ({ last: () => ({
      isVisible: async () => name === "Verify",
      click: async () => clicks.push({ name, code: values[poll] }),
    }) }),
    waitForTimeout: async (ms) => { if (ms === 1500) poll++; },
  };
  const context = {
    cookies: async () => poll >= values.length ? [cookie("sessionid", ".tiktok.com")] : [],
  };
  return { context, page, clicks };
}

function credentialForm({ values = ["", ""], enabled = false, wipe = false, unreadable = false } = {}) {
  const fake = codeDialog(["", "", "", ""]);
  const selectors = ["input[name='username']", "input[type='password']"];
  const fields = new Map(selectors.map((selector, i) => [selector, values[i]]));
  const fills = [];
  const reads = [];
  fake.page.inputValue = async (selector, options) => {
    reads.push(options);
    if (unreadable) throw new Error("Form not ready");
    return fields.get(selector);
  };
  fake.page.fill = async (selector, value) => {
    fills.push({ selector, value });
    fields.set(selector, value);
  };
  fake.page.isEnabled = async () => enabled;
  const wait = fake.page.waitForTimeout;
  fake.page.waitForTimeout = async (ms) => {
    if (wipe) {
      for (const selector of selectors) fields.set(selector, "");
      wipe = false;
    }
    await wait(ms);
  };
  return { ...fake, fills, reads };
}

async function signInWithFake({ context, page }) {
  return signInTiktok(context, page, "bob1", "password", Date.now() + 10000, {
    readCode: async () => null,
  });
}

const credentialFills = [
  { selector: "input[name='username']", value: "bob1" },
  { selector: "input[type='password']", value: "password" },
];

test("a disabled login button leaves filled credentials alone across polls", async () => {
  const fake = credentialForm();
  await signInWithFake(fake);
  assert.deepEqual(fake.fills, credentialFills);
  assert.deepEqual(fake.clicks, []);
});

test("credentials wiped to empty after the first fill are filled again", async () => {
  const fake = credentialForm({ wipe: true });
  await signInWithFake(fake);
  assert.deepEqual(fake.fills, [...credentialFills, ...credentialFills]);
  assert.deepEqual(fake.clicks, []);
});

test("filled credentials with an enabled button submit once and preserve human edits", async () => {
  const fake = credentialForm({ values: ["edited-user", "edited-password"], enabled: true });
  await signInWithFake(fake);
  assert.deepEqual(fake.fills, []);
  assert.deepEqual(fake.clicks, ["credentials"]);
});

test("an unreadable credential field leaves the form alone and keeps polling", async () => {
  const fake = credentialForm({ unreadable: true, enabled: true });
  const cookies = await signInWithFake(fake);
  assert.deepEqual(cookies, [cookie("sessionid", ".tiktok.com")]);
  assert.deepEqual(fake.fills, []);
  assert.deepEqual(fake.clicks, []);
  assert.ok(fake.reads.length >= 4);
  for (const options of fake.reads) assert.ok(options.timeout > 0 && options.timeout <= 1500);
});

test("codes typed in the headed dialog submit once per distinct six-digit value", async () => {
  const { context, page, clicks } = codeDialog([
    "", "", "12345", "12345x", "1234567", "123456", "123456", "654321", "654321", "123456",
  ]);
  await signInTiktok(context, page, "bob1", "password", Date.now() + 10000, {
    readCode: () => new Promise(() => {}),
  });
  assert.deepEqual(clicks, [
    "credentials", { name: "Verify", code: "123456" }, { name: "Verify", code: "654321" },
  ]);
});

test("a terminal code is typed and submitted without resubmitting it from the dialog", async () => {
  const { context, page, clicks } = codeDialog(["", "", "", "123456", "123456"]);
  await signInTiktok(context, page, "bob1", "password", Date.now() + 10000, {
    readCode: async () => "123456",
  });
  assert.deepEqual(clicks, ["credentials", { name: "Verify", code: "123456" }]);
});

test("sign-in keeps polling when the code dialog closes before its value is read", async () => {
  const { context, page, clicks } = codeDialog(["", "", ""]);
  const reads = [];
  page.locator().first().inputValue = async (options) => {
    reads.push(options);
    throw new Error("Code dialog closed");
  };
  const cookies = await signInTiktok(context, page, "bob1", "password", Date.now() + 10000, {
    readCode: () => new Promise(() => {}),
  });
  assert.deepEqual(cookies, [cookie("sessionid", ".tiktok.com")]);
  assert.equal(reads.length, 2);
  for (const options of reads) assert.ok(options.timeout > 0 && options.timeout <= 1500);
  assert.deepEqual(clicks, ["credentials"]);
});
