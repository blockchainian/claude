// ABOUTME: Drives Camoufox to sign a Google account in and export each app's login state.
// ABOUTME: Pure helpers (proxy parsing, state filtering) are tested; the browser flow is not.

import { mkdirSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { generateFingerprint } from "camoufox-js/dist/fingerprints.js";

import * as captcha from "./captcha.mjs";
import { classifyImage, readImageText } from "./classify-image.mjs";
import * as config from "./config.mjs";
import * as debug from "./debug.mjs";
import * as smsOtp from "./sms-otp.mjs";
import * as store from "./store.mjs";
import { computeTotp } from "./totp.mjs";
import { installBlocklist } from "./traffic.mjs";
import * as windowPlace from "./window-place.mjs";
import { gotoWithRetry } from "./page-helpers.mjs";
import { filterState } from "./state.mjs";
export { filterState };
export { gotoWithRetry } from "./page-helpers.mjs";

// Google demanded a challenge we cannot script (e.g. add a phone number).
export class NeedsHuman extends Error {}

// Google restricted the account (the banned-account speedbump). A terminal dead end: no human can
// clear it, so the caller marks the account restricted (never retried) rather than escalated.
export class Restricted extends Error {}

// Google temporarily refused the account — a phone-step cooldown ("Too many failed attempts. Try
// again in a few hours.") or a rejected sign-in. Not a dead end and not a human's job: the caller
// marks it expired so a later run retries. `holdOpen` keeps a headed window open on this stop so the
// person can read Google's message (otherwise the window closes, since no one needs to act).
export class Expired extends Error {
  constructor(message, { holdOpen = false } = {}) {
    super(message);
    this.holdOpen = holdOpen;
  }
}

// The profile has no live Google session, so a settings flow cannot run. The account must be
// signed in first (`login google`); this is not a Google challenge and never marks it escalated.
export class NotLoggedIn extends Error {}

// Convert a proxy URL into Playwright's {server, username, password} form.
export function toProxyDict(url) {
  if (!url) return null;
  const parsed = new URL(url);
  let server = `${parsed.protocol}//${parsed.hostname}`;
  if (parsed.port) server += `:${parsed.port}`;
  const proxy = { server };
  if (parsed.username) proxy.username = decodeURIComponent(parsed.username);
  if (parsed.password) proxy.password = decodeURIComponent(parsed.password);
  return proxy;
}



// --- Browser flow (not unit-tested; tune live with --headed on the first account) ---

const IDENTIFIER_SELECTOR = "#identifierId, input[name=identifier], input[type=email]";

// True if the persistent profile already carries a live Google session.
// Logged in only when myaccount.google.com serves its own dashboard. A logged-out visit redirects
// to the marketing page `www.google.com/account/about` (which contains neither "signin" nor
// "ServiceLogin"), so a bare substring check false-positives there; require the final host to be
// myaccount.google.com.
export async function isGoogleLoggedIn(page) {
  // A logged-out account redirects myaccount.google.com to the heavy www.google.com/account/about
  // marketing page, whose load can exceed the nav timeout under a slow residential proxy. This is
  // only a probe, so a timeout must not abort the whole login — the URL we landed on still answers
  // "is there a session?" (the about page is not the dashboard), so swallow the timeout and judge by
  // the final URL. A different navigation fault still surfaces (the login can't proceed anyway).
  try {
    await page.goto("https://myaccount.google.com/", { waitUntil: "domcontentloaded" });
  } catch (e) {
    if (!String(e?.message).includes("Timeout")) throw e;
  }
  return isSignedInUrl(page.url());
}

// A read-only "the page is sitting on the account dashboard" test. Unlike `isGoogleLoggedIn` it
// does NOT navigate, so it can judge a just-completed sign-in without clobbering whatever Google
// left on screen (a navigating check can abort an in-flight submit and hide the real blocker).
export function isMyAccountUrl(url) {
  return hostEquals(url, "myaccount.google.com");
}

// Google interposes an optional account-setup wizard (gds.google.com/web/*: "add a recovery phone",
// "set your home address", …) between a fresh sign-in and the dashboard. Reaching it proves the
// account is already authenticated, so it counts as signed in — the cards are skippable nags.
export function isOnboardingUrl(url) {
  return hostEquals(url, "gds.google.com");
}

// Signed in once Google has left the sign-in pages — either on the account dashboard or on the
// post-login setup wizard.
export function isSignedInUrl(url) {
  return isMyAccountUrl(url) || isOnboardingUrl(url);
}

function hostEquals(url, host) {
  try {
    return new URL(url).hostname === host;
  } catch {
    return false;
  }
}

// Classify which node of the Google sign-in graph the page is on, from its URL and the shape of its
// VISIBLE inputs (an inputInventory() list). One place decides "what step is this", so the login and
// the app-OAuth loops dispatch the same way. Hidden inputs never decide the node: the phone
// challenge ships a hidden `identifierId` next to the visible `tel` field, so it must read as
// "phone", not "identifier" — hence the field checks run most-specific first over visible inputs
// only. DOM-text/iframe signals (the reCAPTCHA checkbox iframe, a wrong-password banner, the consent
// buttons) are not input shapes and stay as inline guards in the loop, not here.
export function classifyGoogleNode(url, inputs = []) {
  if (isOnboardingUrl(url)) return "onboarding";
  if (isMyAccountUrl(url)) return "signed-in";
  if (typeof url === "string" && url.includes("challenge/recaptcha")) return "recaptcha";
  // A banned account is routed to the disabled speedbump — a terminal dead end (no human can clear
  // it, only appeal), so it must be recognized before the generic challenge fallback.
  if (typeof url === "string" && url.includes("signin/speedbump/disabled")) return "restricted";
  // challenge/ipp/collect asks to re-enter the phone number ALREADY on the account — we don't hold
  // it, so it is a dead end too. It carries a visible tel field, so it must be caught by URL before
  // the input-shape checks below read it as a rentable "phone" step (that is challenge/iap/verify).
  if (typeof url === "string" && url.includes("challenge/ipp/collect")) return "restricted";
  // challenge/ipp/consent asks the account to consent to a phone/identity check we cannot complete for
  // a fresh account; the generic handler cannot advance it, so recognize it as a dead end by URL
  // instead of spinning on it as an unmapped "challenge".
  if (typeof url === "string" && url.includes("challenge/ipp/consent")) return "restricted";
  // challenge/{iap,ipp}/qrcode makes you scan a QR code with a real phone to prove the device is human
  // — a dead end for automation. Google serves this device check under both the iap and ipp paths.
  // Its URL contains challenge/iap or challenge/ipp, so catch it before the phone step does.
  if (typeof url === "string" && (url.includes("challenge/iap/qrcode") || url.includes("challenge/ipp/qrcode")))
    return "restricted";
  // /signin/rejected is Google refusing the sign-in ("Couldn't sign you in — Google couldn't verify
  // this account belongs to you"). Its only exit is Account Recovery, which asks for the recovery
  // email or phone we do not hold, so it is a dead end. Recognize it by URL so the loop stops instead
  // of re-filling the email on the page it bounces back to.
  if (typeof url === "string" && url.includes("signin/rejected")) return "restricted";
  // challenge/selection ("Verify it's you — choose how you want to sign in") lists recovery methods
  // and ships a hidden-but-visible Passwd input, so it must be claimed by URL before the input-shape
  // checks read it as the password step and spin re-submitting the password.
  if (typeof url === "string" && url.includes("challenge/selection")) return "selection";
  const visible = inputs.filter((i) => i && i.visible !== false);
  const has = (pred) => visible.some(pred);
  if (has((i) => i.name === "totpPin" || i.id === "totpPin")) return "totp";
  if (has((i) => i.name === "backupCode" || i.id === "backupCode")) return "backup";
  // Device-bound code pages we can't answer directly but can route around via "Try another way":
  //   challenge/skotp — a security-key one-time code (field id securityKeyOtpInputId);
  //   challenge/ootp  — "get a security code from your registered phone" (a code only that device shows).
  // We hold neither the key nor the device, but the account's authenticator is reachable from here via
  // "Try another way", so route both to the authenticator switch — and claim them before the tel check
  // below, which would otherwise read their code field as a rentable phone number.
  if (
    (typeof url === "string" && (url.includes("challenge/skotp") || url.includes("challenge/ootp"))) ||
    has((i) => i.id === "securityKeyOtpInputId")
  )
    return "authenticator";
  // The SMS-code page's field (`idvPin`, `idvAnyPhonePin`, or a plain `pin`/`code`) is also `type=tel`,
  // so it must be claimed before the number page (`phoneNumberId`) — otherwise the code box reads as
  // another number prompt and the flow rents a fresh number instead of seeing the code was rejected.
  if (
    has(
      (i) =>
        i.id === "idvPin" ||
        i.name === "idvPin" ||
        i.id === "idvAnyPhonePin" ||
        i.name === "pin" ||
        i.id === "code" ||
        i.name === "code",
    )
  )
    return "phone-code";
  if (has((i) => i.id === "phoneNumberId" || i.type === "tel")) return "phone";
  // Google adds an image-text CAPTCHA to the password page (a visible `ca` answer field beside the
  // password) for a low-trust account/IP. The script fills the password but cannot read the distorted
  // text, so this must be its own node — otherwise it reads as plain "password", re-submits into the
  // captcha error, and spins until it gives up. Claim it before the password check.
  if (has((i) => i.type === "password" || i.name === "Passwd") && has((i) => i.name === "ca" || i.id === "ca"))
    return "password-captcha";
  if (has((i) => i.type === "password" || i.name === "Passwd")) return "password";
  if (has((i) => i.id === "identifierId" || i.name === "identifier" || i.type === "email")) return "identifier";
  if (typeof url === "string" && (url.includes("challenge/") || url.includes("gaia/signin/challenge"))) return "challenge";
  return "unknown";
}

// A password field the user can actually type into. Google's identifier page ships an invisible
// `input[name=hiddenPassword]` decoy, so presence alone (`page.$`) is a false positive there;
// require a VISIBLE password field so the sign-in fills the email first instead of stalling.
async function hasPasswordField(page) {
  try {
    return (await page.locator("input[type=password]:visible").count()) > 0;
  } catch {
    return false; // the page is mid-navigation ("Loading"); not a password field yet
  }
}

// Advance Google's sign-in to the password field, from whichever entry page shows. A pristine
// profile lands on the email page (`input[type=email]`), but a profile that still remembers the
// account lands on the `confirmidentifier` "Verify it's you" page, which has the account prefilled
// and only a Next button — no email field. Handle both: fill the email when present, else click
// the account tile / Next to advance. Throws NeedsHuman if the password field never appears
// (Google is holding a re-verification only a person can clear).
async function reachPasswordPage(page, cred, { assist = false, timeoutS = 20 } = {}) {
  let deadline = Date.now() + timeoutS * 1000;
  let recaptchaTries = 0; // cap the paid CapSolver attempts so a page that never clears can't re-solve forever
  while (Date.now() < deadline) {
    if (await hasPasswordField(page)) return;
    await ensureEnglish(page); // the identifier hop can drop `hl`; "Verify it" below is matched in English
    // Fresh, low-trust accounts hit a reCAPTCHA at challenge/recaptcha before the password field.
    // CapSolver clears it automatically when a key is set; otherwise a headed run lets the person
    // click it and we wait for the password field, and a headless run with no solver escalates.
    if (await onRecaptcha(page)) {
      if (recaptchaTries < 2) {
        recaptchaTries += 1;
        if (await clearRecaptcha(page, cred)) {
          deadline = Date.now() + timeoutS * 1000; // fresh budget for the steps after the CAPTCHA
          if (await hasPasswordField(page)) return;
          continue;
        }
      }
      if (!assist) {
        await debug.capture(page, cred.email, "google-recaptcha");
        throw new NeedsHuman(`${cred.email}: Google is holding a reCAPTCHA — set CAPSOLVER_API_KEY or run headed to clear it by hand`);
      }
      await waitForHuman(page, cred, {
        message: `click the reCAPTCHA for ${cred.email} in the browser`,
        done: async (p) => (await hasPasswordField(p)) || !(await onRecaptcha(p)),
      });
      deadline = Date.now() + timeoutS * 1000; // fresh budget for the steps after the CAPTCHA
      if (await hasPasswordField(page)) return;
      continue;
    }
    const identifierField = await tryFill(page, IDENTIFIER_SELECTOR, cred.email);
    if (identifierField) {
      await submitField(page, identifierField);
      await page.waitForTimeout(2000);
      continue;
    }
    if (page.url().includes("confirmidentifier") || (await page.getByText("Verify it", { exact: false }).count())) {
      // "Verify it's you" prefills the account and advances with Next; the email is only a label
      // here, so click Next first and fall back to an account tile (the multi-account chooser).
      if (!(await clickNext(page))) await clickFirst(page, [cred.email]);
      await page.waitForTimeout(2000);
      continue;
    }
    await page.waitForTimeout(1000);
  }
  if (await hasPasswordField(page)) return;
  // Google is holding a "verify it's you" re-check the script cannot pass on its own (it loops the
  // confirmidentifier page for a real person). On a headed run, hand it to the person at the window and
  // wait until the password field appears or Google takes them straight to the dashboard (a re-verify
  // sometimes skips the password). `done` is URL/DOM-only so it never navigates a page mid-interaction.
  if (assist) {
    await waitForHuman(page, cred, {
      message: `clear the 'verify it's you' step for ${cred.email} in the browser`,
      done: async (p) => (await hasPasswordField(p)) || isSignedInUrl(p.url()),
    });
    if (await hasPasswordField(page)) return;
    if (isSignedInUrl(page.url())) return; // re-verify completed the sign-in without a password step
  }
  await debug.capture(page, cred.email, "google-verify-its-you");
  throw new NeedsHuman(`${cred.email}: Google is holding a 'verify it's you' re-check that needs a human`);
}

// True when Google is showing an interactive reCAPTCHA challenge — the dedicated challenge page, or
// the visible checkbox (its api2/anchor iframe). Deliberately narrow so the invisible reCAPTCHA v3
// that ordinary pages embed for risk scoring does not read as a challenge.
async function onRecaptcha(page) {
  if (page.url().includes("challenge/recaptcha")) return true;
  try {
    // Require the checkbox iframe to be VISIBLE. The shared traversal probes this every tick on every
    // page (password, TOTP, phone), and Google embeds a hidden api2/anchor for risk scoring on those —
    // a mere presence check would misfire a "click the reCAPTCHA" handoff where there is nothing to click.
    return await page.locator("iframe[src*='recaptcha/api2/anchor']").first().isVisible().catch(() => false);
  } catch {
    return false;
  }
}

// The #recaptcha-anchor checkbox of the ONE visible api2/anchor iframe, or null. Google embeds
// several anchor iframes (hidden ones for risk scoring), so match on the iframe ELEMENTS — their src
// carries api2/anchor even while the frame is still loading, which a page.frames() url filter misses
// during that window — and reach into the one whose checkbox is actually visible. Clicking a hidden
// frame's checkbox would be a click into nothing.
export async function visibleRecaptchaAnchor(page) {
  const frameEls = await page
    .locator("iframe[src*='recaptcha/api2/anchor'], iframe[src*='recaptcha/enterprise/anchor']")
    .all();
  for (const frameEl of frameEls) {
    if (!(await frameEl.isVisible().catch(() => false))) continue;
    const cb = frameEl.contentFrame().locator("#recaptcha-anchor");
    if (await cb.isVisible().catch(() => false)) return cb;
  }
  return null;
}

// True when the reCAPTCHA image-challenge popup is open — a bframe whose grid image is visible.
// Reusing visibleBframe makes the "is the grid up" check match the frame the solver actually drives,
// rather than an iframe-element visibility probe that misses the popup.
export async function recaptchaGridOpen(page) {
  return (await visibleBframe(page)) !== null;
}

const RECAPTCHA_POLL_MS = 200; // how often the auto reCAPTCHA steps re-check the widget/page state
const RECAPTCHA_WIDGET_BUDGET_MS = 15000; // how long to wait for the clicked checkbox to tick or open a grid
const RECAPTCHA_ADVANCE_BUDGET_MS = 12000; // how long to wait for a ticked challenge to leave the page
const RECAPTCHA_ANCHOR_BUDGET_MS = 15000; // how long to wait for the visible checkbox iframe to render
const RECAPTCHA_VERIFY_BUDGET_MS = 3000; // how long a pressed Verify has to close or swap the grid
const RECAPTCHA_RELOAD_BUDGET_MS = 2000; // how long a reload has to swap a fresh grid in
const RECAPTCHA_SWAP_BUDGET_MS = 3000; // how long a clicked dynamic selection has to swap before it counts as final (provisional — re-tune once a live grid timing run lands)

// Poll `state(page)` on a tight cadence until it returns a truthy value or `timeoutMs` elapses, and
// return that first truthy value (or null on timeout). The value is the caller's signal — a tag it
// switches on, or the matched locator — so one helper serves the "ticked / grid / password", the
// "page advanced", and the "checkbox appeared" waits without each spinning its own coarse loop.
// Checks before the first sleep, so a state already met costs no wait. Deliberately does NOT catch:
// a page-closed throw propagates to the caller's try/catch, which falls back to the human — the same
// path the old timed loops took.
export async function pollForState(page, state, timeoutMs) {
  let waited = 0;
  for (;;) {
    const value = await state();
    if (value) return value;
    if (waited >= timeoutMs) return null;
    await page.waitForTimeout(RECAPTCHA_POLL_MS);
    waited += RECAPTCHA_POLL_MS;
  }
}

// Submit the sign-in page after the reCAPTCHA is ticked and wait for it to actually leave the
// challenge (URL moves, the password field appears, or the anchor is gone). Returns whether it
// advanced, so the caller reports success only on real progress.
export async function submitRecaptcha(page, cred) {
  // The checkbox pass may land straight on the password page — do NOT click Next there, or it submits
  // an empty password and Google shows the "enter your password" hint. Report success and let the
  // traversal type the password.
  if (await hasPasswordField(page)) {
    console.log(`  captcha: reCAPTCHA cleared for ${cred.email}`);
    return true;
  }
  const at = page.url();
  // A Next on the challenge page advances to the password step; only click it while still on the
  // reCAPTCHA, never on a later form.
  if (await onRecaptcha(page)) await clickNext(page).catch(() => {});
  // Hold the same ~12s window for the challenge to actually leave the page, but notice the move on the
  // tight poll instead of in 1s steps. The positive signals (URL moved, password field up) are trusted
  // on every poll; the negative "the reCAPTCHA is gone" is only read from the second poll on, so a
  // checkbox iframe still settling in the instant after the Next click cannot be misread as cleared.
  let firstPoll = true;
  const advanced = await pollForState(
    page,
    async () => {
      if (page.url() !== at || (await hasPasswordField(page))) return true;
      if (firstPoll) {
        firstPoll = false;
        return false;
      }
      return !(await onRecaptcha(page));
    },
    RECAPTCHA_ADVANCE_BUDGET_MS,
  );
  if (advanced) {
    console.log(`  captcha: reCAPTCHA cleared for ${cred.email}`);
    return true;
  }
  console.log(`  captcha: reCAPTCHA ticked but the page did not advance for ${cred.email}`);
  return false;
}

// Try to clear a reCAPTCHA by driving the real widget. Google gates form submission on the widget's
// own solved-state (the widget lives in an iframe; a CapSolver token injected into the page textarea
// is never consumed), so the only thing that works is to act on the widget itself: click the
// checkbox — on a good Camoufox fingerprint + residential exit it often passes with no image
// challenge — and if Google then shows the image grid, solve that with CapSolver's tile
// classification. Returns true only when the flow actually leaves the reCAPTCHA. Never throws: any
// failure degrades to the human `--assist` click.
async function clearRecaptcha(page, cred) {
  try {
    // A grid may already be open (a retry, or a previous click that expanded it) — solve it directly
    // instead of clicking the checkbox again, which would reset the challenge.
    if (await recaptchaGridOpen(page)) {
      console.log(`  captcha: image grid already open for ${cred.email} — solving with CapSolver classification`);
      if (await solveRecaptchaGrid(page, cred)) return await submitRecaptcha(page, cred);
      return false;
    }
    // The visible checkbox iframe can take a few seconds to render, so hold the same ~15s window,
    // noticed on the tight poll rather than in 500ms steps.
    const anchor = await pollForState(page, () => visibleRecaptchaAnchor(page), RECAPTCHA_ANCHOR_BUDGET_MS);
    if (!anchor) {
      const urls = page.frames().map((f) => f.url()).filter((u) => u.includes("recaptcha"));
      console.log(`  captcha: no visible checkbox found for ${cred.email}; recaptcha frames=${JSON.stringify(urls)}`);
      return false;
    }
    console.log(`  captcha: clicking the reCAPTCHA checkbox for ${cred.email}`);
    await anchor.click({ timeout: 5000 }).catch((e) => console.log(`  captcha: checkbox click failed: ${e.message}`));
    // Wait for the widget to either tick (passed outright) or open the image grid. The grid can take
    // several seconds to render, so hold the same ~15s window — just notice the change on a tight poll
    // instead of in 1s steps. The password field is checked too: the checkbox may pass and the page
    // move straight on to it, and we must stop polling the now-detached anchor.
    const outcome = await pollForState(
      page,
      async () => {
        if (await hasPasswordField(page)) return "password";
        if ((await anchor.getAttribute("aria-checked").catch(() => null)) === "true") return "ticked";
        if (await recaptchaGridOpen(page)) return "grid";
        return null;
      },
      RECAPTCHA_WIDGET_BUDGET_MS,
    );
    if (outcome === "password") {
      console.log(`  captcha: checkbox passed for ${cred.email}`);
      return true;
    }
    if (outcome === "ticked") {
      console.log(`  captcha: checkbox passed with no image challenge for ${cred.email}`);
      return await submitRecaptcha(page, cred);
    }
    if (outcome === "grid") {
      console.log(`  captcha: image grid appeared for ${cred.email} — solving with CapSolver classification`);
      if (await solveRecaptchaGrid(page, cred)) return await submitRecaptcha(page, cred);
      return false;
    }
    // Neither ticked nor grid within the window — the page may already have advanced.
    return await submitRecaptcha(page, cred);
  } catch (e) {
    console.log(`  captcha: could not clear the reCAPTCHA for ${cred.email}: ${e.message}`);
    return false;
  }
}

// Read one record per grid tile straight from the bframe DOM: whether the cell owns an image, whether
// that image has finished downloading (`complete`), its decoded pixel width, and its rendered opacity
// (the min of the image and its wrapper, since a dynamic replacement tile fades in via a CSS opacity
// transition). This is the browser's own answer to "is the picture actually there yet", not a guess
// from a screenshot or a fixed delay. Returns [] if the grid is gone.
async function gridTileRecords(bframe) {
  return bframe
    .evaluate(() =>
      [...document.querySelectorAll("td.rc-imageselect-tile")].map((td) => {
        const img = td.querySelector("img");
        if (!img) return { hasImg: false, complete: false, naturalWidth: 0, opacity: 0 };
        const op = (el) => {
          const v = parseFloat(getComputedStyle(el).opacity);
          return Number.isNaN(v) ? 1 : v;
        };
        return {
          hasImg: true,
          complete: img.complete,
          naturalWidth: img.naturalWidth,
          opacity: Math.min(op(img), op(img.parentElement || td)),
        };
      }),
    )
    .catch(() => []);
}

// Block until every tile is fully rendered — downloaded, decoded, and done fading in — so the grid is
// never screenshotted or clicked while a picture is still grainy or a swapped tile is still fading. The
// gate is DOM truth (captcha.gridReady over per-tile records), not a timed wait: an image that loads
// later than any fixed delay would still be caught. Once ready, await decode() and one paint so the
// screenshot captures the final pixels. Returns true when ready, false if it never settled in time.
async function waitGridLoaded(bframe, timeoutMs = 20000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (captcha.gridReady(await gridTileRecords(bframe))) {
      await bframe
        .evaluate(async () => {
          const imgs = [...document.querySelectorAll("td.rc-imageselect-tile img")];
          await Promise.all(imgs.map((i) => i.decode().catch(() => {})));
          await new Promise((r) => requestAnimationFrame(() => r()));
        })
        .catch(() => {});
      return true;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
}

// After clicking dynamic-challenge matches, wait for the grid to actually swap those cells in new
// images — the real signal that the challenge wants another pass, read from the DOM rather than timed.
// reCAPTCHA marks a chosen tile with rc-imageselect-tileselected and clears it when the fresh image
// loads, so a clicked tile that loses that class (or a grid that closed outright) means the reload
// began. Returns true if any clicked tile reloaded (→ re-check next round), false if the selection
// stayed put until the deadline (→ the selection is final, press Verify).
export async function waitTilesSwapped(bframe, page, idxs, timeoutMs = RECAPTCHA_SWAP_BUDGET_MS) {
  const tiles = bframe.locator("td.rc-imageselect-tile");
  // A swap is noticed the instant a clicked tile loses its selected class (or the grid closes); only a
  // selection that stays put the whole window counts as final. The old 8s was dead time on every
  // dynamic challenge's last round — the swap, when it comes, lands fast. Poll on the shared cadence; a
  // page closing mid-wait throws out to solveRecaptchaGrid's catch, same as every other reCAPTCHA wait.
  const swapped = await pollForState(
    page,
    async () => {
      if (!(await recaptchaGridOpen(page))) return true; // grid closed — accepted
      for (const idx of idxs) {
        const cls = (await tiles.nth(idx).getAttribute("class").catch(() => "")) || "";
        if (!cls.includes("rc-imageselect-tileselected")) return true; // this tile reloaded
      }
      return null;
    },
    timeoutMs,
  );
  return swapped === true;
}

// The visible reCAPTCHA image-challenge frame (bframe), or null. Google keeps a hidden bframe on the
// page, so pick the one whose payload image is actually visible.
async function visibleBframe(page) {
  for (const f of page.frames()) {
    if (!/recaptcha\/(api2|enterprise)\/bframe/.test(f.url())) continue;
    const img = f.locator("img.rc-image-tile-44, img.rc-image-tile-33").first();
    if (await img.isVisible().catch(() => false)) return f;
  }
  return null;
}

// The src of the live grid's payload image, or null when no grid is up. reCAPTCHA serves a fresh
// payload URL for each challenge, so a change in this src is the DOM's own "the grid reloaded / swapped
// in a new one" signal — read it rather than guess from a timer.
export async function currentGridSrc(page) {
  const bframe = await visibleBframe(page);
  if (!bframe) return null;
  return await bframe
    .locator("img.rc-image-tile-44, img.rc-image-tile-33")
    .first()
    .getAttribute("src")
    .catch(() => null);
}

// After clicking a bframe button (Verify or the reload arrow), wait until the grid we just acted on is
// no longer the live one: the challenge closed (solved → "closed"), or its payload image was replaced
// (rejected, a fresh reload, or the next challenge in a gauntlet → "changed"). Returns null if neither
// happens within the window, so the caller degrades to its existing next-round re-read. A grid mid-swap
// can read as closed for a single tick (the old image detaches before the new one paints), so "closed"
// must hold on two consecutive polls before it counts.
export async function waitGridChanged(page, { prevSrc, timeoutMs }) {
  let closedLastPoll = false;
  return await pollForState(
    page,
    async () => {
      if (!(await recaptchaGridOpen(page))) {
        if (closedLastPoll) return "closed";
        closedLastPoll = true;
        return null;
      }
      closedLastPoll = false;
      const src = await currentGridSrc(page);
      if (prevSrc && src && src !== prevSrc) return "changed";
      return null;
    },
    timeoutMs,
  );
}

// Solve the reCAPTCHA image grid with CapSolver's per-tile classification. Each round waits for the
// grid to finish rendering, screenshots every unselected tile element, and classifies them in parallel
// with CapSolver's single-tile mode — which returns a real yes/no per tile, unlike the whole-grid mode
// that always names ~3 tiles and never reports "none". Matching tiles are clicked at a human pace. Two
// challenge shapes: the static "select all squares … if there are none click skip" is verified in one
// pass; the dynamic "select all images … click verify once there are none left" replaces clicked tiles,
// so it loops — clicking matches, waiting for the swap, re-checking — and presses Verify once a round
// finds no matches (the yes/no mode makes that "none" reachable). Bounded rounds, then the human.
// Returns true once the challenge closes. Never throws.
export async function solveRecaptchaGrid(page, cred) {
  if (!config.capSolverKey()) return false;
  try {
    let prevSig = null;
    let prevObject = null;
    let roundsThisChallenge = 0;
    let reloads = 0;
    const maxReloads = 3;
    const challengeBudget = 5; // rounds on one challenge before refreshing it for a fresh grid
    for (let round = 0; round < 20; round++) {
      const bframe = await visibleBframe(page);
      if (!bframe) return !(await recaptchaGridOpen(page)); // grid gone → solved
      const desc = (await bframe.locator(".rc-imageselect-desc-no-canonical, .rc-imageselect-desc").first().innerText().catch(() => "")).toLowerCase();
      const dynamic = /verify once|none left|new images/.test(desc);
      const object = await bframe
        .locator(".rc-imageselect-desc-no-canonical strong, .rc-imageselect-desc strong")
        .first()
        .innerText()
        .catch(() => "");
      if (!object) {
        console.log(`  captcha: could not read the grid prompt for ${cred.email} — falling back to human`);
        return false;
      }
      // A multi-challenge gauntlet swaps the prompt (buses → motorcycles → …); a fresh prompt is real
      // progress, so reset this challenge's round budget whenever the object changes.
      if (object !== prevObject) {
        roundsThisChallenge = 0;
        prevSig = null;
      }
      prevObject = object;
      const tiles = bframe.locator("td.rc-imageselect-tile");
      const tileCount = await tiles.count();
      const gridN = tileCount >= 16 ? 4 : 3;
      // Wait for every tile to finish rendering before screenshotting — classifying a grainy or
      // half-faded grid is what made it click blank cells. DOM truth (gridReady), not a fixed delay.
      if (!(await waitGridLoaded(bframe))) {
        console.log(`  captcha: grid never finished loading for ${cred.email} — falling back to human`);
        return false;
      }
      // Classify the whole grid with a vision model (classifyImage): it sees the full picture, so it
      // handles an object spanning several cells and can report "none" — both of which CapSolver's grid
      // and per-tile classifiers get wrong. Screenshot the table and ask which cells hold the object.
      const table = bframe.locator("table[class*='rc-imageselect-table']").first();
      const shot = await table.screenshot().catch(() => null);
      if (!shot) return false;
      const tClassify = Date.now();
      const result = await classifyImage(shot, { object, gridN });
      if (process.env.SM_DEBUG_GRIDS) console.log(`  captcha[dbg]: classify round ${round + 1} (${object}, ${dynamic ? "dynamic" : "static"}) took ${Date.now() - tClassify}ms`);
      if (!result) {
        console.log(`  captcha: vision classify failed for ${cred.email} — falling back to human`);
        return false;
      }
      // On a headed debug run, save the exact image sent to the classifier, named with its answer, so
      // the classifier's accuracy can be checked by eye and re-tested offline afterward.
      if (process.env.SM_DEBUG_GRIDS) {
        const name = `grid-${object.replace(/[^a-z0-9]+/gi, "_")}-r${round + 1}-cells_${result.cells.join("-") || "none"}.png`;
        debug.write(cred.email, name, shot);
      }
      // Drop cells already chosen — clicking a selected tile only deselects it.
      const matches = [];
      for (const idx of result.cells) {
        const cls = (await tiles.nth(idx).getAttribute("class").catch(() => "")) || "";
        if (!cls.includes("rc-imageselect-tileselected")) matches.push(idx);
      }
      console.log(`  captcha: "${object}" → tiles ${JSON.stringify(matches)} of ${tileCount} (round ${round + 1})`);
      const sig = `${object}:${JSON.stringify(matches)}`;
      const spinning = sig === prevSig && matches.length > 0; // same non-empty pick as the last round
      // A garbage over-inclusive answer: the model named more than half the grid, which no real object
      // spans. Clicking most of the grid fails the challenge and looks like a bot, so discard this round
      // and refresh rather than acting on it.
      const implausible = result.cells.length > tileCount / 2;
      if (implausible) console.log(`  captcha: implausible ${result.cells.length}/${tileCount} match for "${object}" (${cred.email}) — discarding`);
      prevSig = sig;
      roundsThisChallenge += 1;
      // When a challenge won't converge — the model keeps missing a tile so Verify is rejected with
      // "please select all matching images", loops on the same pick, or returns garbage — refreshing gets
      // a fresh, often easier grid. Reload a bounded number of times, then hand off to the human.
      if (implausible || spinning || roundsThisChallenge >= challengeBudget) {
        if (reloads < maxReloads) {
          reloads += 1;
          console.log(`  captcha: no progress on "${object}" for ${cred.email} — reloading the challenge (reload ${reloads}/${maxReloads})`);
          const reloadingSrc = await currentGridSrc(page);
          await bframe.locator("#recaptcha-reload-button").click({ timeout: 3000 }).catch(() => {});
          // Wait for the reload to actually swap the grid in before the next round reads it, so
          // visibleBframe cannot screenshot the stale grid. DOM truth (the payload src changed), not a
          // fixed 1.2s.
          await waitGridChanged(page, { prevSrc: reloadingSrc, timeoutMs: RECAPTCHA_RELOAD_BUDGET_MS });
          roundsThisChallenge = 0;
          prevSig = null;
          prevObject = null;
          continue;
        }
        console.log(`  captcha: gave up on "${object}" for ${cred.email} after ${reloads} reloads — falling back to human`);
        return false;
      }
      // Click each match at a human pace: move the cursor in a few steps, pause a random beat, click
      // with a short randomized press, and leave a randomized gap — a burst of instant identical clicks
      // is the opposite of how a person selects tiles.
      for (const idx of matches) {
        const tile = tiles.nth(idx);
        const box = await tile.boundingBox().catch(() => null);
        if (box) {
          await page.mouse.move(
            box.x + box.width * (0.3 + Math.random() * 0.4),
            box.y + box.height * (0.3 + Math.random() * 0.4),
            { steps: 4 + Math.floor(Math.random() * 6) },
          );
        }
        await page.waitForTimeout(70 + Math.floor(Math.random() * 110));
        await tile.click({ timeout: 3000, delay: 30 + Math.floor(Math.random() * 70) }).catch(() => {});
        await page.waitForTimeout(110 + Math.floor(Math.random() * 220));
      }
      if (dynamic && matches.length > 0) {
        // Fresh matches selected. If the grid swaps new images into those tiles, there may be more of
        // the object to find — re-check next round (the load gate waits for the swap to finish). If it
        // does NOT swap (the selection is final), fall through and press Verify.
        if (await waitTilesSwapped(bframe, page, matches)) continue;
      }
      // Static grid, or a dynamic grid with nothing left to select (matches empty → "none") — submit.
      const verifiedSrc = await currentGridSrc(page);
      await bframe.locator("#recaptcha-verify-button").click({ timeout: 3000 }).catch(() => {});
      // Solved the instant the grid closes; a rejected Verify instead swaps the grid or shows the next
      // challenge, which the next round re-reads. DOM truth, not a fixed 3s.
      const verdict = await waitGridChanged(page, { prevSrc: verifiedSrc, timeoutMs: RECAPTCHA_VERIFY_BUDGET_MS });
      if (process.env.SM_DEBUG_GRIDS) {
        let errShown = "none";
        for (const s of [".rc-imageselect-error-select-more", ".rc-imageselect-error-dynamic-more", ".rc-imageselect-incorrect-response"]) {
          const disp = await bframe.locator(s).first().evaluate((el) => getComputedStyle(el).display).catch(() => "none");
          if (disp && disp !== "none") { errShown = s; break; }
        }
        console.log(`  captcha[dbg]: verify round ${round + 1} → ${verdict ?? "null"}; error=${errShown}`);
      }
      if (verdict === "closed") {
        console.log(`  captcha: grid solved for ${cred.email}`);
        return true;
      }
    }
    console.log(`  captcha: grid not solved for ${cred.email} — falling back to human`);
    return false;
  } catch (e) {
    console.log(`  captcha: grid solve failed for ${cred.email}: ${e.message}`);
    return false;
  }
}

// Try to read the password-page distorted-text CAPTCHA with CapSolver and fill the `ca` answer field.
// Returns true when the field was filled (the caller then submits the password). False (no key, no
// image, or a solver failure) falls back to the human. Never throws.
async function clearPasswordCaptcha(page, cred) {
  try {
    const img = page.locator("img#captchaimg, img[src*='Captcha'], img[src*='captcha']").first();
    if (!(await img.isVisible().catch(() => false))) return false;
    const shot = await img.screenshot();
    // Read the distorted characters with the same vision model that solves the reCAPTCHA grid; the
    // model sees the whole warped string at once, where CapSolver's OCR kept failing on these.
    const result = await readImageText(shot);
    if (!result) {
      console.log(`  captcha: vision could not read the password-page CAPTCHA for ${cred.email}`);
      return false;
    }
    if (!(await tryFill(page, "input[name=ca], input#ca", result.text))) return false;
    console.log(`  captcha: filled the password-page text CAPTCHA for ${cred.email} (vision)`);
    return true;
  } catch (e) {
    console.log(`  captcha: could not read the password-page CAPTCHA for ${cred.email}: ${e.message}`);
    return false;
  }
}

// Sign the account into Google. A profile that already carries a live session is redirected from
// the sign-in URL straight to the dashboard, and there is nothing more to do. Otherwise reaches the
// password field (clearing a pre-password reCAPTCHA or "verify it's you" on a headed run), then
// hands the rest of the sign-in graph — password, TOTP, phone, and any other challenge — to the
// shared traversal. Throws NeedsHuman on a dead-end a person cannot or did not clear.
export async function signInGoogle(page, cred, { assist = false } = {}) {
  // Force English (hl=en) so the sign-in buttons ("Next", password, challenges) carry their English
  // names regardless of the account's own UI language — the vendor's accounts default to non-English.
  await gotoWithRetry(page, "https://accounts.google.com/ServiceLogin?hl=en");
  if (isSignedInUrl(page.url())) return;
  await reachPasswordPage(page, cred, { assist });
  // A "verify it's you" step cleared by hand on a headed run can land straight on the dashboard, with no
  // password field to fill — don't try to submit one.
  if (isSignedInUrl(page.url())) return;
  // Right after a reCAPTCHA the password page is still hydrating (its body shows "Loading"); let it
  // settle before the traversal's password node submits (that node re-submits if the first click
  // raced the reload).
  await page.waitForTimeout(1500);

  await driveGoogleChallenges(page, cred, { assist, done: (s) => isSignedInUrl(s.url()) });

  // Google often drops a fresh sign-in onto its optional setup wizard (gds.google.com/web/*: add a
  // recovery phone, set a home address, …) before the dashboard. The account is already signed in,
  // so skip the whole chain of cards at once by going straight to the dashboard.
  if (isOnboardingUrl(page.url())) {
    await page.goto("https://myaccount.google.com/", { waitUntil: "domcontentloaded" }).catch(() => {});
  }
  if (!isSignedInUrl(page.url())) {
    await debug.capture(page, cred.email, "google-login-incomplete");
    throw new NeedsHuman(`${cred.email}: Google login did not complete`);
  }
}

// Drive whatever node of the Google sign-in graph the surface is on — one node per tick — until the
// caller's `done` (signed into Google, or the app OAuth left Google) or a dead-end. Both the Google
// login and the app-OAuth handshake share this one traversal, so a challenge is handled the same way
// wherever it appears (a cold OAuth session that drops into a full login is covered for free).
//
// The flow drives every node it can on its own — password, TOTP, and the phone step (always via a
// rented HeroSMS number). `assist` = a person is at the window (a headed run), so the one thing the
// script genuinely cannot do — clear a reCAPTCHA image challenge, or type a real phone number when
// HeroSMS is exhausted — is handed to them in the window and the run resumes once the URL moves. It
// is derived from `headed`, never a per-challenge flag. Throws NeedsHuman on a dead-end no one clears.
async function driveGoogleChallenges(surface, cred, { assist = false, done, timeoutMs = 120000 } = {}) {
  const isDone = () => {
    try {
      return surface.isClosed() || done(surface);
    } catch {
      return true;
    }
  };
  // Hand the current step to the person, then re-dispatch on whatever comes next: resume when the
  // caller is done or the URL has moved off the page we handed off at. URL/DOM-only, so it never
  // navigates a page a human is mid-interaction on.
  const handOff = async (message) => {
    const at = safeUrl(surface);
    return waitForHuman(surface, cred, {
      message,
      done: (s) => {
        try {
          return isDone() || s.url() !== at;
        } catch {
          return true;
        }
      },
    });
  };
  let waited = 0;
  let stuck = 0;
  let passwordTries = 0;
  let identifierTries = 0;
  let recaptchaTries = 0; // cap the paid CapSolver reCAPTCHA attempts per traversal
  let captchaTries = 0; // cap the paid CapSolver password-page image reads per traversal
  let lastTotp = null; // the last TOTP code we submitted, so we only re-enter when the 30s window rolls
  let choseAccount = false;
  while (waited < timeoutMs) {
    if (isDone()) return;
    await ensureEnglish(surface); // every node below matches English button names

    // Inline guards for signals that are not input shapes (so they are not in classifyGoogleNode):
    // an error modal, a wrong-credential banner, the reCAPTCHA checkbox iframe.
    if (await hasErrorModal(surface)) {
      await debug.capture(surface, cred.email, "google-error-modal");
      if (assist && (await handOff(`clear the Google error for ${cred.email} in the window`))) continue;
      throw new NeedsHuman(`${cred.email}: Google returned an error at ${safeUrl(surface)}`);
    }
    const wrong = await hasWrongCredential(surface);
    if (wrong === "password") {
      await debug.capture(surface, cred.email, "google-password-rejected");
      throw new NeedsHuman(`${cred.email}: Google rejected the password (wrong password on file)`);
    }
    if (wrong === "totp") {
      await debug.capture(surface, cred.email, "google-totp-wrong");
      if (assist && (await handOff(`enter a working 2FA code for ${cred.email} in the window`))) continue;
      throw new NeedsHuman(`${cred.email}: Google rejected the 2FA code (stale TOTP secret?) at ${safeUrl(surface)}`);
    }
    if (await onRecaptcha(surface)) {
      await debug.capture(surface, cred.email, "google-recaptcha");
      if (recaptchaTries < 2) {
        recaptchaTries += 1;
        if (await clearRecaptcha(surface, cred)) continue;
      }
      if (assist && (await handOff(`click the reCAPTCHA for ${cred.email} in the window`))) continue;
      throw new NeedsHuman(`${cred.email}: Google is holding a reCAPTCHA — set CAPSOLVER_API_KEY or run headed to clear it by hand`);
    }

    const inputs = await inputInventory(surface);
    const node = classifyGoogleNode(safeUrl(surface), inputs);
    console.log(`  node=${node} at ${safeUrl(surface)} inputs=${JSON.stringify(inputs)}`);
    let progressed = true;
    switch (node) {
      case "signed-in":
      case "onboarding":
        // Reached the account (dashboard or the skippable setup wizard). The caller's `done` usually
        // returned already; otherwise nudge to the dashboard.
        await surface.goto("https://myaccount.google.com/", { waitUntil: "domcontentloaded" }).catch(() => {});
        break;
      case "restricted":
        // Terminal dead end: the account is banned (disabled speedbump), the sign-in is rejected
        // (only Account Recovery remains) or Google demands a factor we cannot supply (confirm the
        // existing recovery number). No point waiting for a human or retrying — capture it and stop
        // so the caller can mark it restricted.
        await debug.capture(surface, cred.email, "google-restricted");
        throw new Restricted(`${cred.email}: Google restricted this account at ${safeUrl(surface)}`);
      case "authenticator":
        // Google's security-key OTP page. We hold no key, but the account's authenticator is reachable
        // via "Try another way"; switch to it and let the totp node fill the code next tick. With no
        // secret (or no authenticator path) it is a genuine dead end.
        if (cred.totp_secret && (await switchToAuthenticator(surface))) break;
        await debug.capture(surface, cred.email, "google-authenticator-switch-failed");
        if (assist && (await handOff(`switch to the authenticator for ${cred.email} in the window`))) continue;
        throw new Restricted(`${cred.email}: Google wants a security key and no authenticator path is available at ${safeUrl(surface)}`);
      case "selection":
        // The "Verify it's you — choose how you want to sign in" chooser. Prefer the authenticator when
        // this account carries a TOTP secret — the chooser lists it directly. Otherwise none of its
        // methods are scriptable here (recovery-email code, another device, …): if it offers a
        // recovery-email path a person could still finish it, so escalate; with no such path there is
        // nothing anyone can do, so it is a dead end — restrict. The recovery address is read off the page.
        await debug.capture(surface, cred.email, "google-verify-selection");
        if (cred.totp_secret && (await switchToAuthenticator(surface))) break;
        if (await hasRecoveryEmailOption(surface)) {
          throw new NeedsHuman(`${cred.email}: Google's verify-it's-you chooser needs a recovery email at ${safeUrl(surface)}`);
        }
        throw new Restricted(`${cred.email}: Google's verify-it's-you chooser offers no path we can take at ${safeUrl(surface)}`);
      case "identifier":
        // Google often lands back on the identifier page with the email already prefilled (e.g. after
        // a reCAPTCHA, or on 'confirm it's you'). A prefilled field must still click Next, so mirror
        // the password step: (re)fill only when it differs, always click Next, bounded so an unadvancing
        // page is treated as stuck instead of looping forever.
        progressed = identifierTries < 3 && (await submitIdentifier(surface, cred.email));
        if (progressed) identifierTries += 1;
        break;
      case "password":
        // The first Next after a reCAPTCHA races the password page's reload, so allow a couple of
        // submits before treating the page as stuck rather than one shot.
        progressed = passwordTries < 3 && (await submitPassword(surface, cred.password));
        if (progressed) passwordTries += 1;
        break;
      case "password-captcha":
        // Google put an image-text CAPTCHA on the password page (low-trust account/IP). Fill the
        // password first, then let CapSolver read the distorted text and fill the `ca` field and
        // submit. With no solver a headed run hands off for a person to type it, and a headless run
        // escalates (the caller marks the account escalated on NeedsHuman).
        await tryFill(surface, "input[type=password]", cred.password);
        if (captchaTries < 2) {
          captchaTries += 1;
          if (await clearPasswordCaptcha(surface, cred)) {
            await clickNext(surface);
            break;
          }
        }
        await debug.capture(surface, cred.email, "google-password-captcha");
        if (assist && (await handOff(`solve the image CAPTCHA on the password page for ${cred.email} in the window`))) continue;
        throw new NeedsHuman(`${cred.email}: Google is holding an image CAPTCHA on the password page — set CAPSOLVER_API_KEY or run headed to type it by hand`);
      case "totp": {
        if (!cred.totp_secret) {
          if (assist && (await handOff(`clear the 2-Step step for ${cred.email} in the window`))) continue;
          throw new NeedsHuman(`${cred.email}: TOTP requested but no secret on file`);
        }
        // Enter the code once per 30s window: if we are still on the TOTP page in the same window, wait
        // for a fresh code rather than re-submitting the same one; a truly stale secret surfaces as the
        // wrong-code banner (handled above) instead of spinning here.
        const code = computeTotp(cred.totp_secret);
        const totpField = code !== lastTotp ? await tryFill(surface, "input[name=totpPin]", code) : null;
        if (totpField) {
          lastTotp = code;
          await submitField(surface, totpField);
        } else {
          progressed = false;
        }
        break;
      }
      case "phone":
        // Always try to clear the phone step automatically with a rented HeroSMS number (it
        // auto-refunds if no code arrives, so a rejected attempt costs ~nothing). Only when every
        // preferred country is exhausted does a person — if the run is headed — type a real number by
        // hand; headless, it escalates.
        try {
          await passPhoneChallenge(surface, cred);
        } catch (e) {
          if (!(e instanceof NeedsHuman)) throw e;
          if (assist && (await handOff(`enter a phone number for ${cred.email} in the window`))) continue;
          throw e;
        }
        break;
      case "phone-code":
        // Google is on the SMS-code page. passPhoneChallenge already typed the code and Google balked
        // (or re-prompted); either way the script cannot recover it, so hand off (headed) / escalate.
        await debug.capture(surface, cred.email, "google-phone-code");
        if (assist && (await handOff(`enter the SMS code for ${cred.email} in the window`))) continue;
        throw new NeedsHuman(`${cred.email}: Google is on the SMS-code step and it was not completed`);
      case "backup":
        await debug.capture(surface, cred.email, "google-backup-code");
        if (assist && (await handOff(`enter a backup code for ${cred.email} in the window`))) continue;
        throw new NeedsHuman(`${cred.email}: Google asked for a backup code, none on file`);
      default:
        // An unmapped page. Before treating it as a dead-end, try the moves that carry no dedicated
        // node: switch a non-TOTP 2-Step method to the authenticator, click an OAuth consent button,
        // or pick this account's chooser tile (once).
        if (cred.totp_secret && !lastTotp && (await switchToAuthenticator(surface))) break;
        if (await clickRole(surface, ["Continue", "Allow", "Confirm", "Weiter"])) break;
        if (!choseAccount && (await clickFirst(surface, [cred.email]))) {
          choseAccount = true;
          break;
        }
        progressed = false;
    }

    stuck = progressed ? 0 : stuck + 1;
    if (stuck >= 6) {
      console.log(`  unknown-challenge inputs at ${safeUrl(surface)}: ${JSON.stringify(await inputInventory(surface))}`);
      await debug.capture(surface, cred.email, "google-challenge");
      if (assist && (await handOff(`finish the Google step for ${cred.email} in the window`))) {
        stuck = 0;
        continue;
      }
      throw new NeedsHuman(`${cred.email}: unsupported Google challenge at ${safeUrl(surface)}`);
    }
    try {
      await surface.waitForTimeout(2000);
    } catch {
      return; // the surface (an OAuth popup) closed — the caller's ready-check decides
    }
    waited += 2000;
  }
  await debug.capture(surface, cred.email, "google-timeout");
  if (assist && (await handOff(`finish the Google step for ${cred.email} in the window`))) return;
  throw new NeedsHuman(`${cred.email}: Google sign-in did not finish in ${timeoutMs}ms at ${safeUrl(surface)}`);
}

// Every input on the page (name/type/id/placeholder/visible) — for capturing an unmapped step.
async function inputInventory(page) {
  return page
    .evaluate(() =>
      [...document.querySelectorAll("input")].map((e) => ({
        name: e.name,
        type: e.type,
        id: e.id,
        placeholder: e.placeholder,
        visible: e.offsetParent !== null,
      })),
    )
    .catch(() => []);
}

// The most we will pay HeroSMS for one Google-verification number.
const PHONE_MAX_PRICE = 0.1;

// Google's phone-verification step (challenge/iap): rent a number from HeroSMS, enter it, read the
// SMS code and submit it. The countries to try are read live from HeroSMS's own price list — every
// country that stocks a Google (`go`) number at or under PHONE_MAX_PRICE, cheapest first, minus the
// blacklist (a country that never delivers Google's code) and any country whose dial code we do not
// know (we could not split its number for Google's national field). A number Google rejects at entry,
// or one that never receives the code, is cancelled (refunded) and the next is tried. Throws
// NeedsHuman if none works. HeroSMS auto-refunds any number that gets no SMS in 20 min.
async function passPhoneChallenge(page, cred) {
  let candidates = [];
  try {
    const prices = await smsOtp.getPrices({ service: smsOtp.GOOGLE_SERVICE });
    candidates = smsOtp
      .affordableCountries(prices, smsOtp.GOOGLE_SERVICE, PHONE_MAX_PRICE, config.SMS_COUNTRY_BLACKLIST)
      .map((c) => Number(c.country))
      .filter((c) => DIAL_CODES[c] !== undefined);
    console.log(`  sms: candidate countries (cheapest first): ${candidates.join(", ") || "none"}`);
  } catch (e) {
    console.log(`  sms: could not read HeroSMS prices: ${e.message}`);
  }
  for (const country of candidates) {
    // Stop before renting another number once Google has cooled the phone step down ("Too many
    // failed attempts. Try again in a few hours."). It is a temporary lockout, not a dead end, so
    // give up now and let the caller mark the account expired for a later retry.
    if (await hasTooManyAttempts(page)) {
      await debug.capture(page, cred.email, "google-phone-rate-limited");
      throw new Expired(`${cred.email}: Google cooled the phone step down (too many failed attempts) at ${safeUrl(page)}`);
    }
    // Recover the number-entry field before renting: a previous rejected number leaves Google on
    // challenge/iap/error (no field), so click "Try Again" first. This both fixes the "could not
    // enter" that stranded every attempt after the first rejection and stops us renting a number we
    // cannot type in.
    if (!(await recoverPhoneNumberField(page))) {
      await debug.capture(page, cred.email, "google-phone-no-field");
      break;
    }
    let rental;
    try {
      rental = await smsOtp.requestNumber({ country, maxPrice: PHONE_MAX_PRICE });
    } catch (e) {
      console.log(`  sms: country ${country} unavailable: ${e.message}`);
      continue;
    }
    const { activationId, phone } = rental;
    console.log(`  sms: rented +${phone} (country ${country}, activation ${activationId})`);
    if (!(await enterPhoneNumber(page, phone, country))) {
      await smsOtp.setStatus(activationId, smsOtp.STATUS_CANCEL);
      console.log(`  sms: could not enter +${phone}; cancelled ${activationId}`);
      continue;
    }
    // Google renders "problem sending" a few seconds after Next, so poll for the outcome rather
    // than checking once: the error appearing means the number is dead (try the next), leaving the
    // phone page means it accepted the number and a code is on the way.
    let rejected = false;
    for (let i = 0; i < 7; i++) {
      await page.waitForTimeout(2000);
      if (await phoneRejected(page)) {
        rejected = true;
        break;
      }
      if (await hasTooManyAttempts(page)) {
        // The cooldown appeared right after this number — refund it and stop; expired, retry later.
        await smsOtp.setStatus(activationId, smsOtp.STATUS_CANCEL);
        await debug.capture(page, cred.email, "google-phone-rate-limited");
        throw new Expired(`${cred.email}: Google cooled the phone step down (too many failed attempts) at ${safeUrl(page)}`);
      }
      if (page.url().includes("challenge/iap/qrcode")) {
        // Google escalated to a scan-a-QR-with-your-phone device check — a dead end. Refund the
        // number and stop trying countries; the main loop re-classifies this page as restricted.
        await smsOtp.setStatus(activationId, smsOtp.STATUS_CANCEL);
        console.log(`  sms: Google escalated +${phone} to a QR-scan device check; cancelled ${activationId}`);
        return;
      }
      if (!page.url().includes("challenge/iap")) break; // advanced past phone entry → accepted
    }
    await debug.capture(page, cred.email, `phone-after-entry-${country}`);
    console.log(`  sms: after +${phone} — rejected=${rejected} url=${page.url()}`);
    if (rejected) {
      await smsOtp.setStatus(activationId, smsOtp.STATUS_CANCEL);
      console.log(`  sms: Google rejected +${phone}; cancelled ${activationId}, trying next country`);
      continue;
    }
    const result = await smsOtp.pollCode(activationId, { timeoutS: 15 });
    if (result.state !== "ok") {
      await smsOtp.setStatus(activationId, smsOtp.STATUS_CANCEL);
      console.log(`  sms: no code for +${phone} (${result.state}); cancelled ${activationId}, trying next`);
      continue;
    }
    console.log(`  sms: code received for +${phone}`);
    const submitted = await enterPhoneCode(page, result.code);
    await smsOtp.setStatus(activationId, submitted ? smsOtp.STATUS_COMPLETE : smsOtp.STATUS_CANCEL);
    if (!submitted) {
      console.log(`  sms: could not enter the code for +${phone}`);
      continue;
    }
    await page.waitForTimeout(4000);
    return; // code submitted; signInGoogle re-checks the logged-in state
  }
  await debug.capture(page, cred.email, "google-phone-exhausted");
  throw new NeedsHuman(`${cred.email}: phone verification failed for every affordable country`);
}

// HeroSMS country id → international dial code. Used to split the dial code off the number so it
// goes to Google's country picker, not the national field. This map is also the eligibility list for
// the phone step: passPhoneChallenge only rents a country whose dial code is here, so a number can
// always be entered correctly. Add a country here — and its name to COUNTRY_NAMES — to let the phone
// step use it.
export const DIAL_CODES = { 36: "1", 16: "44", 73: "55", 7: "60", 2: "7", 41: "237", 4: "63", 31: "27", 8: "254" };

// HeroSMS country id → the country's name as Google's picker lists it in English (the browser is
// launched with locale en-US so these labels are stable). enterPhoneNumber selects this row in the
// country dropdown the way a person does; the dial code alone cannot identify the row, since id 36
// (Canada, +1) and id 2 (Kazakhstan, +7) share their code with other countries. Every DIAL_CODES id
// must have an entry here.
export const COUNTRY_NAMES = {
  36: "Canada",
  16: "United Kingdom",
  73: "Brazil",
  7: "Malaysia",
  2: "Kazakhstan",
  41: "Cameroon",
  4: "Philippines",
  31: "South Africa",
  8: "Kenya",
};

// The national part of an E.164 number: HeroSMS returns `<dial><national>` digits; Google's field
// wants only the national digits (the dial code goes to the picker). Unknown country → unchanged.
export function nationalNumber(phone, country) {
  const digits = String(phone).replace(/\D/g, "");
  const dial = DIAL_CODES[country];
  return dial && digits.startsWith(dial) ? digits.slice(dial.length) : digits;
}

// Whether Google is showing the phone-NUMBER entry field (`#phoneNumberId`), specifically. NOT a
// bare `input[type=tel]`: the SMS-code page's field (`idvPin`) is also `type=tel`, so matching it
// would type a phone number into the code box; and the error page (challenge/iap/error) has neither,
// so this is correctly false there.
async function phoneFieldVisible(page) {
  return page.locator("#phoneNumberId").first().isVisible({ timeout: 8000 }).catch(() => false);
}

// After a rejected or failed number Google parks on challenge/iap/error ("We couldn't verify your
// info"), whose only controls are "Try Again" (→ back to the number field) and "Try another way".
// Click "Try Again" to restore the number-entry field before the next attempt, so one bad number
// does not strand every later one at "could not enter" — and we never rent a number we cannot type in.
async function recoverPhoneNumberField(page) {
  if (await phoneFieldVisible(page)) return true;
  await clickRole(page, ["Try again"]);
  await page.waitForTimeout(2000);
  return phoneFieldVisible(page);
}

// Enter the rented number the way a person does: open the country dropdown, pick the country, then
// type ONLY the national digits in the field. The dial code must live in the picker, not the field —
// typing the full +<code><number> into the field while the picker also holds a country makes Google
// build a malformed number and answer "There was a problem sending you a verification code" (the
// picker showed a US flag for a +1 Canada number, and the field still held the +1). If the picker
// cannot be driven (its open-trigger selector drifted), fall back to typing the +code into the field
// to set the picker and then replacing it with the national part alone.
async function enterPhoneNumber(page, phone, country) {
  const field = page.locator("#phoneNumberId").first();
  if (!(await field.isVisible({ timeout: 8000 }).catch(() => false))) return false;
  const national = nationalNumber(phone, country);
  const name = COUNTRY_NAMES[country];
  if (name && (await selectCountryInPicker(page, name))) {
    await field.click();
    await page.keyboard.press("Meta+a");
    await page.keyboard.type(national, { delay: 50 }); // picker already holds the country → national digits only
  } else {
    console.log("  phone: country picker not driven, entering +code to set the country instead");
    const digits = phone.replace(/\D/g, "");
    await field.click();
    await page.keyboard.press("Meta+a");
    await page.keyboard.type(`+${digits}`, { delay: 50 }); // the picker reads the +code and sets the country
    await page.waitForTimeout(700);
    await page.keyboard.press("Meta+a"); // select the whole value…
    await page.keyboard.type(national, { delay: 50 }); // …and replace it with the national number only
  }
  await page.waitForTimeout(600);
  await clickNext(page);
  return true;
}

// Open Google's country-code dropdown and choose the row labelled `name`, returning whether it was
// selected. The widget is a `role=combobox` (the country one shows a `+<dial>`, distinguishing it
// from the language combobox) that opens a `role=listbox` of country `option`s; locale is en-US so
// `name` matches the row's English label, and getByRole mirrors the robust selection used in
// clickNext. Any failure here is non-fatal — enterPhoneNumber falls back to the +code method — and an
// unmatched option logs the listbox's country names so the label can be corrected.
async function selectCountryInPicker(page, name) {
  let opened = false;
  let selected = false;
  try {
    const trigger = page
      .getByRole("combobox")
      .filter({ hasText: /\+\d/ })
      .first();
    if ((await trigger.count()) === 0 || !(await trigger.isVisible().catch(() => false))) return false;
    await trigger.click({ timeout: 3000 });
    opened = true;
    await page.waitForTimeout(400);
    const option = page.getByRole("option", { name }).first();
    if ((await option.count()) === 0 || !(await option.isVisible().catch(() => false))) {
      const options = await page
        .evaluate(() => [...document.querySelectorAll("ul[role=listbox] li")].slice(0, 20).map((li) => li.textContent.trim()))
        .catch(() => []);
      console.log(`  phone: country option "${name}" not found; options: ${JSON.stringify(options)}`);
      return false;
    }
    await option.click({ timeout: 3000 });
    await page.waitForTimeout(400);
    selected = true;
    console.log(`  phone: picked ${name} in the country dropdown`);
    return true;
  } catch {
    return false;
  } finally {
    // Never leave the dropdown open over the field: the fallback path clicks #phoneNumberId, which an
    // open listbox overlay would block. Close it whenever we opened it but did not select.
    if (opened && !selected) {
      await page.keyboard.press("Escape").catch(() => {});
      await page.waitForTimeout(300);
    }
  }
}

// True when Google will not send a code to the number — it refuses the number outright (VoIP /
// over-used / unverifiable) or reports a send failure ("There was a problem sending you a
// verification code"). Either way the number is dead; move on rather than poll for a code that
// will never come.
async function phoneRejected(page) {
  try {
    const loc = page
      .getByText(
        /can.?t be used|couldn.?t verify|different phone number|too many|not a valid|wrong number|problem sending|couldn.?t send|unable to send/i,
      )
      .first();
    return (await loc.count()) > 0 && (await loc.isVisible());
  } catch {
    return false;
  }
}

// Enter the SMS code Google sent to the rented number. The code page's DOM is captured (logged) so
// the selector can be tuned if it misses; common code-field selectors are tried.
async function enterPhoneCode(page, code) {
  console.log(`  sms: code-page inputs: ${JSON.stringify(await inputInventory(page))}`);
  const field = page
    .locator("input[name=code]:visible, #idvPin:visible, #code:visible, input[type=tel]:visible, input[type=text]:visible")
    .first();
  if (!(await field.isVisible({ timeout: 8000 }).catch(() => false))) return false;
  await field.click();
  await field.fill(code);
  await page.waitForTimeout(500);
  await clickNext(page);
  return true;
}

// Fill the first VISIBLE matching field if it is empty; return the field, or null when nothing was filled. Uses
// waitForSelector(state: visible) rather than .first() — Google renders a hidden decoy input
// before the real one, and .first() grabs the decoy.
async function tryFill(surface, selector, value, timeout = 2500) {
  try {
    const el = await surface.waitForSelector(selector, { state: "visible", timeout });
    if (!el || (await el.inputValue())) return null;
    await el.fill(value);
    return el;
  } catch {
    return null;
  }
}

// True if the verify-it's-you chooser (challenge/selection) offers a recovery-email path — a "Get a
// verification code at <email>" or "Confirm your recovery email" option. Its presence means a human
// (or a later recovery-email flow) could still get in, so the account is escalated rather than a dead
// end. Read from the page, so no recovery address needs to be stored.
async function hasRecoveryEmailOption(surface) {
  try {
    const loc = surface.getByText("recovery email", { exact: false }).first();
    return (await loc.count()) > 0 && (await loc.isVisible());
  } catch {
    return false;
  }
}

// True if Google is showing the phone-step cooldown banner ("Too many failed attempts. Unavailable
// because of too many failed attempts. Try again in a few hours."). A temporary lockout, not a dead
// end — the caller marks the account expired and retries later.
async function hasTooManyAttempts(surface) {
  try {
    const loc = surface.getByText("Too many failed attempts", { exact: false }).first();
    return (await loc.count()) > 0 && (await loc.isVisible());
  } catch {
    return false;
  }
}

// True if Google is showing its OAuth 'Something went wrong' error dialog. Matched narrowly on
// that phrase only: 'Try again' also appears in ordinary field errors like 'Wrong code. Try
// again.', which must not be treated as this modal.
async function hasErrorModal(surface) {
  try {
    const loc = surface.getByText("Something went wrong", { exact: false }).first();
    return (await loc.count()) > 0 && (await loc.isVisible());
  } catch {
    return false;
  }
}

// The kind of rejected-credential message Google is showing, if any. 'Wrong code' means the TOTP
// was rejected (a stale 2FA secret cannot be scripted around); 'Wrong password' means the
// password was rejected.
async function hasWrongCredential(surface) {
  for (const [probe, kind] of [
    ["Wrong code", "totp"],
    ["Wrong password", "password"],
  ]) {
    try {
      const loc = surface.getByText(probe, { exact: false }).first();
      if ((await loc.count()) > 0 && (await loc.isVisible())) return kind;
    } catch {
      continue;
    }
  }
  return null;
}

// Fill and submit Google's password page, even when Firefox has autofilled the field. `tryFill`
// skips a field that already holds a value, so a profile-saved password would never be re-entered
// and the page would never advance. Here we (re)fill only when the value differs from the
// credential, then always click Next. Returns whether a visible password field was found (i.e. we
// were on the password step).
// Submit a filled Google field with Enter, the one submit that carries no label: Google's Next
// button has no stable id and its text follows the page language. When the page ignores Enter (the
// field is still on screen a moment later) fall back to the labelled button.
async function submitField(surface, el) {
  try {
    await el.press("Enter");
    await surface.waitForTimeout(800);
    if (!(await el.isVisible())) return;
  } catch {
    /* the field left the page with the navigation, or has no keyboard: try the button */
  }
  await clickNext(surface);
}

export async function submitPassword(surface, password) {
  let el;
  try {
    el = await surface.waitForSelector("input[type=password]", { state: "visible", timeout: 2500 });
  } catch {
    return false;
  }
  if (!el) return false;
  try {
    if ((await el.inputValue()) !== password) await el.fill(password);
  } catch {
    return false;
  }
  await submitField(surface, el);
  return true;
}

// Fill and submit Google's identifier (email) page, even when the field is already prefilled.
// `tryFill` skips a non-empty field, so a prefilled email would never click Next and the loop would
// stall until it escalated. Here we (re)fill only when the value differs, then always click Next.
// Returns whether a visible identifier field was found (i.e. we were on the identifier step).
export async function submitIdentifier(surface, email) {
  let el;
  try {
    el = await surface.waitForSelector(IDENTIFIER_SELECTOR, { state: "visible", timeout: 2500 });
  } catch {
    return false;
  }
  if (!el) return false;
  try {
    if ((await el.inputValue()) !== email) await el.fill(email);
  } catch {
    return false;
  }
  await submitField(surface, el);
  return true;
}

// On a non-TOTP 2-Step page, switch to the authenticator method via 'Try another way'. Google may
// default an account to its 'Security code' (Android-device) method, whose page has no `totpPin`
// field. 'Try another way' lists the account's other methods; pick the authenticator so the
// loop's TOTP branch can enter the computed code next iteration. Returns true only when the
// authenticator option was clicked.
async function switchToAuthenticator(surface) {
  const AUTHENTICATOR_LABELS = [
    "Get a verification code from the Google Authenticator",
    "Google Authenticator",
    "Authenticator app",
    "Authenticator",
  ];
  try {
    if (await surface.$("input[name=totpPin]")) return false; // already on the authenticator page
    // The "verify it's you" chooser lists the methods directly; a specific challenge page (skotp,
    // phone) hides them behind "Try another way". Take the direct option first, else open the chooser.
    if (await clickFirst(surface, AUTHENTICATOR_LABELS)) return true;
    if (!(await clickFirst(surface, ["Try another way"]))) return false;
    await surface.waitForTimeout(1500);
    return clickFirst(surface, AUTHENTICATOR_LABELS);
  } catch {
    return false;
  }
}

async function clickNext(surface) {
  for (const name of ["Next", "Continue", "Weiter"]) {
    try {
      const loc = surface.getByRole("button", { name }).first();
      if ((await loc.count()) > 0 && (await loc.isVisible())) {
        await loc.click({ timeout: 3000 });
        return true;
      }
    } catch {
      continue;
    }
  }
  return false;
}

// Force a Google page into English with the `hl` query param (what Google's own language dropdown
// does), so button matching does not depend on the account's UI language. The vendor's Gmail
// accounts default to non-English (e.g. Indonesian), which renders the OAuth consent's "Continue" as
// "Lanjutkan" and stalls the flow. Returns the rewritten URL, or null when no change is needed (the
// page is already English, is not a Google page, or the string is not a URL).
export function forceEnglishUrl(url) {
  try {
    const u = new URL(url);
    if (!u.hostname.endsWith(".google.com")) return null;
    if (u.searchParams.get("hl") === "en") return null;
    u.searchParams.set("hl", "en");
    return u.toString();
  } catch {
    return null;
  }
}

// Reload a Google page in English when it renders in another language (its `<html lang>`), so its
// buttons carry their English names. Google drops `hl` on some hops (the account tile leads to a
// "Sign in to <app>" page without it), and that page then renders in the account's own language.
// Returns whether it reloaded. Best-effort: a failed read or navigation leaves the page as-is.
export async function ensureEnglish(surface) {
  try {
    const lang = await surface.evaluate(() => document.documentElement.lang);
    if (!lang || lang.toLowerCase().startsWith("en")) return false;
    const english = forceEnglishUrl(surface.url());
    if (!english) return false;
    await surface.goto(english, { waitUntil: "domcontentloaded" });
    return true;
  } catch {
    return false;
  }
}

// Whether `url` is on the app's registrable domain or one of its sub-domains.
function onAppDomain(url, domain) {
  try {
    const host = new URL(url).hostname;
    return host === domain || host.endsWith(`.${domain}`);
  } catch {
    return false;
  }
}

// The accounts.google.com page driving the app's OAuth — a popup or `page` itself — or null when the
// app's session lands with no Google page caught (a pre-consented OAuth can pass through Google
// between two polls). The app may hop through its auth provider first (app → auth.example.com →
// Google) and render a "Loading" overlay, so the Google page can take 20 s+ to appear. When neither
// shows up the app never opened the Google sign-in (the app's edge drops the popup now and then);
// that throws at once instead of being waited out as a missing session token. The session is read
// only while `page` is on the app's own domain, because the check reads storage on whatever origin
// the page is on.
export async function oauthSurface(page, adapter, email) {
  const seen = new Set();
  for (let i = 0; i < 30; i++) {
    for (const candidate of page.context().pages()) {
      try {
        const u = candidate.url();
        seen.add(u);
        if (u.includes("accounts.google.com")) return candidate;
      } catch {
        continue;
      }
    }
    if (onAppDomain(page.url(), adapter.domain) && (await adapter.ready(page))) return null;
    await page.waitForTimeout(1000);
  }
  console.log(`  oauthSurface: no Google page after 30s; pages seen: ${JSON.stringify([...seen])}`);
  await debug.capture(page, email, `${adapter.name}-oauth-not-opened`);
  throw new Error(`${adapter.name}: Google sign-in never opened`);
}

const ASSIST_WAIT_MS = 300000; // how long an assist run pauses for the person to clear a challenge
const ASSIST_POLL_MS = 100; // how often an assist run checks whether the person has finished

// Stop automating and let the person clear a Google step in the window by hand — a reCAPTCHA on the
// sign-in path, or an unscriptable challenge during app OAuth. Polls `done(surface)` until it is
// true (or the surface closes) and returns true, or returns false if the window passed. `done`
// defaults to "the OAuth has left accounts.google.com" (the app took over); the sign-in path passes
// a "the password field appeared" predicate instead. `message` is the ASSIST NEEDED line.
export async function waitForHuman(surface, cred, { message, timeoutMs = ASSIST_WAIT_MS, done } = {}) {
  const finished = done ?? ((s) => !s.url().includes("accounts.google.com"));
  console.log(`  >>> ASSIST NEEDED: ${message ?? `finish the Google step for ${cred.email} in the browser now`}`);
  let waited = 0;
  while (waited < timeoutMs) {
    try {
      if (surface.isClosed() || (await finished(surface))) {
        console.log(`  >>> ASSIST: ${cred.email} continuing`);
        return true;
      }
    } catch {
      return true;
    }
    try {
      await surface.waitForTimeout(ASSIST_POLL_MS);
    } catch {
      return true;
    }
    waited += ASSIST_POLL_MS;
  }
  return false;
}

// Drive the app's "Sign in with Google" prompt to completion. The app has already sent the
// browser to Google's OAuth page (same tab or a popup). Google may show an account tile, a fresh
// identifier + password + TOTP, and a consent screen; answer whichever appears until it redirects
// back to the app. Returns at once if the app's session lands without a Google prompt. On a headed
// run (`assist`) a challenge the script cannot pass hands control to the person at the window instead
// of failing, and resumes once they clear it.
export async function completeGoogleOauth(page, cred, adapter, { timeoutMs = 60000, assist = false, proxyUrl } = {}) {
  const surface = await oauthSurface(page, adapter, cred.email);
  if (!surface) return; // the app's session landed with no Google prompt to answer
  // Same traversal as the Google login: consent and the account-chooser tile are handled as nodes,
  // and a cold OAuth session that drops into a full identifier→password→TOTP→… login is covered by
  // the shared core for free. Done when the surface leaves Google (redirected back to the app or the
  // popup closed); the app's ready-check then decides whether a session actually landed.
  await driveGoogleChallenges(surface, cred, {
    assist,
    timeoutMs,
    proxyUrl,
    done: (s) => !s.url().includes("accounts.google.com"),
  });
}

function safeUrl(surface) {
  try {
    return surface.url();
  } catch {
    return "?";
  }
}

async function clickFirst(surface, texts) {
  for (const text of texts) {
    try {
      const loc = surface.getByText(text, { exact: false }).first();
      if ((await loc.count()) > 0 && (await loc.isVisible())) {
        await loc.click({ timeout: 3000 });
        return true;
      }
    } catch {
      continue;
    }
  }
  return false;
}

async function clickRole(surface, names) {
  for (const name of names) {
    try {
      const loc = surface.getByRole("button", { name }).first();
      if ((await loc.count()) > 0 && (await loc.isVisible())) {
        await loc.click({ timeout: 3000 });
        return true;
      }
    } catch {
      continue;
    }
  }
  return false;
}

// Open `url` and wait out Cloudflare's "Performing security verification" interstitial, which a
// fresh profile hits on the app's first load and which clears by itself within a few seconds; any
// interaction before it clears finds none of the app's buttons.
export async function gotoPastCloudflare(page, url, { assist = false, timeoutMs = 45000 } = {}) {
  await gotoWithRetry(page, url);
  let deadline = Date.now() + timeoutMs;
  let asked = false;
  while (Date.now() < deadline) {
    let text = null;
    try {
      text = await page.innerText("body");
    } catch {
      /* mid-navigation: try again */
    }
    // An empty body is the interstitial before it renders, not the app.
    if (text && !text.includes("Performing security verification")) return;
    if (assist && !asked && Date.now() > deadline - 15000) {
      // The check is a human step: hand it to the person at the window and wait for them.
      console.log(`  >>> ASSIST NEEDED: pass the Cloudflare check on ${url} in the browser now`);
      asked = true;
      deadline = Date.now() + ASSIST_WAIT_MS;
    }
    await page.waitForTimeout(1000);
  }
  console.log(`  Cloudflare verification on ${url} did not clear`);
}

// Poll until the app's session token has landed, or the timeout elapses. The OAuth handshake
// finishes asynchronously after the account chooser, so the token appears a few seconds later;
// capturing before it does stores a useless mid-handshake state.
export async function waitReady(page, adapter, timeoutMs = 40000) {
  let waited = 0;
  const step = 1000;
  while (waited < timeoutMs) {
    if (await adapter.ready(page)) return true;
    await page.waitForTimeout(step);
    waited += step;
  }
  return adapter.ready(page);
}

// Whether any of the adapter's logged-out entry labels is on the page right now. isVisible resolves
// at once (it does not wait), so this is a cheap point-in-time check the poll below repeats.
async function loginEntryVisible(page, texts = []) {
  for (const text of texts) {
    try {
      if (await page.getByText(text, { exact: false }).first().isVisible()) return true;
    } catch {
      /* not on the page yet */
    }
  }
  return false;
}

// Decide whether the profile is already signed into the app, so the sign-in can be skipped. Resolve
// on whichever appears first — the signed-in signal or the logged-out login entry — instead of
// waiting out a token poll a logged-out profile can never satisfy. The signed-in signal is the
// adapter's signed-in URL marker when it has one (the app redirects the root to /token), else its
// session token.
export async function appAlreadySignedIn(page, adapter, timeoutMs = 12000) {
  const signedIn = async () => (adapter.signedInUrl ? adapter.signedInUrl(page.url()) : adapter.ready(page));
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await signedIn()) return true;
    if (await loginEntryVisible(page, adapter.entryTexts)) return false;
    await page.waitForTimeout(300);
  }
  return signedIn();
}

