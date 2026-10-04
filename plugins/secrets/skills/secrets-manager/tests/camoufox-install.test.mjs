// ABOUTME: Tests the pinned browser's platform assets, version checks, and download skipping.
// ABOUTME: Uses temporary version files and a fake installer; no network or browser.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { assetName, ensurePinnedBrowser, browserReport } from "../scripts/camoufox-install.mjs";

test("pinned assets match supported platform and architecture names", () => {
  for (const [platform, arch, suffix] of [
    ["darwin", "arm64", "mac.arm64"], ["darwin", "x64", "mac.x86_64"],
    ["linux", "x64", "lin.x86_64"], ["linux", "arm64", "lin.arm64"],
  ]) assert.equal(assetName(platform, arch), `camoufox-152.0.4-beta.30-${suffix}.zip`);
  assert.throws(() => assetName("darwin", "ia32"), /Unsupported/);
  assert.throws(() => assetName("linux", "ia32"), /Unsupported/);
});

test("version checks report missing or mismatched browsers and skip only the exact pin", async () => {
  const dir = mkdtempSync(join(tmpdir(), "camoufox-pin-"));
  let installs = 0;
  const install = async () => {
    installs++;
    writeFileSync(join(dir, "version.json"), JSON.stringify({ version: "152.0.4", release: "beta.30" }));
  };
  try {
    assert.match(browserReport(dir), /missing.*152\.0\.4-beta\.30/);
    writeFileSync(join(dir, "version.json"), JSON.stringify({ version: "156.0.1", release: "beta.34" }));
    assert.match(browserReport(dir), /156\.0\.1-beta\.34.*mismatch.*152\.0\.4-beta\.30/);
    await ensurePinnedBrowser(dir, install);
    await ensurePinnedBrowser(dir, install);
    assert.equal(installs, 1);
    assert.match(browserReport(dir), /152\.0\.4-beta\.30.*matches pin/);
    writeFileSync(join(dir, "version.json"), "bad json");
    await assert.rejects(ensurePinnedBrowser(dir, install), SyntaxError);
    assert.equal(installs, 1);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
