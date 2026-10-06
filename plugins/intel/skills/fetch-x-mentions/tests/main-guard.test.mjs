// ABOUTME: The research scripts run their main() when invoked through a symlinked path, as the
// ABOUTME: plugin cache is reached through one (~/.claude may be a symlink).
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const skills = fileURLToPath(new URL("../../", import.meta.url));
const scripts = [
  "fetch-x-posts/scripts/fetch-x-posts.mjs",
  "fetch-x-user-posts/scripts/fetch-x-user-posts.mjs",
  "analyze-x-mentions/scripts/run-labels.mjs",
  "fetch-tiktok-mentions/scripts/fetch-tiktok-mentions.mjs",
  "fetch-x-mentions/scripts/fetch-x-mentions.mjs",
  "fetch-x-mentions/scripts/verify-x.mjs",
  "fetch-app-reviews/scripts/fetch-app-reviews.mjs",
];

test("each script's main runs when the script is reached through a symlink", () => {
  const home = mkdtempSync(join(tmpdir(), "main-guard-"));
  const link = join(home, "skills");
  symlinkSync(skills, link);
  const fixtureEnv = {
    INTEL_RESIDENTIAL_PROXY_URL: "http://proxy.invalid:1", INTEL_X_BEARER_TOKEN: "x", INTEL_X_SEARCH_QUERY_ID: "x",
    INTEL_X_USER_QUERY_ID: "x", INTEL_X_TIMELINE_QUERY_ID: "x", INTEL_X_TID_VERIFICATION: "x", INTEL_X_TID_FRAME: "x",
    INTEL_X_TID_ROW: "0", INTEL_X_TID_INDICES: "0", INTEL_ISP_PROXY_URL: "http://proxy.invalid:1", INTEL_ISP_PROXY_COUNT: "1",
  };
  for (const rel of scripts) {
    const r = spawnSync(process.execPath, [...process.execArgv, join(link, rel)], {
      encoding: "utf8",
      env: { ...process.env, ...fixtureEnv, INTEL_SECRETS_STATE_DIR: home },
      timeout: 20000,
    });
    assert.ok(r.stderr.trim().length > 0, `${rel}: main did not run (no stderr)`);
    assert.doesNotMatch(r.stderr, /Error/, `${rel}: crashed instead of running main: ${r.stderr.slice(0, 200)}`);
  }
});