export async function exportScoped(db, page, adapter, email) {
  const state = await page.context().storageState();
  const scoped = filterState(state, adapter.domain);
  store.saveSession(db, adapter.name, email, scoped.cookies, scoped.local_storage);
}

// Whether an app-login attempt that threw is worth retrying on a fresh page load. the app's Cloudflare
// edge intermittently serves a 404/blank landing (its login entry never renders) or fails to open the
// Google OAuth popup (the session token never lands); both surface as a plain Error and clear on a
// reload. A ban (Restricted), a Google-side stop that needs a person (NeedsHuman), and a lapsed
// session (Expired) are terminal, not flakes, so they propagate at once.
export function isTransientAppError(error) {
  return !(error instanceof Restricted || error instanceof NeedsHuman || error instanceof Expired);
}

// Run one app-login attempt, retrying a transient failure up to `attempts` times. Each retry re-runs
// the whole attempt against the same profile (a fresh page load), which is what clears the app's flaky
// edge; a non-transient error stops immediately, and the last error is thrown once the cap is hit.
export async function withAppRetries(attempts, attempt, { onRetry } = {}) {
  let lastErr;
  for (let i = 1; i <= attempts; i++) {
    try {
      return await attempt(i);
    } catch (e) {
      lastErr = e;
      if (!isTransientAppError(e) || i >= attempts) throw e;
      onRetry?.(i, e);
    }
  }
  throw lastErr;
}

