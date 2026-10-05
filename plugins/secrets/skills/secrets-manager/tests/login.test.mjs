// ABOUTME: Tests login.mjs's pure helpers: proxy URL parsing, storage-state scoping, password
// ABOUTME: candidates, app-password extraction, and submitPassword against a stub surface.
// ABOUTME: The Camoufox browser flow itself is not covered here (needs real accounts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

import { APPPASSWORDS_URL, TWOSV_URL, toProxyDict, filterState, extractAppPassword, submitPassword, submitIdentifier, aliasFor, parseSetupKey, onSettingsPage, nationalNumber, DIAL_CODES, COUNTRY_NAMES, isMyAccountUrl, isOnboardingUrl, isSignedInUrl, forceEnglishUrl, ensureEnglish, waitForHuman, pollForState, submitRecaptcha, visibleRecaptchaAnchor, waitGridChanged, waitTilesSwapped, classifyGoogleNode, shouldHoldOpenForDebug, gotoWithRetry, signInGoogle, appAlreadySignedIn, oauthSurface, withAppRetries, isTransientAppError, loadOrCreateFingerprint, Restricted, Expired, NeedsHuman, AppRestricted } from "../scripts/login.mjs";

// classifyGoogleNode maps (url, visible-input inventory) to the graph node the loop dispatches on.
// A hidden input (visible:false) never decides the node — the phone challenge ships a hidden
// identifierId alongside the visible tel field, and must classify as "phone", not "identifier".
test("classifyGoogleNode reads the phone challenge (visible tel, hidden identifier) as phone", () => {
  const inputs = [
    { name: "", type: "tel", id: "phoneNumberId", visible: true },
    { name: "", type: "hidden", id: "identifierId", visible: false },
  ];
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/iap?x=1", inputs), "phone");
});

test("classifyGoogleNode reads the SMS code page (idvPin, also type tel) as phone-code, not phone", () => {
  // The code-entry page and the number-entry page both carry a type=tel field; only the number page
  // is `phoneNumberId`. Misreading the code page as `phone` re-prompts for a number, or rents a fresh
  // HeroSMS number and types it into the code box.
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/ipp", [{ id: "idvPin", type: "tel", visible: true }]), "phone-code");
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/iap", [{ id: "phoneNumberId", type: "tel", visible: true }]), "phone");
});

test("signInGoogle does nothing more when the sign-in page lands on the account dashboard", async () => {
  // An already signed-in profile is redirected from Google's sign-in URL straight to the dashboard
  // (verified live 2026-10-01), so the sign-in needs no separate "is there a session?" visit.
  const visited = [];
  let url = "about:blank";
  const page = {
    url: () => url,
    goto: async (to) => {
      visited.push(to);
      url = "https://myaccount.google.com/";
    },
  };
  await signInGoogle(page, { email: "user@example.com" });
  assert.deepEqual(visited, ["https://accounts.google.com/ServiceLogin?hl=en"]);
});

test("gotoWithRetry retries a stalled navigation until one attempt succeeds", async () => {
  let calls = 0;
  const target = {
    goto: async (_url, opts) => {
      calls += 1;
      assert.equal(opts.timeout, 5000); // per-attempt bound is passed through
      if (calls < 3) throw new Error("page.goto: Timeout 5000ms exceeded.");
      return "loaded";
    },
  };
  assert.equal(await gotoWithRetry(target, "https://x", { totalMs: 1000 }), "loaded");
  assert.equal(calls, 3);
});

test("gotoWithRetry accepts a committed navigation even when the per-attempt wait times out", async () => {
  let calls = 0;
  const target = {
    url: () => (calls === 0 ? "about:blank" : "https://accounts.google.com/v3/signin/identifier"),
    goto: async () => {
      calls += 1;
      throw new Error("page.goto: Timeout 5000ms exceeded."); // domcontentloaded slow, but the page navigated
    },
  };
  assert.equal(await gotoWithRetry(target, "https://accounts.google.com/ServiceLogin", { attemptMs: 1, totalMs: 1000 }), null);
  assert.equal(calls, 1); // did not keep restarting a navigation that already committed
});

test("gotoWithRetry throws the last error when nothing loads within the total budget", async () => {
  let calls = 0;
  const target = {
    goto: async () => {
      calls += 1;
      throw new Error("page.goto: Timeout exceeded.");
    },
  };
  await assert.rejects(gotoWithRetry(target, "https://x", { attemptMs: 1, totalMs: 10 }), /Timeout/);
  assert.ok(calls >= 1);
});

test("loadOrCreateFingerprint generates and persists on first call, then reuses the stored one", () => {
  const dir = mkdtempSync(join(tmpdir(), "fp-"));
  const path = join(dir, "fingerprint.json");
  let generated = 0;
  const generate = () => ({ id: ++generated, navigator: { userAgent: "FF" } });

  assert.equal(existsSync(path), false);
  const first = loadOrCreateFingerprint(path, generate);
  assert.equal(first.id, 1); // generated
  assert.equal(generated, 1);
  assert.equal(existsSync(path), true); // persisted
  assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), first);

  const second = loadOrCreateFingerprint(path, generate);
  assert.deepEqual(second, first); // same fingerprint reused
  assert.equal(generated, 1); // generator NOT called again
});

test("loadOrCreateFingerprint reuses an existing file without generating", () => {
  const dir = mkdtempSync(join(tmpdir(), "fp-"));
  const path = join(dir, "fingerprint.json");
  const stored = { id: 42, navigator: { userAgent: "FF" } };
  writeFileSync(path, JSON.stringify(stored));
  let generated = 0;
  const out = loadOrCreateFingerprint(path, () => { generated += 1; return { id: 999 }; });
  assert.deepEqual(out, stored);
  assert.equal(generated, 0);
});

