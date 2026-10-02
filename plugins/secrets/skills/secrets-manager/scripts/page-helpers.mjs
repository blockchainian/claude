export async function hasLsKey(page, substr) {
  try {
    return await page.evaluate((s) => Object.keys(localStorage).some((k) => k.includes(s)), substr);
  } catch {
    return false;
  }
}

export async function hasCookie(page, name) {
  return (await page.context().cookies()).some((c) => c.name === name);
}

export async function clickFirst(page, texts, timeout = 15000) {
  for (const text of texts) {
    try {
      await page.getByText(text, { exact: false }).first().click({ timeout });
      return true;
    } catch (e) {
      if (e?.name === "TimeoutError") continue;
      throw e;
    }
  }
  return false;
}

export async function gotoWithRetry(target, url, { waitUntil = "domcontentloaded", attemptMs = 5000, totalMs = 30000 } = {}) {
  const deadline = Date.now() + totalMs;
  const safeUrl = () => {
    try {
      return target.url();
    } catch {
      return "";
    }
  };
  const start = safeUrl();
  let lastErr;
  do {
    try {
      return await target.goto(url, { waitUntil, timeout: attemptMs });
    } catch (e) {
      lastErr = e;
      // A slow proxy can blow the per-attempt timeout AFTER the navigation already committed — the page
      // is on the new URL, just slow to fire `domcontentloaded`. Restarting would only reset a load that
      // would have finished, so accept the committed page and let the caller poll it for elements.
      const at = safeUrl();
      if (at && at !== start && !at.startsWith("about:")) return null;
    }
  } while (Date.now() < deadline);
  throw lastErr;
}