// One pass at signing into the app and storing its scoped session. Throws on any step that did not
// complete; exportApp decides whether the failure is worth another pass.
async function attemptAppLogin(page, db, adapter, cred, { assist, proxyUrl }) {
  await gotoPastCloudflare(page, adapter.startUrl, { assist });
  // Skip the whole login when the profile is already signed in.
  if (!(await appAlreadySignedIn(page, adapter))) {
    await adapter.signIn(page, cred.email);
    await completeGoogleOauth(page, cred, adapter, { assist, proxyUrl });
    if (!(await waitReady(page, adapter))) {
      // OAuth returned to the app but the session token never landed. This is an app-side
      // outcome, distinct from a Google-side stop, which completeGoogleOauth throws as
      // NeedsHuman before we get here.
      await debug.capture(page, cred.email, `${adapter.name}-token-missing`);
      throw new Error(`${adapter.name}: session token never appeared; login did not complete`);
    }
  }
  await exportScoped(db, page, adapter, cred.email);
}

// Sign into one app with Google and store its scoped session, once it is complete. Retries an app
// whose adapter asks for it (an app with a transient edge) on a fresh page load.
export async function exportApp(page, db, adapter, cred, { assist = false, proxyUrl } = {}) {
  await withAppRetries(adapter.attempts ?? 1, () => attemptAppLogin(page, db, adapter, cred, { assist, proxyUrl }), {
    onRetry: (i, e) => console.log(`  ${adapter.name}: attempt ${i} failed (${e.message}); reloading and retrying`),
  });
}