test("isTransientAppError retries a generic flake but not a ban, a human stop, or a lapsed session", () => {
  // beta's Cloudflare edge intermittently serves a 404/blank page or fails to open the OAuth popup;
  // both surface as a plain Error and clear on a fresh reload, so they are worth retrying. A ban,
  // a Google-side stop that needs a person, and a lapsed session are terminal and must propagate.
  assert.equal(isTransientAppError(new Error("beta: session token never appeared")), true);
  assert.equal(isTransientAppError(new Error('login entry not found (tried ["Sign up"])')), true);
  assert.equal(isTransientAppError(new Restricted("banned")), false);
  assert.equal(isTransientAppError(new AppRestricted("beta", "beta refused the sign-in")), false);
  assert.equal(isTransientAppError(new NeedsHuman("captcha")), false);
  assert.equal(isTransientAppError(new Expired("session lapsed")), false);
});

// A page for the auth-state check: `entryVisible` says whether a login entry label is on screen, and
// every wait is recorded so a test can assert how long the check paused.
function pageForAuthState({ url = "https://app.example/", entryVisible = () => false } = {}) {
  const waits = [];
  return {
    waits,
    url: () => url,
    waitForTimeout: async (ms) => void waits.push(ms),
    getByText: (text) => ({ first: () => ({ isVisible: async () => entryVisible(text) }) }),
  };
}

test("appAlreadySignedIn reports logged out as soon as the login entry shows, without a token poll", async () => {
  const page = pageForAuthState({ entryVisible: (text) => text === "Log In" });
  const adapter = { ready: async () => false, entryTexts: ["Log In"] };
  assert.equal(await appAlreadySignedIn(page, adapter), false);
  assert.deepEqual(page.waits, []);
});

test("appAlreadySignedIn reports signed in when the session token is already there", async () => {
  const page = pageForAuthState({ entryVisible: () => true });
  const adapter = { ready: async () => true, entryTexts: ["Log In"] };
  assert.equal(await appAlreadySignedIn(page, adapter), true);
  assert.deepEqual(page.waits, []);
});

test("appAlreadySignedIn keeps polling until the token or the login entry appears", async () => {
  let polls = 0;
  const page = pageForAuthState();
  const adapter = { ready: async () => ++polls >= 3, entryTexts: ["Log In"] };
  assert.equal(await appAlreadySignedIn(page, adapter), true);
  assert.deepEqual(page.waits, [300, 300]);
});

test("appAlreadySignedIn reads a signed-in URL marker instead of the token when the adapter has one", async () => {
  const page = pageForAuthState({ url: "https://app.example/token" });
  const adapter = { ready: async () => false, signedInUrl: (url) => url.endsWith("/token"), entryTexts: ["Login"] };
  assert.equal(await appAlreadySignedIn(page, adapter), true);
});

// A fake app page whose browser context holds `pages`; records the waits the OAuth wait makes.
function pageForOauth({ url = "https://app.example/", pages = [] } = {}) {
  const page = pageForAuthState({ url });
  page.context = () => ({ pages: () => [page, ...pages] });
  page.screenshot = async () => {};
  page.content = async () => "<html></html>";
  return page;
}

test("oauthSurface returns the Google page as soon as one is open", async () => {
  const google = { url: () => "https://accounts.google.com/o/oauth2/auth" };
  const page = pageForOauth({ pages: [google] });
  const adapter = { name: "app", domain: "app.example", ready: async () => false };
  assert.equal(await oauthSurface(page, adapter, "user@example.com"), google);
  assert.deepEqual(page.waits, []);
});

test("oauthSurface returns null once the app session lands with no Google page caught", async () => {
  let polls = 0;
  const page = pageForOauth();
  const adapter = { name: "app", domain: "app.example", ready: async () => ++polls >= 3 };
  assert.equal(await oauthSurface(page, adapter, "user@example.com"), null);
  assert.deepEqual(page.waits, [1000, 1000]);
});

test("oauthSurface fails when neither a Google page nor the app session appears", async () => {
  process.env.SECRETS_MANAGER_STATE_PATH = mkdtempSync(join(tmpdir(), "login-"));
  try {
    const page = pageForOauth();
    const adapter = { name: "app", domain: "app.example", ready: async () => false };
    await assert.rejects(oauthSurface(page, adapter, "user@example.com"), /app: Google sign-in never opened/);
    assert.equal(page.waits.length, 30);
    const captures = readdirSync(join(process.env.SECRETS_MANAGER_STATE_PATH, "debug", "user@example.com"));
    assert.match(captures[0], /^app-oauth-not-opened-/);
  } finally {
    delete process.env.SECRETS_MANAGER_STATE_PATH;
  }
});

test("oauthSurface does not read the app session while the page is off the app's domain", async () => {
  // The session check reads storage on whatever origin the page is on, so a same-tab hop through an
  // auth provider must not be read as the app's own session.
  process.env.SECRETS_MANAGER_STATE_PATH = mkdtempSync(join(tmpdir(), "login-"));
  try {
    let readyCalls = 0;
    const page = pageForOauth({ url: "https://auth.provider.example/oauth" });
    const adapter = { name: "app", domain: "app.example", ready: async () => (readyCalls++, true) };
    await assert.rejects(oauthSurface(page, adapter, "user@example.com"), /Google sign-in never opened/);
    assert.equal(readyCalls, 0);
  } finally {
    delete process.env.SECRETS_MANAGER_STATE_PATH;
  }
});

