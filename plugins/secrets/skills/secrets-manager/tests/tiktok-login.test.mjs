// ABOUTME: Tests TikTok login helpers and code submission with a minimal fake browser page.
// ABOUTME: Covers session cookies, ISP slots, and headed codes. No browser, no network.

import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

import { sessionCookies, loginProxy, signInTiktok, TiktokLoginError } from "../scripts/tiktok-login.mjs";
import * as tiktok from "../scripts/tiktok-login.mjs";

let previousStatePath;
beforeEach(() => {
  previousStatePath = process.env.SECRETS_DATA_DIR;
  process.env.SECRETS_DATA_DIR = mkdtempSync(join(tmpdir(), "tiktok-login-"));
});
afterEach(() => {
  rmSync(process.env.SECRETS_DATA_DIR, { recursive: true, force: true });
  if (previousStatePath === undefined) delete process.env.SECRETS_DATA_DIR;
  else process.env.SECRETS_DATA_DIR = previousStatePath;
});

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
    getByText: () => ({ first: () => ({ isVisible: async () => false }) }),
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

for (const message of ["Your account was banned", "Your account is currently suspended."]) {
  test(`visible "${message}" after submitting ends sign-in as banned without further clicks`, async () => {
    const fake = codeDialog(["", "", "", ""]);
    fake.page.getByText = (text, options) => {
      assert.deepEqual(options, { exact: true });
      return { first: () => ({
        isVisible: async () => text === message && fake.clicks.includes("credentials"),
        click: async () => { throw new Error("Ban dialogs must not be clicked"); },
      }) };
    };
    let captured = false;
    fake.page.screenshot = async ({ path }) => {
      assert.match(path, /tiktok-login-banned/);
      captured = true;
    };
    await assert.rejects(signInWithFake(fake), (error) => {
      assert.ok(error instanceof tiktok.TiktokBannedError);
      assert.ok(captured);
      return true;
    });
    assert.deepEqual(fake.clicks, ["credentials"]);
  });
}