// Open one account's persistent Camoufox profile and run `fn(context, page)` inside it, closing
// the browser afterwards. Shared by the login flow and any read-only probe (e.g.
// check-restricted); the profile carries the account's Google + app sessions, so a probe reuses
// them without signing in again. The window opens on the CAMOUFOX_DISPLAY display when headed.
// `rotate` uses a rotating proxy exit instead of the account's sticky one. Refuses to launch
// without the residential proxy: every login from the bare home IP gets that IP flagged.
// Whether a failed headed run should keep the window open for a person. A headless run never can. A
// Restricted dead end needs no one and closes at once; an Expired account also closes unless it asks
// to stay open (holdOpen) so its page can be read by hand. Every other error (a NeedsHuman challenge
// a person could finish, or an unexpected fault worth inspecting) keeps the window open.
export function shouldHoldOpenForDebug(headed, error) {
  if (!headed) return false;
  if (error instanceof Restricted) return false;
  if (error instanceof Expired) return error.holdOpen === true;
  return true;
}

// One stable device fingerprint per profile. Camoufox regenerates a fresh fingerprint every launch
// when none is passed, so the same account would present a different device (CPU cores, screen size,
// canvas) on each login — an anomaly for the account. Minting one with Camoufox's own generator on
// first use and reusing it keeps each account one consistent device across logins. `generate` is
// injected for testing; it defaults to camoufox's generator. The stored value is a genuine
// Camoufox-generated Firefox fingerprint, not a hand-built one.
export function loadOrCreateFingerprint(path, generate = generateFingerprint) {
  if (existsSync(path)) return JSON.parse(readFileSync(path, "utf8"));
  const fp = generate();
  writeFileSync(path, JSON.stringify(fp));
  return fp;
}