test("withAppRetries retries a transient failure until one attempt succeeds", async () => {
  let calls = 0;
  const attempt = async () => {
    calls += 1;
    if (calls < 3) throw new Error("beta: app did not render (blank/404)");
    return "ok";
  };
  assert.equal(await withAppRetries(3, attempt), "ok");
  assert.equal(calls, 3);
});

test("withAppRetries stops at the attempt cap and throws the last error", async () => {
  let calls = 0;
  const attempt = async () => {
    calls += 1;
    throw new Error("beta: session token never appeared");
  };
  await assert.rejects(withAppRetries(3, attempt), /session token never appeared/);
  assert.equal(calls, 3);
});

test("withAppRetries propagates a non-transient failure immediately without retrying", async () => {
  let calls = 0;
  const attempt = async () => {
    calls += 1;
    throw new Restricted("banned");
  };
  await assert.rejects(withAppRetries(3, attempt), Restricted);
  assert.equal(calls, 1);
});

test("classifyGoogleNode reads the security-key OTP page (challenge/skotp) as authenticator, not phone", () => {
  // The skotp page carries a type=tel pin field (securityKeyOtpInputId) that would otherwise read as a
  // rentable phone number; it must route to the authenticator switch so a TOTP account can finish.
  const inputs = [
    { name: "Pin", type: "tel", id: "securityKeyOtpInputId", visible: true },
    { name: "", type: "hidden", id: "identifierId", visible: false },
  ];
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/skotp?x=1", inputs), "authenticator");
  // Even without the skotp URL, the security-key pin field alone identifies the node.
  assert.equal(classifyGoogleNode("https://accounts.google.com/x", inputs), "authenticator");
  // The "get a security code from your device" page (challenge/ootp) carries a plain code field; it,
  // too, routes to the authenticator switch rather than reading as a rentable phone number.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/ootp?x=1", [{ name: "Pin", type: "tel", visible: true }]),
    "authenticator",
  );
  // A real number-entry field must still read as phone.
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/iap", [{ id: "phoneNumberId", type: "tel", visible: true }]), "phone");
});
test("classifyGoogleNode reads TOTP, backup, password and identifier from the field shapes", () => {
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/totp", [{ name: "totpPin", type: "tel", visible: true }]), "totp");
  assert.equal(classifyGoogleNode("https://accounts.google.com/x", [{ name: "backupCode", type: "text", visible: true }]), "backup");
  assert.equal(classifyGoogleNode("https://accounts.google.com/x", [{ name: "Passwd", type: "password", visible: true }]), "password");
  assert.equal(classifyGoogleNode("https://accounts.google.com/x", [{ id: "identifierId", type: "email", visible: true }]), "identifier");
});

test("classifyGoogleNode reads reCAPTCHA, onboarding, signed-in from the URL", () => {
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/recaptcha?x", []), "recaptcha");
  assert.equal(classifyGoogleNode("https://gds.google.com/web/homeaddress", []), "onboarding");
  assert.equal(classifyGoogleNode("https://myaccount.google.com/", []), "signed-in");
});

test("classifyGoogleNode reads the banned-account speedbump as restricted", () => {
  // Google routes a banned account to /signin/speedbump/disabled/explanation with no inputs — a
  // terminal dead end no human can clear, so it must not read as a generic challenge.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/speedbump/disabled/explanation?TL=x", []),
    "restricted",
  );
});

test("classifyGoogleNode reads the verify-method chooser (challenge/selection) as selection, not password", () => {
  // challenge/selection ("Verify it's you — choose how you want to sign in") ships a hidden-but-
  // visible Passwd input, so the input-shape check would misread it as the password step and spin
  // re-submitting the password. The URL must claim it first.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/selection?TL=x&lid=3", [
      { name: "Passwd", type: "password", id: "", visible: true },
      { name: "identifier", type: "email", id: "hiddenEmail", visible: false },
    ]),
    "selection",
  );
});

test("classifyGoogleNode reads Google's sign-in rejection as restricted", () => {
  // /signin/rejected is Google refusing this sign-in ("Couldn't sign you in — couldn't verify this
  // account belongs to you"). Its only exit is Account Recovery, which needs the recovery email or
  // phone we do not hold, so it is a dead end, and it must be caught by URL even though it bounces
  // back to the email page.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/rejected?TL=x&rrk=7", []),
    "restricted",
  );
});
test("classifyGoogleNode reads the QR-scan device check (iap/qrcode) as restricted", () => {
  // challenge/iap/qrcode asks to scan a QR code with a real phone to prove the device is human — a
  // dead end for automation. Its URL contains challenge/iap, so it must be caught before anything
  // reads it as the rentable phone step.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/iap/qrcode?TL=x", []),
    "restricted",
  );
});

test("classifyGoogleNode reads the QR-scan device check (ipp/qrcode) as restricted", () => {
  // challenge/ipp/qrcode is the same scan-a-QR-with-a-real-phone device check as iap/qrcode, served
  // under the ipp path — a dead end for automation. Without this it falls through to the generic
  // "challenge" node and the loop spins on a page it can never advance.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/ipp/qrcode?TL=x", []),
    "restricted",
  );
});

