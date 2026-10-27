// ABOUTME: Tests the plugin's bin/browse launcher against a temp CLAUDE_PLUGIN_DATA: the first call builds, later calls reuse
// ABOUTME: the build, concurrent first calls build once, and stdout carries only browse's own output.
//
// Run: node --test tests/
//
// Uses `browse --help`, which prints usage and exits without starting the daemon.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const LAUNCHER = fileURLToPath(new URL("../../../bin/browse", import.meta.url));

function launch(dataDir, cwd, launcher = LAUNCHER, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(launcher, ["--help"], { cwd, env: { ...process.env, CLAUDE_PLUGIN_DATA: dataDir, ...env } });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

function assertCleanHelp(result) {
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^gstack browse — /);
  assert.doesNotMatch(result.stdout, /\[browse\] building|bun install|Bundled/);
}

const builds = (result) => (result.stderr.match(/\[browse\] building /g) ?? []).length;

test("the first call builds into CLAUDE_PLUGIN_DATA and a second call reuses that build", async () => {
  const dataDir = mkdtempSync(join(tmpdir(), "web-browse-data-"));
  try {
    const first = await launch(dataDir, dataDir);
    assertCleanHelp(first);
    assert.equal(builds(first), 1);

    const second = await launch(dataDir, dataDir);
    assertCleanHelp(second);
    assert.equal(builds(second), 0);
    assert.equal(readdirSync(join(dataDir, "browse")).length, 1);
  } finally {
    rmSync(dataDir, { recursive: true, force: true });
  }
});

test("two concurrent first calls build once", async () => {
  const dataDir = mkdtempSync(join(tmpdir(), "web-browse-data-"));
  try {
    const results = await Promise.all([launch(dataDir, dataDir), launch(dataDir, dataDir)]);
    for (const result of results) assertCleanHelp(result);
    assert.equal(results.reduce((sum, result) => sum + builds(result), 0), 1);
    assert.equal(readdirSync(join(dataDir, "browse")).length, 1);
  } finally {
    rmSync(dataDir, { recursive: true, force: true });
  }
});

test("a launcher from a superseded install runs the currently installed version instead", async () => {
  const root = mkdtempSync(join(tmpdir(), "web-browse-install-"));
  try {
    const versions = join(root, "cache", "marketplace", "web");
    const stale = join(versions, "0.0.1", "bin");
    const current = join(versions, "0.0.2", "bin");
    mkdirSync(stale, { recursive: true });
    mkdirSync(current, { recursive: true });
    copyFileSync(LAUNCHER, join(stale, "browse"));
    writeFileSync(join(current, "browse"), '#!/bin/bash\necho "current $*"\n');
    chmodSync(join(current, "browse"), 0o755);
    mkdirSync(join(root, "config", "plugins"), { recursive: true });
    writeFileSync(join(root, "config", "plugins", "installed_plugins.json"), JSON.stringify({
      version: 2,
      plugins: { "web@marketplace": [{ scope: "user", installPath: join(versions, "0.0.2"), version: "0.0.2" }] },
    }));

    const result = await launch(root, root, join(stale, "browse"), { CLAUDE_CONFIG_DIR: join(root, "config") });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, "current --help\n");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