// Run `fn` in the persistent Camoufox profile of `key`, through the account's sticky residential
// exit, or through `proxyUrl` when the caller pins the exit itself. `blockAssets: false` leaves
// images, media and fonts on, for a site whose challenge is an image.
export async function withProfile(key, { headed = false, rotate = false, proxyUrl = config.proxyFor(key, { rotate }), blockAssets = true }, fn) {
  const { Camoufox } = await import("camoufox-js");
  const profile = config.profileDirFor(key);
  mkdirSync(profile, { recursive: true });
  if (!proxyUrl) {
    throw new Error("No proxy set. Add RESIDENTIAL_PROXY_URL to ~/.config/secrets-manager/.env; never log in from the home IP.");
  }
  if (headed) console.log(`  window opens on ${windowPlace.displayName() || "the main display"}: ${windowPlace.openOnDisplay(profile)}`);
  const context = await Camoufox({
    headless: !headed,
    geoip: true, // timezone/locale/geolocation still follow the account's sticky proxy IP
    locale: "en-US", // render every app in English so the login selectors are stable
    proxy: toProxyDict(proxyUrl),
    user_data_dir: profile,
    fingerprint: loadOrCreateFingerprint(join(profile, "fingerprint.json")),
    i_know_what_im_doing: true, // the fingerprint is Camoufox's own, persisted per profile on purpose
  });
  let page;
  let recording = null;
  try {
    // The blocklist keeps the apps' data feeds and assets off the proxy. It lets Google's own
    // sign-in assets (incl. the reCAPTCHA image challenge) through, so it stays installed even under
    // a headed reCAPTCHA is still solvable by hand, and the app's page still finishes loading.
    if (blockAssets) await installBlocklist(context);
    page = context.pages()[0] ?? (await context.newPage());
    if (headed) {
      windowPlace.movePopupsToDisplay(context, profile);
      // A headed run is a debug run: record the screen, and save each reCAPTCHA grid image with the
      // classifier's answer, so the grids can be reviewed and the classifier's accuracy measured after.
      process.env.SM_DEBUG_GRIDS = "1";
      recording = debug.startScreenRecording(key, windowPlace.findPid(profile));
      if (recording) console.log(`  recording headed session to ${recording.path}`);
    }
    // Hand the account's resolved proxy URL to the flow so the CAPTCHA solver can mint tokens behind
    // the SAME sticky exit the browser submits from.
    return await fn(context, page, proxyUrl);
  } catch (e) {
    if (shouldHoldOpenForDebug(headed, e)) await holdOpenForDebug(context, page, e);
    throw e;
  } finally {
    if (recording) {
      const saved = await debug.stopScreenRecording(recording);
      if (saved) console.log(`  saved recording: ${saved}`);
    }
    await context.close().catch(() => {});
  }
}