test("classifyGoogleNode reads the confirm-existing-number challenge (ipp/collect) as restricted", () => {
  // challenge/ipp/collect asks to re-enter the phone number ALREADY on the account (••XX) — a dead
  // end we cannot pass (we don't hold that number). It ships a visible tel field, so without this it
  // would read as "phone" and the loop would futilely rent numbers. It must NOT be confused with the
  // rentable challenge/iap/verify, where Google texts a code to a number we supply.
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/ipp/collect?TL=x", [
      { id: "phoneNumberId", type: "tel", visible: true },
    ]),
    "restricted",
  );
  assert.equal(
    classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/iap/verify?TL=x", [
      { id: "phoneNumberId", type: "tel", visible: true },
    ]),
    "phone",
  );
});

test("shouldHoldOpenForDebug keeps the window only for a human-actionable headed failure", () => {
  // A terminal Restricted dead end closes at once (no one can act on it); a NeedsHuman challenge or
  // any other fault keeps the headed window open; a headless run never holds anything open.
  assert.equal(shouldHoldOpenForDebug(true, new Restricted("dead")), false);
  assert.equal(shouldHoldOpenForDebug(true, new Expired("cooldown")), false); // auto-closes by default
  assert.equal(shouldHoldOpenForDebug(true, new Expired("see it", { holdOpen: true })), true); // kept for inspection
  assert.equal(shouldHoldOpenForDebug(true, new NeedsHuman("finish by hand")), true);
  assert.equal(shouldHoldOpenForDebug(true, new Error("boom")), true);
  assert.equal(shouldHoldOpenForDebug(false, new NeedsHuman("finish by hand")), false);
  assert.equal(shouldHoldOpenForDebug(false, new Restricted("dead")), false);
  assert.equal(shouldHoldOpenForDebug(false, new Expired("see it", { holdOpen: true })), false);
});

test("classifyGoogleNode falls back to challenge for an unmapped challenge page, else unknown", () => {
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/dp", []), "challenge");
  assert.equal(classifyGoogleNode("https://accounts.google.com/signin/v2/consent", []), "unknown");
});

test("forceEnglishUrl adds hl=en to a Google OAuth page that lacks it", () => {
  const out = forceEnglishUrl("https://accounts.google.com/signin/oauth/id?authuser=0&part=xyz");
  assert.equal(out, "https://accounts.google.com/signin/oauth/id?authuser=0&part=xyz&hl=en");
});

test("forceEnglishUrl overrides a non-English hl", () => {
  assert.equal(forceEnglishUrl("https://accounts.google.com/x?hl=id"), "https://accounts.google.com/x?hl=en");
});

// A Google page rendered in `lang` at `url`, recording every navigation.
function googlePageIn(lang, url) {
  const visited = [];
  return {
    visited,
    url: () => url,
    evaluate: async () => lang,
    goto: async (target) => {
      visited.push(target);
    },
  };
}

test("ensureEnglish reloads a Google page rendered in another language with hl=en", async () => {
  const page = googlePageIn("id", "https://accounts.google.com/signin/oauth/id?authuser=0&part=xyz");
  assert.equal(await ensureEnglish(page), true);
  assert.deepEqual(page.visited, ["https://accounts.google.com/signin/oauth/id?authuser=0&part=xyz&hl=en"]);
});

test("ensureEnglish leaves a page that already renders in English alone", async () => {
  const page = googlePageIn("en-GB", "https://accounts.google.com/signin/oauth/id?authuser=0&part=xyz");
  assert.equal(await ensureEnglish(page), false);
  assert.deepEqual(page.visited, []);
});

test("ensureEnglish leaves a page with no declared language alone", async () => {
  const page = googlePageIn("", "https://accounts.google.com/gsi/transform");
  assert.equal(await ensureEnglish(page), false);
  assert.deepEqual(page.visited, []);
});

test("ensureEnglish does not reload a page that already carries hl=en", async () => {
  const page = googlePageIn("id", "https://accounts.google.com/x?hl=en");
  assert.equal(await ensureEnglish(page), false);
  assert.deepEqual(page.visited, []);
});

test("forceEnglishUrl returns null when already English or not a Google page or not a URL", () => {
  assert.equal(forceEnglishUrl("https://accounts.google.com/x?hl=en"), null);
  assert.equal(forceEnglishUrl("https://auth.session.io/callback"), null);
  assert.equal(forceEnglishUrl("not a url"), null);
});

test("isMyAccountUrl is true only on the myaccount.google.com dashboard", () => {
  assert.equal(isMyAccountUrl("https://myaccount.google.com/"), true);
  assert.equal(isMyAccountUrl("https://myaccount.google.com/security"), true);
});

test("isMyAccountUrl is false on the logged-out bounce, a challenge page and a non-URL", () => {
  assert.equal(isMyAccountUrl("https://www.google.com/account/about/?hl=en-US"), false);
  assert.equal(isMyAccountUrl("https://accounts.google.com/v3/signin/challenge/totp"), false);
  assert.equal(isMyAccountUrl("not a url"), false);
});

test("isOnboardingUrl is true on the gds.google.com setup wizard, false elsewhere", () => {
  assert.equal(isOnboardingUrl("https://gds.google.com/web/recoveryoptions?continue=x"), true);
  assert.equal(isOnboardingUrl("https://gds.google.com/web/homeaddress?cardIndex=1"), true);
  assert.equal(isOnboardingUrl("https://myaccount.google.com/"), false);
  assert.equal(isOnboardingUrl("not a url"), false);
});

test("isSignedInUrl accepts both the dashboard and the setup wizard, rejects sign-in pages", () => {
  assert.equal(isSignedInUrl("https://myaccount.google.com/"), true);
  assert.equal(isSignedInUrl("https://gds.google.com/web/homeaddress"), true);
  assert.equal(isSignedInUrl("https://accounts.google.com/ServiceLogin"), false);
  assert.equal(isSignedInUrl("https://www.google.com/account/about/"), false);
});

