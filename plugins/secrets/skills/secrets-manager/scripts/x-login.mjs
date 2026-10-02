// ABOUTME: Logs an X/Twitter account in through a real Camoufox browser and captures auth_token+ct0.
// ABOUTME: X blocks the headless onboarding API; a real browser clears its TLS + JS challenges.

import * as debug from "./debug.mjs";
import * as store from "./store.mjs";
import { computeTotp } from "./totp.mjs";
import { withProfile } from "./login.mjs";

// X's current login lives at the "jf onboarding" page (i/jf/onboarding/web?mode=login), reached by
// this entry URL. Identifier, password and 2FA come on separate steps, each with a "Continue" button.
const LOGIN_URL = "https://x.com/i/flow/login";

const USER_SELECTORS = "input[name='username_or_email'], input[autocomplete='username']";
const PASS_SELECTORS = "input[name='password'], input[autocomplete='current-password']";
const CODE_SELECTORS = "input[autocomplete='one-time-code'], input[data-testid='ocfEnterTextTextInput']";
const ADVANCE_LABELS = ["Continue", "Log in", "Next", "Verify", "继续", "登录", "下一步"];

export class XLoginError extends Error {}

// Pull auth_token + ct0 out of the browser cookie jar; also return the full jar as a name->value map.
export async function captureTokens(context) {
  const jar = {};
  for (const c of await context.cookies()) jar[c.name] = c.value;
  return { authToken: jar.auth_token ?? null, ct0: jar.ct0 ?? null, jar };
}

async function visibleField(page, selectors) {
  for (const field of await page.$$(selectors)) {
    if (await field.isVisible()) return field;
  }
  return null;
}

// Type `value` into the first visible matching input, replacing any existing content. X's login
// field is a React-controlled input that also reuses one element across the identifier and
// password steps: a plain .fill() left the old value in place and the new one appended. So focus
// it (no pointer click — the jf label wrapper eats those), select-all + delete via the keyboard,
// then type character by character so React registers it. Re-queries once if the handle goes
// stale ("Element is not attached to the DOM").
async function fill(page, selectors, value) {
  for (let attempt = 0; attempt < 2; attempt++) {
    const field = await visibleField(page, selectors);
    if (!field) return false;
    try {
      await field.focus();
      await page.keyboard.press("ControlOrMeta+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type(value, { delay: 30 });
      return true;
    } catch {
      await page.waitForTimeout(500);
    }
  }
  return false;
}

// Click the step's submit button. Matches the label EXACTLY: a substring match on "Continue"
// would hit X's "Continue with phone" / "Continue with Google" SSO buttons and derail into phone
// signup, which is exactly what broke an earlier version.
async function advance(page, timeout = 4000) {
  for (const name of ADVANCE_LABELS) {
    try {
      const button = page.getByRole("button", { name, exact: true }).first();
      if (await button.isVisible()) {
        await button.click({ timeout });
        return true;
      }
    } catch {
      continue;
    }
  }
  return false;
}

// Drive X's multi-step login. Returns the cookie jar once auth_token + ct0 appear. A step loop
// handles the identifier -> password -> 2FA sequence in whatever order X serves it, then keeps
// polling for the session cookies until the deadline, so a human in the headed window can clear a
// CAPTCHA or an email/phone challenge without breaking the run.
export async function signInX(context, page, username, password, totpSecret, deadlineMs) {
  await page.goto(LOGIN_URL, { waitUntil: "domcontentloaded" });
  const done = new Set();
  const step = async (key) => {
    done.add(key);
    await advance(page);
    await page.waitForTimeout(2500);
  };
  while (Date.now() < deadlineMs) {
    const { authToken, ct0, jar } = await captureTokens(context);
    if (authToken && ct0) return jar;
    if (!done.has("user") && (await fill(page, USER_SELECTORS, username))) {
      await step("user");
      continue;
    }
    if (!done.has("pass") && (await fill(page, PASS_SELECTORS, password))) {
      await step("pass");
      continue;
    }
    if (totpSecret && !done.has("code") && (await visibleField(page, CODE_SELECTORS))) {
      await fill(page, CODE_SELECTORS, computeTotp(totpSecret));
      await step("code");
      continue;
    }
    await page.waitForTimeout(1500); // a challenge may be up; let a human resolve it, keep polling
  }
  const { authToken, ct0, jar } = await captureTokens(context);
  if (authToken && ct0) return jar;
  await debug.capture(page, username, "x-login-no-tokens");
  throw new XLoginError(`${username}: no auth_token/ct0 after login (challenge unresolved?)`);
}

// Log one X account in and persist the tokens. Returns a one-word outcome.
export async function loginXAccount(db, row, { headed = false, rotate = false, timeoutS = 180 } = {}) {
  const username = row.username;
  if (!row.password) {
    store.setXStatus(db, username, store.STATUS_ESCALATED);
    return "no-password";
  }
  return withProfile(username, { headed, rotate }, async (context, page) => {
    // Already logged in from a previous run's persistent profile?
    let tokens = await captureTokens(context);
    if (!(tokens.authToken && tokens.ct0)) {
      try {
        await signInX(context, page, username, row.password, row.totp_secret, Date.now() + timeoutS * 1000);
      } catch (e) {
        if (!(e instanceof XLoginError)) throw e;
        store.setXStatus(db, username, store.STATUS_ESCALATED);
        return "needs-human";
      }
      tokens = await captureTokens(context);
    }
    store.saveXLogin(db, username, { authToken: tokens.authToken, ct0: tokens.ct0, cookies: tokens.jar });
    return "ok";
  });
}
