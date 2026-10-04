// ABOUTME: Logs a TikTok account in through a real Camoufox browser and captures its session cookies.
// ABOUTME: The login runs on one ISP pool slot (a fixed IP), the only exit the session is used from.

import { createInterface } from "node:readline";


import * as config from "./config.mjs";
import * as debug from "./debug.mjs";
import * as store from "./store.mjs";
import { withProfile } from "./login.mjs";
import { ispProxyAt } from "./config.mjs";

// The username + password form; TikTok's default login page offers QR and phone first.
const LOGIN_URL = "https://www.tiktok.com/login/phone-or-email/email";
const HOME_URL = "https://www.tiktok.com/explore";

const USER_SELECTOR = "input[name='username']";
const PASS_SELECTOR = "input[type='password']";
const SUBMIT_SELECTOR = "button[data-e2e='login-button']";

// "Verify it's really you": TikTok asks a new device for a code it mails to the account's address.
// The dialog offers the Email method, then a code field.
const CODE_SELECTOR = "input[placeholder*='code' i], input[autocomplete='one-time-code'], input[maxlength='6']";
const CODE_SUBMIT_LABELS = ["Next", "Verify", "Continue", "Submit"];

// The cookie TikTok sets once an account is signed in.
const SESSION_COOKIE = "sessionid";

export class TiktokLoginError extends Error {}

// The tiktok.com cookies of a signed-in browser, or null while it is not signed in.
export function sessionCookies(cookies) {
  const own = cookies.filter((c) => /(^|\.)tiktok\.com$/.test(c.domain));
  return own.some((c) => c.name === SESSION_COOKIE && c.value) ? own : null;
}

// The ISP pool slot a login runs on and its proxy url: the slot the account already holds, else the
// pool's last one. The pool is the fetch-tiktok-mentions skill's (ISP_PROXY_URL, ISP_PROXY_COUNT).
export function loginProxy(env, slot = null) {
  if (!env.ISP_PROXY_URL) {
    throw new Error("No ISP_PROXY_URL in ~/.config/secrets-manager/.env; never log in from the home IP.");
  }
  slot ??= Math.max(1, Number(env.ISP_PROXY_COUNT) || 1);
  return { slot, url: ispProxyAt(env.ISP_PROXY_URL, slot) };
}

// TikTok renders fields before its script takes them over, so early input can be wiped or not
// registered. Fill empty fields and retry our unchanged values at most three times while disabled,
// keeping the page from jumping indefinitely so a person can take over. Preserve human edits.
async function submitCredentials(page, username, password, state) {
  try {
    let userValue = await page.inputValue(USER_SELECTOR, { timeout: 1000 });
    let passValue = await page.inputValue(PASS_SELECTOR, { timeout: 1000 });
    const filledEmpty = userValue === "" || passValue === "";
    if (userValue === "") await page.fill(USER_SELECTOR, username, { timeout: 4000 });
    if (passValue === "") await page.fill(PASS_SELECTOR, password, { timeout: 4000 });
    userValue = await page.inputValue(USER_SELECTOR, { timeout: 1000 });
    passValue = await page.inputValue(PASS_SELECTOR, { timeout: 1000 });
    if (!userValue || !passValue) return false;
    if (!(await page.isEnabled(SUBMIT_SELECTOR))) {
      if (!filledEmpty && userValue === username && passValue === password && state.refills < 3) {
        state.refills++;
        await page.fill(USER_SELECTOR, username, { timeout: 4000 });
        await page.fill(PASS_SELECTOR, password, { timeout: 4000 });
      }
      return false;
    }
    await page.click(SUBMIT_SELECTOR, { timeout: 4000 });
    return true;
  } catch {
    return false;
  }
}

// Ask for the emailed code on the terminal. Null when there is no terminal to answer (stdin
// closed) or the question is called off (`signal`), which leaves the code to a person in the
// headed window.
export function promptEmailCode(username, signal, { input = process.stdin, output = process.stdout } = {}) {
  return new Promise((resolve) => {
    const rl = createInterface({ input, output });
    rl.once("close", () => resolve(null));
    signal?.addEventListener("abort", () => rl.close());
    rl.question(`  ${username}: code TikTok emailed: `, (answer) => {
      resolve(answer.trim() || null);
      rl.close();
    });
  });
}