test("toProxyDict is null when empty", () => {
  assert.equal(toProxyDict(null), null);
  assert.equal(toProxyDict(""), null);
});

test("toProxyDict splits a full URL", () => {
  assert.deepEqual(toProxyDict("http://user:pass@gate.proxy.com:8080"), {
    server: "http://gate.proxy.com:8080",
    username: "user",
    password: "pass",
  });
});

test("toProxyDict without auth", () => {
  assert.deepEqual(toProxyDict("socks5://host:1080"), { server: "socks5://host:1080" });
});

test("toProxyDict decodes percent-encoded credentials", () => {
  assert.deepEqual(toProxyDict("http://u%40x:p%23w@h:1"), { server: "http://h:1", username: "u@x", password: "p#w" });
});

// --- submitPassword against a stub page ---------------------------------------

function stubField(value, { submitsOnEnter = true } = {}) {
  const field = {
    value,
    filled: null,
    entered: false,
    inputValue: async () => field.value,
    fill: async (v) => {
      field.filled = v;
      field.value = v;
    },
    press: async (key) => {
      if (key === "Enter") field.entered = true;
    },
    // Enter navigates away, so the field is no longer visible; a page that ignores Enter keeps it.
    isVisible: async () => !(submitsOnEnter && field.entered),
  };
  return field;
}

function stubSurface(field) {
  const surface = {
    nextClicked: false,
    waitForTimeout: async () => {},
    waitForSelector: async () => {
      if (field === null) throw new Error("no such field");
      return field;
    },
    getByRole: () => ({
      first: () => ({
        count: async () => 1,
        isVisible: async () => true,
        click: async () => {
          surface.nextClicked = true;
        },
      }),
    }),
  };
  return surface;
}

test("submitPassword fills an empty field and submits it with Enter", async () => {
  // Enter is the one submit that carries no label: Google's Next button has no stable id and its
  // text follows the page language.
  const field = stubField("");
  const surface = stubSurface(field);
  assert.equal(await submitPassword(surface, "hunter2"), true);
  assert.equal(field.filled, "hunter2");
  assert.equal(field.entered, true);
  assert.equal(surface.nextClicked, false);
});

test("submitPassword falls back to the Next button when Enter leaves the field on screen", async () => {
  const field = stubField("", { submitsOnEnter: false });
  const surface = stubSurface(field);
  assert.equal(await submitPassword(surface, "hunter2"), true);
  assert.equal(field.entered, true);
  assert.equal(surface.nextClicked, true);
});

test("submitPassword leaves a correct autofill alone but still submits", async () => {
  const field = stubField("hunter2");
  const surface = stubSurface(field);
  assert.equal(await submitPassword(surface, "hunter2"), true);
  assert.equal(field.filled, null);
  assert.equal(field.entered, true);
});

test("submitPassword refills a differing autofill", async () => {
  const field = stubField("stale");
  const surface = stubSurface(field);
  assert.equal(await submitPassword(surface, "hunter2"), true);
  assert.equal(field.filled, "hunter2");
});

test("submitIdentifier submits the email with Enter", async () => {
  const field = stubField("");
  const surface = stubSurface(field);
  assert.equal(await submitIdentifier(surface, "user@example.com"), true);
  assert.equal(field.filled, "user@example.com");
  assert.equal(field.entered, true);
  assert.equal(surface.nextClicked, false);
});

test("submitPassword returns false without a password field", async () => {
  assert.equal(await submitPassword(stubSurface(null), "hunter2"), false);
});

// --- app password extraction ----------------------------------------------------

test("extractAppPassword reads the code after the device label", () => {
  const body =
    "Your app passwords\nbeta-scraper\nCreate\n" +
    "Generated app password\nYour app password for your device\n" +
    "medr juie mskt wsbl\nHow to use it\nDone";
  assert.equal(extractAppPassword(body), "medr juie mskt wsbl");
});

test("extractAppPassword ignores ordinary prose before the code", () => {
  const body =
    "App passwords are less secure than using apps.\n" +
    "type a name for it below\nYour app password for your device\n" +
    "abcd efgh ijkl mnop\nDone";
  assert.equal(extractAppPassword(body), "abcd efgh ijkl mnop");
});

test("extractAppPassword is null without a code", () => {
  assert.equal(extractAppPassword("no app password here"), null);
  assert.equal(extractAppPassword(""), null);
});

// --- authenticator setup-key extraction -------------------------------------------

test("parseSetupKey reads the key after the 'spaces don't matter' label, uppercased", () => {
  const body =
    "Set up authenticator\nCan't scan it?\n" +
    "Enter this text code into the Authenticator app (spaces don't matter):\n" +
    "abcd efgh ijkl mnop qrst uvwx yz23 4567\n" +
    "Make sure Google Authenticator is time based.\nNext";
  assert.equal(parseSetupKey(body), "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567");
});

test("parseSetupKey takes the anchored key, not concatenated nav words on the page", () => {
  // The real page glues nav words together (authenticatorprivacytermshelpabout); the anchor and
  // the required whitespace between groups keep those out of the match.
  const body =
    "authenticatorprivacytermshelpabout\n(spaces don't matter):\n" +
    "2fdx j4mn 7ppq wsbl a2c3 d4e5 f6g7 h2i3\nMake sure it is Time based";
  assert.equal(parseSetupKey(body), "2FDXJ4MN7PPQWSBLA2C3D4E5F6G7H2I3");
});

