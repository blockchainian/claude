// ABOUTME: Fetches Anna's Archive pages through a headed Chrome window so DDoS-Guard's
// ABOUTME: browser check is solved by the real browser; every run has its own tab in one Chrome shared by all runs.
import { spawn } from 'node:child_process';
import { readFileSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { dataDir } from '../../fetch-x-mentions/scripts/env.mjs';

// The site's cookies only spare the next run a browser check, so the profile is deletable data.
const profileDir = () => join(dataDir(), 'download-book', 'profile');
// The site's own pages mention DDoS-Guard in script comments; only the challenge page carries these markers.
const CHALLENGE = /<title>\s*DDoS-Guard\s*<\/title>|\/\.well-known\/ddos-guard\/(js-challenge|ddg-captcha-page)\//i;
const CHALLENGE_TIMEOUT = 45_000;
const BODY_LIMIT = 3_000_000;
// The shared Chrome closes once no run has had a tab open for this long.
const IDLE_MS = 60_000;

// Runs inside the page: fetch with the browser's cookies and TLS fingerprint.
async function pageFetch([url, follow]) {
  const response = await fetch(url, { redirect: follow ? 'follow' : 'manual' });
  return { status: response.status, url: response.url, body: await response.text() };
}

export function isChallenge(body) {
  return CHALLENGE.test(body);
}

/** Wraps a Playwright page: in-page fetch, and on a challenge navigate there so the browser solves it, then refetch once. */
export function sessionFor(page) {
  async function fetchOnce(url, follow) {
    const result = await page.evaluate(pageFetch, [url, follow]);
    if (result.body.length > BODY_LIMIT) throw new Error('页面超过 3 MB，已停止读取');
    return result;
  }
  return {
    async get(url, follow = true) {
      let result;
      try { result = await fetchOnce(url, follow); }
      catch (error) { throw new Error(error.message.includes('3 MB') ? error.message : '请求失败或超时'); }
      if (!isChallenge(result.body)) return result;
      await page.goto(url, { waitUntil: 'domcontentloaded' });
      await page.waitForFunction(() => !/ddos-guard/i.test(document.title), null, { timeout: CHALLENGE_TIMEOUT }).catch(() => {});
      return fetchOnce(url, follow);
    },
  };
}

function playwright() {
  try { return createRequire(import.meta.url)('playwright').chromium; }
  catch { throw new Error('缺少 playwright：请在本脚本目录运行 npm install'); }
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

// The port the shared Chrome listens on for other runs: Chrome writes it into its profile when it starts.
function debugPort() {
  try { return readFileSync(join(profileDir(), 'DevToolsActivePort'), 'utf8').split('\n')[0].trim() || null; }
  catch { return null; }
}

/** Connects to the shared Chrome. When none answers, starts one, once, and keeps trying until it is up. */
export async function attach({ port, connect, start, wait, tries = 60 }) {
  for (let i = 0; i <= tries; i++) {
    const listening = port();
    if (listening) {
      try { return await connect(listening); }
      catch { /* nothing answers on that port: the Chrome that wrote it is gone, or the new one is not up yet */ }
    }
    if (i === tries) break;
    if (i === 0) start();
    await wait();
  }
  throw new Error('Chrome 未能启动或无法连接');
}

/** Resolves once `tabs()` has shown no tab but the Chrome's own first one for `idle` ms, looking every `every` ms. */
export async function untilIdle(tabs, { idle = IDLE_MS, every = 5_000, now = Date.now, wait = sleep } = {}) {
  let busy = now();
  for (;;) {
    if (tabs() > 1) busy = now();
    else if (now() - busy >= idle) return;
    await wait(every);
  }
}

// The shared Chrome, headed (DDoS-Guard serves a captcha to headless browsers), open to other runs on a port of its
// own choosing. It runs in a process of its own, which outlives the run that started it and closes it when idle.
async function keepChrome() {
  const context = await playwright().launchPersistentContext(profileDir(), {
    headless: false,
    channel: 'chrome',
    chromiumSandbox: true,
    viewport: null,
    args: ['--disable-blink-features=AutomationControlled', '--remote-debugging-port=0'],
    ignoreDefaultArgs: ['--enable-automation', '--disable-extensions', '--disable-component-extensions-with-background-pages', '--disable-popup-blocking', '--disable-component-update', '--disable-default-apps'],
  });
  // Exit only after context.close() has let Chrome finish its shutdown: exiting while it still runs kills it, and
  // the next start then shows the "Restore pages?" bubble.
  await untilIdle(() => context.pages().length);
  await context.close();
}

/** Opens a tab without bringing Chrome to the front: a plain newPage() takes the focus from the app in use. */
async function backgroundTab(context, cdp) {
  const created = cdp.send('Target.createTarget', { url: 'about:blank', background: true });
  // Other runs open tabs in the same Chrome at the same time, so the tab is matched by its target id.
  const isOurs = async page => {
    const session = await context.newCDPSession(page);
    try { return (await session.send('Target.getTargetInfo')).targetInfo.targetId === (await created).targetId; }
    finally { await session.detach(); }
  };
  return context.waitForEvent('page', { predicate: isOurs });
}

/** Opens a tab of the shared Chrome on the site's origin, so in-page fetches are same-origin. Closing the session closes the tab only. */
export async function openSession(origin) {
  const chromium = playwright();
  const browser = await attach({
    port: debugPort,
    connect: port => chromium.connectOverCDP(`http://127.0.0.1:${port}`),
    start: () => spawn(process.execPath, [fileURLToPath(import.meta.url), 'keep'], { detached: true, stdio: 'ignore' }).unref(),
    wait: () => sleep(500),
  });
  const page = await backgroundTab(browser.contexts()[0], await browser.newBrowserCDPSession());
  await page.goto(origin, { waitUntil: 'domcontentloaded' });
  return { ...sessionFor(page), close: async () => { await page.close(); await browser.close(); } };
}

// Node resolves the main module through symlinks, so argv[1] must be resolved the same way.
if (process.argv[1] && pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url && process.argv[2] === 'keep') {
  keepChrome().catch(() => { process.exitCode = 1; });
}