async function clickVisible(locator) {
  if (!(await locator.isVisible().catch(() => false))) return false;
  return locator.click({ timeout: 4000 }).then(() => true, () => false);
}

// Click the first visible submit button in the code dialog.
async function clickCodeSubmit(page) {
  for (const name of CODE_SUBMIT_LABELS) {
    if (await clickVisible(page.getByRole("button", { name, exact: true }).last())) return;
  }
}

// Type the emailed code into the dialog's code field and submit it.
async function submitEmailCode(page, code) {
  await page.locator(CODE_SELECTOR).first().click({ timeout: 4000 });
  await page.keyboard.type(code, { delay: 80 });
  await page.waitForTimeout(1000);
  await clickCodeSubmit(page);
}

// Drive TikTok's username + password form and the email-code dialog after it, polling for the
// session cookie until the deadline, so a person in the headed window can clear whatever else
// TikTok shows (a CAPTCHA). `readCode` supplies the emailed code; the polling goes on while it is
// asked, so that person may as well type the code into the window, and the question is called off
// when the sign-in ends either way.
export async function signInTiktok(context, page, username, password, deadlineMs, { readCode = promptEmailCode } = {}) {
  await page.goto(LOGIN_URL, { waitUntil: "domcontentloaded" });
  const ended = new AbortController();
  let submitted = false;
  const credentialState = { refills: 0 };
  let codeAsked = false;
  let code = null;
  const submittedCodes = new Set();
  try {
    while (Date.now() < deadlineMs) {
      const cookies = sessionCookies(await context.cookies());
      if (cookies) return cookies;
      if (!submitted) {
        submitted = await submitCredentials(page, username, password, credentialState);
      } else if (code) {
        submittedCodes.add(code);
        await submitEmailCode(page, code).catch(() => {});
        code = null;
      } else if (await page.locator(CODE_SELECTOR).first().isVisible().catch(() => false)) {
        if (!codeAsked) {
          codeAsked = true;
          readCode(username, ended.signal).then((answer) => (code = answer));
        }
        const value = await page.locator(CODE_SELECTOR).first().inputValue({ timeout: 1000 }).catch(() => "");
        if (!code && /^\d{6}$/.test(value) && !submittedCodes.has(value)) {
          submittedCodes.add(value);
          await clickCodeSubmit(page);
        }
      } else if (!codeAsked) {
        await clickVisible(page.getByText("Email", { exact: true }).last());
      }
      await page.waitForTimeout(1500);
    }
  } finally {
    ended.abort();
  }
  await debug.capture(page, username, "tiktok-login-no-session");
  throw new TiktokLoginError(`${username}: no ${SESSION_COOKIE} cookie after login (challenge unresolved?)`);
}

// Whether tiktok.com itself treats this browser as signed in: its page data names the user. A
// session TikTok ended on its side leaves the cookie behind, so the cookie alone does not tell.
async function signedIn(page) {
  await page.goto(HOME_URL, { waitUntil: "domcontentloaded" });
  return page.evaluate(() => {
    const data = JSON.parse(document.getElementById("__UNIVERSAL_DATA_FOR_REHYDRATION__")?.textContent || "{}");
    return Boolean(data.__DEFAULT_SCOPE__?.["webapp.app-context"]?.user?.uniqueId);
  });
}

// Log one TikTok account in and persist its session. Returns a one-word outcome.
export async function loginTiktokAccount(db, row, { headed = false, timeoutS = 300 } = {}) {
  const username = row.username;

  const { slot, url } = loginProxy(process.env, row.isp_slot);
  return withProfile(username, { headed, proxyUrl: url, blockAssets: false }, async (context, page) => {
    // Already logged in from a previous run's persistent profile? A cookie TikTok no longer
    // honours is dropped, so the sign-in below starts from a signed-out browser.
    let cookies = sessionCookies(await context.cookies());
    if (cookies && !(await signedIn(page))) {
      await context.clearCookies({ name: /^sessionid/ });
      cookies = null;
    }
    if (!cookies) {
      try {
        cookies = await signInTiktok(context, page, username, row.password, Date.now() + timeoutS * 1000);
      } catch (e) {
        if (!(e instanceof TiktokLoginError)) throw e;
        store.setTiktokStatus(db, username, store.STATUS_ESCALATED);
        return "needs-human";
      }
    }
    store.saveTiktokLogin(db, username, { cookies, ispSlot: slot });
    return "ok";
  });
}