test("parseSetupKey is null without a key", () => {
  assert.equal(parseSetupKey("no key on this page"), null);
  assert.equal(parseSetupKey(""), null);
  assert.equal(parseSetupKey(null), null);
});

// --- sensitive-settings-page predicate ---------------------------------------------

test("onSettingsPage is true only on myaccount.google.com at the target path", () => {
  assert.equal(onSettingsPage("https://myaccount.google.com/apppasswords", "apppasswords"), true);
  assert.equal(onSettingsPage("https://myaccount.google.com/signinoptions/twosv", "twosv"), true);
});

test("onSettingsPage is false on the challenge page even when it carries the target in the query", () => {
  // 'signinoptions' contains 'signin' and the challenge URL carries the twosv return path in its
  // continue= param — a bare substring check would misfire on both.
  const challenge =
    "https://accounts.google.com/v3/signin/challenge/pwd?continue=https://myaccount.google.com/signinoptions/twosv";
  assert.equal(onSettingsPage(challenge, "twosv"), false);
});

test("onSettingsPage is false on a wrong host or a non-URL", () => {
  assert.equal(onSettingsPage("https://www.google.com/signinoptions/twosv", "twosv"), false);
  assert.equal(onSettingsPage("not a url", "twosv"), false);
});

test("nationalNumber strips the country dial code for Google's national-only field", () => {
  assert.equal(nationalNumber("17822067036", 36), "7822067036"); // Canada +1
  assert.equal(nationalNumber("639552773022", 4), "9552773022"); // Philippines +63
  assert.equal(nationalNumber("+1 782-206-7036", 36), "7822067036"); // non-digits ignored
});

test("nationalNumber passes the number through for an unmapped country", () => {
  assert.equal(nationalNumber("999888777", 999), "999888777");
});

test("every phone-step country has a picker name", () => {
  assert.deepEqual(Object.keys(COUNTRY_NAMES).sort(), Object.keys(DIAL_CODES).sort());
  for (const id of Object.keys(DIAL_CODES)) {
    assert.ok(COUNTRY_NAMES[id], `country ${id} needs a COUNTRY_NAMES entry`);
  }
});

test("classifyGoogleNode flags the password page's image CAPTCHA as its own node", () => {
  const inputs = [
    { name: "Passwd", type: "password", visible: true },
    { name: "ca", type: "text", id: "ca", visible: true },
  ];
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/pwd", inputs), "password-captcha");
});

test("classifyGoogleNode stays plain password when the ca captcha field is hidden", () => {
  const inputs = [
    { name: "Passwd", type: "password", visible: true },
    { name: "ca", type: "text", id: "ca", visible: false },
  ];
  assert.equal(classifyGoogleNode("https://accounts.google.com/v3/signin/challenge/pwd", inputs), "password");
});

// --- storage-state scoping --------------------------------------------------------

const state = () => ({
  cookies: [
    { name: "a", value: "1", domain: "app.example.com" },
    { name: "b", value: "2", domain: ".example.com" },
    { name: "c", value: "3", domain: "other.com" },
  ],
  origins: [
    { origin: "https://app.example.com", localStorage: [{ name: "t", value: "x" }] },
    { origin: "https://other.com", localStorage: [{ name: "z", value: "y" }] },
  ],
});

test("filterState keeps only matching host cookies", () => {
  const scoped = filterState(state(), "app.example.com");
  assert.deepEqual(new Set(scoped.cookies.map((c) => c.name)), new Set(["a", "b"]));
});

test("filterState keeps only matching origin localStorage", () => {
  const scoped = filterState(state(), "app.example.com");
  assert.deepEqual(scoped.local_storage.map((o) => o.origin), ["https://app.example.com"]);
});

test("filterState excludes an unrelated domain entirely", () => {
  const scoped = filterState(state(), "other.com");
  assert.deepEqual(scoped.cookies.map((c) => c.name), ["c"]);
  assert.deepEqual(scoped.local_storage.map((o) => o.origin), ["https://other.com"]);
});

test("filterState keeps subdomain api cookies", () => {
  // beta's session cookies sit on api*.beta.trade while the adapter scopes to the registrable
  // base beta.trade; those cookies must not be dropped.
  const s = {
    cookies: [
      { name: "root", value: "1", domain: "beta.trade" },
      { name: "api", value: "2", domain: "api6.beta.trade" },
      { name: "other", value: "3", domain: "api.gamma.ai" },
    ],
    origins: [],
  };
  assert.deepEqual(new Set(filterState(s, "beta.trade").cookies.map((c) => c.name)), new Set(["root", "api"]));
});

// --- alias ------------------------------------------------------------------------

test("aliasFor builds a plus-alias and never stacks one", () => {
  assert.equal(aliasFor("bob@gmail.com", "beta"), "bob+beta@gmail.com");
  assert.equal(aliasFor("bob+beta@gmail.com", "2"), "bob+2@gmail.com");
});

// pollForState drives the auto reCAPTCHA steps: poll a page predicate on a tight cadence, return its
// first truthy value (a tag the caller switches on, or null at the budget). A virtual clock reads how
// quickly it notices the state and that it holds the full budget before giving up.
test("pollForState returns the state's value within one poll of it turning truthy", async () => {
  let clock = 0;
  const page = { waitForTimeout: async (ms) => { clock += ms; } };
  const v = await pollForState(page, async () => (clock >= 650 ? "grid" : null), 15000);
  assert.equal(v, "grid");
  assert.ok(clock - 650 < 200, `noticed ${clock - 650}ms after the state turned truthy`);
});

