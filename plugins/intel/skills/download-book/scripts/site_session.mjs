// ABOUTME: Fetches Anna's Archive pages through a headed Chrome window so DDoS-Guard's
// ABOUTME: browser check is solved by the real browser, then reuses its cookies for plain fetches.
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { join } from 'node:path';

const PROFILE_DIR = join(homedir(), '.cache', 'blockchainian', 'download-book-profile');
// The site's own pages mention DDoS-Guard in script comments; only the challenge page carries these markers.
const CHALLENGE = /<title>\s*DDoS-Guard\s*<\/title>|\/\.well-known\/ddos-guard\/(js-challenge|ddg-captcha-page)\//i;
const CHALLENGE_TIMEOUT = 45_000;
const BODY_LIMIT = 3_000_000;

// Runs inside the page: fetch with the browser's cookies and TLS fingerprint.
async function pageFetch([url, follow]) {
  const response = await fetch(url, { redirect: follow ? 'follow' : 'manual' });
  return { status: response.status, redirected: response.type === 'opaqueredirect', body: await response.text() };
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

/** Opens a headed Chrome on the site's origin, so in-page fetches are same-origin; DDoS-Guard serves a captcha to headless browsers. */
export async function openSession(origin) {
  let chromium;
  try { ({ chromium } = createRequire(import.meta.url)('playwright')); }
  catch { throw new Error('缺少 playwright：请在本脚本目录运行 npm install'); }
  const context = await chromium.launchPersistentContext(PROFILE_DIR, {
    headless: false,
    channel: 'chrome',
    viewport: null,
    args: ['--disable-blink-features=AutomationControlled'],
    ignoreDefaultArgs: ['--enable-automation', '--disable-extensions', '--disable-component-extensions-with-background-pages', '--disable-popup-blocking', '--disable-component-update', '--disable-default-apps'],
  });
  const page = context.pages()[0] ?? await context.newPage();
  await page.goto(origin, { waitUntil: 'domcontentloaded' });
  return { ...sessionFor(page), close: () => context.close() };
}