const HOLD_OPEN_MS = 300000; // 5-min ceiling so a forgotten headed window doesn't pin its worker

// A headed run keeps its browser open when `fn` throws, so a failed login can be inspected and
// finished by hand instead of the window slamming shut. Logs the error and the page URL, then waits
// until the person closes the window (the exit signal) or the ceiling passes. Never swallows the
// error — the caller still marks the account escalated.
async function holdOpenForDebug(context, page, err) {
  let where = "";
  try {
    where = ` (at ${page.url()})`;
  } catch {
    /* the page may already be gone */
  }
  console.log(`  error: ${err?.message ?? err}${where}`);
  console.log("  >>> ASSIST NEEDED: browser left open for debugging — close the window to continue");
  const ceiling = Date.now() + HOLD_OPEN_MS;
  while (Date.now() < ceiling) {
    let open;
    try {
      open = context.pages().length > 0;
    } catch {
      open = false;
    }
    if (!open) break;
    await new Promise((r) => setTimeout(r, 2000));
  }
}

// Gmail plus-alias; tag is supplied by the adapter.
export function aliasFor(baseEmail, tag) {
  if (typeof tag !== "string" || !tag) throw new Error("alias tag is required");
  const [local, domain] = baseEmail.split("@");
  return `${local.split("+", 1)[0]}+${tag}@${domain}`;
}

