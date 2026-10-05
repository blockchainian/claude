// ABOUTME: Live 实测 probe — drives Google's real reCAPTCHA v2 demo widget (same api2/anchor + bframe
// ABOUTME: DOM as the sign-in path) headless and times the changed waits. No account, read-only.
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  withProfile,
  visibleRecaptchaAnchor,
  recaptchaGridOpen,
  solveRecaptchaGrid,
  pollForState,
} from "./login.mjs";

// Where to drop the machine-readable timing summary. Override with PROBE_OUT for a known location.
const OUT = process.env.PROBE_OUT || join(tmpdir(), "recaptcha-probe-result.json");
const cred = { email: "recaptcha-demo-probe" };
const now = () => Date.now();
const result = { startedAt: new Date().toISOString(), head: process.env.PROBE_LABEL || "HEAD", steps: {} };

try {
  await withProfile(cred.email, { headed: false }, async (_context, page) => {
    const t0 = now();
    await page.goto("https://www.google.com/recaptcha/api2/demo", { waitUntil: "domcontentloaded", timeout: 60000 });
    result.steps.navMs = now() - t0;

    // #1 — find the one visible api2/anchor checkbox iframe
    const tAnchor = now();
    const anchor = await pollForState(page, () => visibleRecaptchaAnchor(page), 15000);
    result.steps.anchorFoundMs = now() - tAnchor;
    if (!anchor) { result.outcome = "no-anchor"; return; }

    await anchor.click({ timeout: 5000 }).catch((e) => { result.clickErr = e.message; });

    // #3 — wait for the widget to tick or open the image grid
    const tWait = now();
    const outcome = await pollForState(page, async () => {
      if ((await anchor.getAttribute("aria-checked").catch(() => null)) === "true") return "ticked";
      if (await recaptchaGridOpen(page)) return "grid";
      return null;
    }, 15000);
    result.steps.tickOrGridMs = now() - tWait;
    result.outcome = outcome ?? "neither";

    // #8/#9/#10 — only reachable if the demo actually served a grid
    if (outcome === "grid") {
      const tSolve = now();
      result.gridSolved = await solveRecaptchaGrid(page, cred);
      result.steps.gridSolveMs = now() - tSolve;
    }
  });
} catch (e) {
  result.error = e?.message ?? String(e);
}

result.finishedAt = new Date().toISOString();
writeFileSync(OUT, JSON.stringify(result, null, 2));
console.log("PROBE DONE\n" + JSON.stringify(result, null, 2));