test("pollForState checks before the first sleep, so an already-true state costs no wait", async () => {
  const waits = [];
  const page = { waitForTimeout: async (ms) => void waits.push(ms) };
  const v = await pollForState(page, async () => "password", 15000);
  assert.equal(v, "password");
  assert.deepEqual(waits, []);
});

test("pollForState gives up at the budget and returns null", async () => {
  let clock = 0;
  const page = { waitForTimeout: async (ms) => { clock += ms; } };
  const v = await pollForState(page, async () => null, 3000);
  assert.equal(v, null);
  assert.ok(clock >= 3000 && clock < 3200, `waited ${clock}ms`);
});

test("pollForState lets a predicate throw propagate instead of swallowing it", async () => {
  const page = { waitForTimeout: async () => {} };
  await assert.rejects(
    () => pollForState(page, async () => { throw new Error("page closed"); }, 3000),
    /page closed/,
  );
});

// submitRecaptcha waits for the ticked challenge to actually leave the page. The fake stays on the
// reCAPTCHA URL (no password field, checkbox iframe not visible) until `flipAfter` polls, then moves
// to the password page — so the wait must notice the move on the 200ms cadence, not in 1s steps.
function pageForRecaptchaSubmit({ flipAfter }) {
  const waits = [];
  let polls = 0;
  let url = "https://accounts.google.com/v3/signin/challenge/recaptcha?x";
  return {
    waits,
    url: () => url,
    waitForTimeout: async (ms) => {
      waits.push(ms);
      if (++polls >= flipAfter) url = "https://accounts.google.com/v3/signin/challenge/pwd";
    },
    locator: () => ({ first: () => ({ isVisible: async () => false }), count: async () => 0 }),
  };
}

test("submitRecaptcha reports success on the tight cadence once the page leaves the challenge", async () => {
  const page = pageForRecaptchaSubmit({ flipAfter: 3 });
  const advanced = await submitRecaptcha(page, { email: "someone@example.com" });
  assert.equal(advanced, true);
  assert.deepEqual(page.waits, [200, 200, 200]);
});

test("submitRecaptcha gives up after the full window when the page never advances", async () => {
  const page = pageForRecaptchaSubmit({ flipAfter: Infinity });
  const advanced = await submitRecaptcha(page, { email: "someone@example.com" });
  assert.equal(advanced, false);
  assert.equal(page.waits.length, 12000 / 200);
});

test("submitRecaptcha does not trust the reCAPTCHA-gone signal on the first post-click poll", async () => {
  // The URL never moves and no password field appears, so the only "advanced" signal is the negative
  // onRecaptcha (the anchor iframe is not visible). The checkbox may still be settling in the instant
  // after Next, so that negative must not be trusted on poll 0 — success is confirmed one poll later.
  const waits = [];
  const page = {
    url: () => "https://accounts.google.com/signin/v2/sl/pwd", // not a challenge/recaptcha URL
    waitForTimeout: async (ms) => void waits.push(ms),
    locator: () => ({ first: () => ({ isVisible: async () => false }), count: async () => 0 }),
  };
  const advanced = await submitRecaptcha(page, { email: "someone@example.com" });
  assert.equal(advanced, true);
  assert.deepEqual(waits, [200]);
});

// visibleRecaptchaAnchor picks the checkbox of the ONE anchor iframe that is actually visible —
// Google keeps hidden api2/anchor frames on the page for risk scoring, and clicking their checkbox
// would be a click into nothing.
function fakeAnchorFrame({ frameVisible, cbVisible, tag }) {
  return {
    isVisible: async () => frameVisible,
    contentFrame: () => ({ locator: () => ({ isVisible: async () => cbVisible, tag }) }),
  };
}

test("visibleRecaptchaAnchor returns the checkbox of the visible anchor frame, skipping hidden ones", async () => {
  const hidden = fakeAnchorFrame({ frameVisible: false, cbVisible: true, tag: "hidden" });
  const real = fakeAnchorFrame({ frameVisible: true, cbVisible: true, tag: "real" });
  const page = { locator: () => ({ all: async () => [hidden, real] }) };
  const cb = await visibleRecaptchaAnchor(page);
  assert.equal(cb.tag, "real");
});

test("visibleRecaptchaAnchor returns null when no anchor frame's checkbox is visible", async () => {
  const frame = fakeAnchorFrame({ frameVisible: true, cbVisible: false, tag: "x" });
  const page = { locator: () => ({ all: async () => [frame] }) };
  assert.equal(await visibleRecaptchaAnchor(page), null);
});

// waitGridChanged watches the live reCAPTCHA grid after a Verify/reload click. The fake drives one
// {visible, src} grid state per poll; a grid mid-swap that reads closed for a single tick must NOT be
// taken as solved.
function pageForGrid(states) {
  let i = 0;
  const waits = [];
  const bframe = {
    url: () => "https://www.google.com/recaptcha/api2/bframe?x",
    locator: () => ({
      first: () => ({
        isVisible: async () => states[Math.min(i, states.length - 1)].visible,
        getAttribute: async () => states[Math.min(i, states.length - 1)].src ?? null,
      }),
    }),
  };
  return {
    waits,
    frames: () => [bframe],
    waitForTimeout: async (ms) => { waits.push(ms); i++; },
  };
}