test("loginTiktokAccount maps ban errors to restricted and keeps escalation unchanged", () => {
  const scripts = new URL("../scripts/", import.meta.url);
  const result = spawnSync(process.execPath, ["--experimental-test-module-mocks", "--input-type=module", "-e", `
    import assert from "node:assert/strict";
    import { mock } from "node:test";
    let failure;
    mock.module(${JSON.stringify(new URL("login.mjs", scripts).href)}, {
      namedExports: { withProfile: async (username, options, run) => run(
        { cookies: async () => [] }, { goto: async () => { throw failure; } }
      ) },
    });
    const tiktok = await import(${JSON.stringify(new URL("tiktok-login.mjs", scripts).href)});
    const store = await import(${JSON.stringify(new URL("store.mjs", scripts).href)});
    process.env.ISP_PROXY_URL = "http://isp.example:8000";
    const db = store.openDb(":memory:");
    try {
      store.upsertTiktok(db, { username: "bob1", password: "pw" });
      for (const [error, outcome, status] of [
        [new tiktok.TiktokBannedError("banned"), "restricted", store.STATUS_RESTRICTED],
        [new tiktok.TiktokLoginError("unresolved"), "needs-human", store.STATUS_ESCALATED],
      ]) {
        failure = error;
        assert.equal(await tiktok.loginTiktokAccount(db, store.getTiktok(db, "bob1")), outcome);
        assert.equal(store.getTiktok(db, "bob1").status, status);
      }
    } finally { db.close(); }
  `], { encoding: "utf8", timeout: 10000 });
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

function credentialForm({ values = ["", ""], enabled = false, wipe = false, unreadable = false, polls = wipe ? 2 : 4 } = {}) {
  const fake = codeDialog(Array(polls).fill(""));
  let poll = 0;
  const selectors = ["input[name='username']", "input[type='password']"];
  const fields = new Map(selectors.map((selector, i) => [selector, values[i]]));
  const fills = [];
  const fillPolls = [];
  const reads = [];
  fake.page.inputValue = async (selector, options) => {
    reads.push(options);
    if (unreadable) throw new Error("Form not ready");
    return fields.get(selector);
  };
  fake.page.fill = async (selector, value) => {
    fills.push({ selector, value });
    fillPolls.push(poll);
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
    if (ms === 1500) poll++;
  };
  return { ...fake, fills, fillPolls, reads };
}

async function signInWithFake({ context, page }) {
  return signInTiktok(context, page, "bob1", "password", Date.now() + 10000, {
    headed: true,
  });
}

const credentialFills = [
  { selector: "input[name='username']", value: "bob1" },
  { selector: "input[type='password']", value: "password" },
];

test("a login button still disabled on the poll after filling ends sign-in with its own error and a capture", async () => {
  const fake = credentialForm({ polls: 10 });
  const waits = [];
  const wait = fake.page.waitForTimeout;
  fake.page.waitForTimeout = async (ms) => { waits.push(ms); await wait(ms); };
  let captured = false;
  fake.page.screenshot = async () => { captured = true; };
  await assert.rejects(signInWithFake(fake), (error) => {
    assert.ok(error instanceof tiktok.TiktokDisabledLoginError);
    assert.match(error.message, /disabled login button/i);
    assert.ok(captured);
    return true;
  });
  assert.deepEqual(fake.fills, credentialFills);
  assert.deepEqual(fake.fillPolls, [0, 0]);
  assert.deepEqual(waits, [1500]);
  assert.deepEqual(fake.clicks, []);
});

for (const scenario of ["disabled-success", "disabled-twice", "banned"]) {
  test(`fresh profile retry: ${scenario}`, () => {
    const scripts = new URL("../scripts/", import.meta.url);
    const result = spawnSync(process.execPath, ["--experimental-test-module-mocks", "--input-type=module", "-e", `
      import assert from "node:assert/strict";
      import { mock } from "node:test";
      import { mkdirSync, writeFileSync, readFileSync, readdirSync, existsSync } from "node:fs";
      import { dirname, join } from "node:path";
      const scenario = ${JSON.stringify(scenario)};
      const config = await import(${JSON.stringify(new URL("config.mjs", scripts).href)});
      const profile = config.profileDirFor("bob1");
      mkdirSync(profile, { recursive: true });
      writeFileSync(join(profile, "fingerprint.json"), "old fingerprint");
      const backups = () => readdirSync(dirname(profile)).filter(name => name.startsWith("bob1.bak-"));
      let attempts = 0, closed = 0, tiktok;
      mock.module(${JSON.stringify(new URL("login.mjs", scripts).href)}, {
        namedExports: { withProfile: async (username, options, run) => {
          assert.equal(username, "bob1");
          assert.equal(options.proxyUrl, "http://isp.example:8003");
          assert.equal(options.headed, true);
          attempts++;
          if (attempts === 2) {
            assert.equal(closed, 1);
            assert.equal(backups().length, 1);
            assert.ok(!existsSync(profile));
            mkdirSync(profile);
            writeFileSync(join(profile, "fingerprint.json"), "fresh fingerprint");
          }
          const success = scenario === "disabled-success" && attempts === 2;
          const cookies = [{ name: "sessionid", value: "new-session", domain: ".tiktok.com" }];
          try {
            return await run({ cookies: async () => success ? cookies : [] }, {
              goto: async () => {
                if (!success) throw scenario === "banned"
                  ? new tiktok.TiktokBannedError("banned")
                  : new tiktok.TiktokDisabledLoginError("disabled");
              },
              evaluate: async () => true,
            });
          } finally {
            // The first profile must still exist until its browser is closed.
            if (attempts === 1) assert.equal(backups().length, 0);
            closed++;
          }
        } },
      });
      tiktok = await import(${JSON.stringify(new URL("tiktok-login.mjs", scripts).href)});
      const store = await import(${JSON.stringify(new URL("store.mjs", scripts).href)});
      process.env.ISP_PROXY_URL = "http://isp.example:8000";
      const db = store.openDb(":memory:");
      try {
        store.upsertTiktok(db, { username: "bob1", password: "pw" });
        const outcome = await tiktok.loginTiktokAccount(db,
          { ...store.getTiktok(db, "bob1"), isp_slot: 3 }, { headed: true });
        const row = store.getTiktok(db, "bob1");
        assert.equal(attempts, scenario === "banned" ? 1 : 2);
        assert.equal(closed, attempts);
        assert.equal(outcome, scenario === "banned" ? "restricted" : scenario === "disabled-twice" ? "needs-human" : "ok");
        assert.equal(row.status, scenario === "banned" ? store.STATUS_RESTRICTED : scenario === "disabled-twice" ? store.STATUS_ESCALATED : store.STATUS_ACTIVE);
        assert.equal(backups().length, scenario === "banned" ? 0 : 1);
        if (scenario !== "banned") {
          assert.match(backups()[0], /^bob1\\.bak-\\d{4}-\\d{2}-\\d{2}T.*Z$/);
          assert.equal(readFileSync(join(dirname(profile), backups()[0], "fingerprint.json"), "utf8"), "old fingerprint");
        }
        if (scenario === "disabled-success") {
          assert.equal(row.isp_slot, 3);
          assert.ok(row.cookies.includes("new-session"));
        }
      } finally { db.close(); }
    `], { encoding: "utf8", timeout: 10000 });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const logs = result.stdout.match(/login button disabled; retrying once/g) ?? [];
    assert.equal(logs.length, scenario === "banned" ? 0 : 1);
  });
}

test("a disabled login button never refills credentials containing a human edit", async () => {
  for (const values of [["edited-user", "password"], ["bob1", "edited-password"]]) {
    const fake = credentialForm({ values, polls: 10 });
    await assert.rejects(signInWithFake(fake), TiktokLoginError);
    assert.deepEqual(fake.fills, []);
    assert.deepEqual(fake.clicks, []);
  }
});

test("credentials wiped to empty after the first fill are filled again", async () => {
  const fake = credentialForm({ wipe: true });
  fake.page.inputValue = async () => "";
  const cookies = await signInWithFake(fake);
  assert.deepEqual(cookies, [cookie("sessionid", ".tiktok.com")]);
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
    headed: true,
  });
  assert.deepEqual(clicks, [
    "credentials", { name: "Verify", code: "123456" }, { name: "Verify", code: "654321" },
  ]);
});

test("headless email verification reports that a headed login is required", async () => {
  const { context, page, clicks } = codeDialog(["", "", ""]);
  await assert.rejects(signInTiktok(context, page, "bob1", "password", Date.now() + 10000), /run login tiktok --headed/);
  assert.deepEqual(clicks, ["credentials"]);
});

test("sign-in keeps polling when the code dialog closes before its value is read", async () => {
  const { context, page, clicks } = codeDialog(["", "", ""]);
  const reads = [];
  page.locator().first().inputValue = async (options) => {
    reads.push(options);
    throw new Error("Code dialog closed");
  };
  const cookies = await signInTiktok(context, page, "bob1", "password", Date.now() + 10000, {
    headed: true,
  });
  assert.deepEqual(cookies, [cookie("sessionid", ".tiktok.com")]);
  assert.equal(reads.length, 2);
  for (const options of reads) assert.ok(options.timeout > 0 && options.timeout <= 1500);
  assert.deepEqual(clicks, ["credentials"]);
});