// Opened with `hl=en`: myaccount and the re-auth challenge it bounces through otherwise render in the
// account's own language, and every label matched on those pages is English.
export const APPPASSWORDS_URL = "https://myaccount.google.com/apppasswords?hl=en";
// The generated code is four groups of four lowercase letters (`abcd efgh ijkl mnop`).
const APP_PW_RE = /\b[a-z]{4} [a-z]{4} [a-z]{4} [a-z]{4}\b/;

// The 16-char app password from the 'Generated app password' dialog body. Prefers the code that
// follows the 'for your device' label, so a stray four-word run elsewhere on the page cannot be
// mistaken for it; falls back to the first four-group match. Returns null when the dialog has no
// such code.
export function extractAppPassword(dialogText) {
  if (!dialogText) return null;
  const anchor = dialogText.indexOf("for your device");
  if (anchor !== -1) {
    const after = dialogText.slice(anchor).match(APP_PW_RE);
    if (after) return after[0];
  }
  const match = dialogText.match(APP_PW_RE);
  return match ? match[0] : null;
}

// The base32 authenticator secret from the "Set up authenticator" dialog's text-key view, upper-
// cased and stripped of spaces. Google prints it as eight space-separated 4-char base32 groups
// after "spaces don't matter):" and before "Make sure … Time based." Prefer the run anchored to
// that label; the whitespace between groups is what stops a concatenated nav word from matching.
// Falls back to the longest base32 run on the page, capped at 32 chars. Null when none is found.
export function parseSetupKey(text) {
  if (!text) return null;
  const anchored = text.match(/matter\)\s*:?\s*((?:[a-z2-7]{4}\s+){5,7}[a-z2-7]{2,4})/i);
  if (anchored) return anchored[1].replace(/\s+/g, "").toUpperCase();
  const runs = [...text.matchAll(/([a-z2-7]{4}\s+){3,}[a-z2-7]{4}/gi)].map((m) => m[0].replace(/\s+/g, ""));
  const longest = runs.filter((c) => /^[a-z2-7]{16,64}$/i.test(c)).sort((a, b) => b.length - a.length)[0];
  return longest ? longest.slice(0, 32).toUpperCase() : null;
}