test("waitGridChanged reports closed only after the grid stays gone for two consecutive polls", async () => {
  const page = pageForGrid([{ visible: true, src: "a" }, { visible: false }, { visible: false }]);
  const outcome = await waitGridChanged(page, { prevSrc: "a", timeoutMs: 3000 });
  assert.equal(outcome, "closed");
  assert.deepEqual(page.waits, [200, 200]);
});

test("waitGridChanged does not call a one-tick disappearance closed (treats the later swap as changed)", async () => {
  const page = pageForGrid([
    { visible: true, src: "a" },
    { visible: false }, // transient during the swap — must not count as solved
    { visible: true, src: "a" },
    { visible: true, src: "b" },
  ]);
  const outcome = await waitGridChanged(page, { prevSrc: "a", timeoutMs: 3000 });
  assert.equal(outcome, "changed");
});

test("waitGridChanged reports changed when the payload image src is replaced", async () => {
  const page = pageForGrid([{ visible: true, src: "a" }, { visible: true, src: "b" }]);
  const outcome = await waitGridChanged(page, { prevSrc: "a", timeoutMs: 3000 });
  assert.equal(outcome, "changed");
  assert.deepEqual(page.waits, [200]);
});

test("waitGridChanged returns null when the grid neither closes nor changes in the window", async () => {
  const page = pageForGrid([{ visible: true, src: "a" }]);
  const outcome = await waitGridChanged(page, { prevSrc: "a", timeoutMs: 600 });
  assert.equal(outcome, null);
});

// waitTilesSwapped watches a clicked dynamic selection: true the instant a chosen tile reloads (loses
// rc-imageselect-tileselected) or the grid closes, false only if the selection stays put the whole
// window (→ final, press Verify). classSeq drives the clicked tile's class, openSeq the grid-open read,
// one state per poll.
function pageAndBframeForSwap({ classSeq, openSeq }) {
  let i = 0;
  const waits = [];
  const frame = {
    url: () => "https://www.google.com/recaptcha/api2/bframe?x",
    locator: () => ({ first: () => ({ isVisible: async () => openSeq[Math.min(i, openSeq.length - 1)] }) }),
  };
  const page = {
    waits,
    frames: () => [frame],
    waitForTimeout: async (ms) => { waits.push(ms); i++; },
  };
  const bframe = {
    locator: () => ({ nth: () => ({ getAttribute: async () => classSeq[Math.min(i, classSeq.length - 1)] }) }),
  };
  return { page, bframe };
}

test("waitTilesSwapped returns false after the full window when a selection stays final", async () => {
  const { page, bframe } = pageAndBframeForSwap({
    classSeq: ["rc-imageselect-tile rc-imageselect-tileselected"],
    openSeq: [true],
  });
  const swapped = await waitTilesSwapped(bframe, page, [0], 600);
  assert.equal(swapped, false);
  assert.deepEqual(page.waits, [200, 200, 200]);
});

test("waitTilesSwapped returns true as soon as a clicked tile reloads", async () => {
  const { page, bframe } = pageAndBframeForSwap({
    classSeq: [
      "rc-imageselect-tile rc-imageselect-tileselected",
      "rc-imageselect-tile rc-imageselect-tileselected",
      "rc-imageselect-tile", // the chosen tile reloaded — selected class gone
    ],
    openSeq: [true, true, true],
  });
  const swapped = await waitTilesSwapped(bframe, page, [0], 3000);
  assert.equal(swapped, true);
  assert.deepEqual(page.waits, [200, 200]);
});

// waitForHuman hands a Google step to the person at the window and resumes once `done` turns true.
// The surface below keeps a virtual clock so the test reads how long after the person finished the
// flow noticed, without sleeping.
test("waitForHuman resumes within a tenth of a second of the person finishing", async (t) => {
  t.mock.method(console, "log", () => {});
  let clock = 0;
  const surface = {
    isClosed: () => false,
    waitForTimeout: async (ms) => {
      clock += ms;
    },
  };
  const finishedAt = 650;
  const resumed = await waitForHuman(surface, { email: "someone@example.com" }, { done: async () => clock >= finishedAt });
  assert.equal(resumed, true);
  assert.ok(clock - finishedAt < 100, `noticed ${clock - finishedAt}ms after the person finished`);
});

test("waitForHuman gives up once its window has passed", async (t) => {
  t.mock.method(console, "log", () => {});
  let clock = 0;
  const surface = {
    isClosed: () => false,
    waitForTimeout: async (ms) => {
      clock += ms;
    },
  };
  const resumed = await waitForHuman(surface, { email: "someone@example.com" }, { timeoutMs: 3000, done: async () => false });
  assert.equal(resumed, false);
  assert.ok(clock >= 3000 && clock < 3100, `waited ${clock}ms`);
});

test("the account settings pages are opened in English", () => {
  // myaccount.google.com and the re-auth challenge it bounces through render in the account's own
  // language unless `hl=en` is on the URL; every label matched on those pages is English.
  for (const url of [APPPASSWORDS_URL, TWOSV_URL]) {
    assert.equal(new URL(url).searchParams.get("hl"), "en", url);
    assert.equal(new URL(url).hostname, "myaccount.google.com");
  }
  assert.equal(onSettingsPage(APPPASSWORDS_URL, "apppasswords"), true);
  assert.equal(onSettingsPage(TWOSV_URL, "twosv"), true);
});

test("an app's ban names the app and is not a Google restriction to the caller's eye", () => {
  const e = new AppRestricted("axiom", "Axiom refused the Google sign-in");
  assert.equal(e.app, "axiom");
  assert.ok(e instanceof Restricted);
  assert.equal(shouldHoldOpenForDebug(true, e), false);
});