// True when `url` is the sensitive settings page whose path contains `target`, and not the
// re-auth challenge that guards it. A bare substring check misfires: `signinoptions` contains
// `signin`, and the challenge URL carries the return path (e.g. `signinoptions/twosv`) in its
// `continue=` query — so match on the host + path only, the way `isGoogleLoggedIn` does.
export function onSettingsPage(url, target) {
  try {
    const u = new URL(url);
    return u.hostname === "myaccount.google.com" && u.pathname.includes(target);
  } catch {
    return false;
  }
}

// Clear Google's 'verify it's you' re-prompt guarding a sensitive settings page. App-password
// creation and 2-Step settings force a fresh credential check even on a live session; on a fresh
// exit IP Google escalates it to a full sign-in that starts at the email step. Handles whichever
// page appears — identifier, password or TOTP — reusing the sign-in email, password and computed
// TOTP, until the URL is back on the settings page named by `target` (a path fragment such as
// `apppasswords` or `twosv`). Returns whether the re-auth cleared.
async function reauthSensitive(page, cred, target = "apppasswords", { assist = false, timeoutMs = 120000, proxyUrl } = {}) {
  // Same graph traversal as sign-in — identifier / password / TOTP, and a reCAPTCHA or other challenge
  // Google throws on a fresh exit IP (CapSolver clears the reCAPTCHA, or a headed run's person does) —
  // done once the URL is back on the settings page named by `target`. A dead-end (wrong password, no
  // TOTP, an unpassable challenge) raises NeedsHuman; the caller only wants a boolean, so translate it
  // to "did we land on the page".
  try {
    await driveGoogleChallenges(page, cred, {
      assist,
      timeoutMs,
      proxyUrl,
      done: (s) => onSettingsPage(s.url(), target),
    });
  } catch (e) {
    if (!(e instanceof NeedsHuman)) throw e;
  }
  return onSettingsPage(page.url(), target);
}

// Create a fresh Gmail app password in the account's own profile and return it. Opens the base
// profile (which already holds the Google session), clears the sensitive-page re-auth, types a
// name, clicks Create and reads the 16-char code from the result dialog. Used when the stored
// app_password is missing or revoked so the IMAP OTP read for email signup can work again.
// Returns the `xxxx xxxx xxxx xxxx` code, or null on any step that did not complete. Never logged.
export async function mintAppPassword(cred, { name, headed = false, rotate = false } = {}) {
  if (typeof name !== "string" || !name) throw new Error("app password name is required");
  return withProfile(cred.email, { headed, rotate }, async (_context, page, proxyUrl) => {
    await gotoWithRetry(page, APPPASSWORDS_URL);
    if (!(await reauthSensitive(page, cred, "apppasswords", { assist: headed, proxyUrl }))) {
      await debug.capture(page, cred.email, "apppw-reauth-failed");
      return null;
    }
    await page.waitForTimeout(1500);
    await ensureEnglish(page); // the re-auth's followup hop can drop `hl`
    const handle = await page.evaluateHandle(
      () => [...document.querySelectorAll("input[type=text]")].find((e) => e.offsetParent !== null) || null,
    );
    const field = handle.asElement();
    if (!field) {
      await debug.capture(page, cred.email, "apppw-name-field-missing");
      return null;
    }
    await field.click();
    await field.fill(name);
    await page.waitForTimeout(500);
    let clicked = false;
    for (let i = 0; i < 3; i++) {
      try {
        await page.getByRole("button", { name: "Create" }).first().click({ timeout: 8000 });
        clicked = true;
        break;
      } catch {
        await page.waitForTimeout(1500);
      }
    }
    if (!clicked) {
      await debug.capture(page, cred.email, "apppw-create-missing");
      return null;
    }
    await page.waitForTimeout(4000);
    const code = extractAppPassword(await page.evaluate(() => document.body.innerText));
    if (!code) await debug.capture(page, cred.email, "apppw-code-missing");
    return code;
  });
}

export const TWOSV_URL = "https://myaccount.google.com/signinoptions/twosv?hl=en";

// Add a Google Authenticator app to an already-signed-in account and return its base32 secret.
// Opens the account's profile, clears the sensitive-settings re-auth, reveals the text setup key,
// then confirms it with a computed code. Adding an authenticator on a 2-Step-off account does not
// itself turn 2-Step on — turnOnTwoStep does that. Returns the secret, or null when a step did not
// complete (a debug capture is left). Throws NotLoggedIn when the profile has no session and
// NeedsHuman on a Google challenge. Not unit-tested; verify with --headed on the first account.
export async function enrollAuthenticator(cred, { headed = false, rotate = false } = {}) {
  return withProfile(cred.email, { headed, rotate }, async (_context, page, proxyUrl) => {
    if (!(await isGoogleLoggedIn(page))) throw new NotLoggedIn(`${cred.email}: no Google session; run \`login google\` first`);
    await gotoWithRetry(page, TWOSV_URL);
    if (!(await reauthSensitive(page, cred, "twosv", { assist: headed, proxyUrl }))) {
      await debug.capture(page, cred.email, "twofactor-reauth-failed");
      return null;
    }
    await ensureEnglish(page); // the re-auth's followup hop can drop `hl`
    const addAuth = page.getByText(/Add authenticator app/i).first();
    if (!(await addAuth.isVisible({ timeout: 8000 }).catch(() => false))) {
      await debug.capture(page, cred.email, "twofactor-no-add-authenticator");
      return null;
    }
    await addAuth.click();
    await page.waitForTimeout(3500);
    const setUp = page.getByRole("button", { name: /Set up authenticator/i }).first();
    if (await setUp.isVisible({ timeout: 6000 }).catch(() => false)) {
      await setUp.click();
      await page.waitForTimeout(4000);
    }
    // Reveal the text key rather than the QR ("Can't scan it?").
    const cantScan = page.getByText(/can.?t scan/i).first();
    if (await cantScan.isVisible({ timeout: 6000 }).catch(() => false)) {
      await cantScan.click();
      await page.waitForTimeout(2500);
    }
    const secret = parseSetupKey(await page.evaluate(() => document.body.innerText));
    if (!secret) {
      await debug.capture(page, cred.email, "twofactor-no-setup-key");
      return null;
    }
    // Advance to the code-entry step and confirm with a fresh TOTP computed from the scraped key.
    await clickNext(page);
    await page.waitForTimeout(3500);
    const codeField = page.locator("input[type=tel]:visible, input[type=text]:visible, input[name=code]:visible").first();
    if (!(await codeField.isVisible({ timeout: 6000 }).catch(() => false))) {
      await debug.capture(page, cred.email, "twofactor-no-code-field");
      return null;
    }
    await codeField.click();
    await codeField.fill(computeTotp(secret));
    await page.waitForTimeout(600);
    await page.getByRole("button", { name: /Verify|Next|Done|Save/i }).first().click({ timeout: 8000 }).catch(() => {});
    await page.waitForTimeout(6000);
    if ((await hasWrongCredential(page)) === "totp") {
      await debug.capture(page, cred.email, "twofactor-code-rejected");
      return null;
    }
    return secret;
  });
}

// Turn on 2-Step Verification for an already-signed-in account whose authenticator is added,
// skipping Google's "add a phone number" prompt so the account stays authenticator-only. Idempotent:
// returns true immediately when 2-Step already reads as on, so a resumed run is safe. Throws
// NotLoggedIn without a session. Not unit-tested; verify with --headed on the first account.
export async function turnOnTwoStep(cred, { headed = false, rotate = false } = {}) {
  return withProfile(cred.email, { headed, rotate }, async (_context, page, proxyUrl) => {
    if (!(await isGoogleLoggedIn(page))) throw new NotLoggedIn(`${cred.email}: no Google session; run \`login google\` first`);
    await gotoWithRetry(page, TWOSV_URL);
    if (!(await reauthSensitive(page, cred, "twosv", { assist: headed, proxyUrl }))) {
      await debug.capture(page, cred.email, "twofactor-reauth-failed");
      return false;
    }
    await ensureEnglish(page); // the re-auth's followup hop can drop `hl`
    await page.waitForTimeout(2000);
    const isOn = async () =>
      /2-Step Verification is on|Turn off/i.test((await page.evaluate(() => document.body.innerText)).replace(/\s+/g, " "));
    if (await isOn()) return true; // already on — nothing to do
    const turnOn = page.getByRole("button", { name: /^Turn on( 2-Step Verification)?$/i }).first();
    if (!(await turnOn.isVisible({ timeout: 6000 }).catch(() => false))) {
      await debug.capture(page, cred.email, "twofactor-no-turn-on");
      return false;
    }
    await turnOn.click();
    await page.waitForTimeout(4000);
    await reauthSensitive(page, cred, "twosv", { assist: headed, proxyUrl }); // Google may re-prompt on the switch
    // "Add a phone number for two-step verification?" — Skip it to stay authenticator-only.
    const skip = page.getByRole("button", { name: /^Skip$/i }).first();
    if (await skip.isVisible({ timeout: 5000 }).catch(() => false)) await skip.click();
    await page.waitForTimeout(3000);
    const confirm = page.getByRole("button", { name: /^(Turn on|Confirm|Done|OK)$/i }).first();
    if (await confirm.isVisible({ timeout: 3000 }).catch(() => false)) {
      await confirm.click();
      await page.waitForTimeout(4000);
    }
    return isOn();
  });
}

// Log one account into Google, then refresh each requested app's session. The browser closes as
// soon as the exports finish; the profile stays on disk.
export async function runAccount(db, cred, adapters, { headed = false, rotate = false } = {}) {
  // A headed run is interactive, so the flow may pause for the person to clear a reCAPTCHA in the
  // window; `assist` is that "a human is at the window", derived from headed — never a separate flag.
  const assist = headed;
  await withProfile(cred.email, { headed, rotate }, async (_context, page, proxyUrl) => {
    await signInGoogle(page, cred, { assist });
    store.markLoggedIn(db, cred.email);
    for (const adapter of adapters) await exportApp(page, db, adapter, cred, { assist, proxyUrl });
  });
}
