// ABOUTME: Tests the vendored browse build: every patch applies cleanly to a fresh copy of vendor/gstack,
// ABOUTME: and build.sh produces a binary whose patched tree passes the patches' own bun tests.
//
// Run: node --test tests/
//
// Needs bun on PATH and a Playwright Chromium for the bun test; the build installs
// from the committed bun.lock, so it fetches packages the first time.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SKILL_DIR = fileURLToPath(new URL("..", import.meta.url));
const VENDOR = join(SKILL_DIR, "vendor", "gstack");
const PATCHES = join(SKILL_DIR, "patches");

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

test("every patch applies cleanly, in order, to a fresh copy of vendor/gstack", () => {
  const tree = mkdtempSync(join(tmpdir(), "web-browse-apply-"));
  try {
    cpSync(VENDOR, tree, { recursive: true });
    const patches = readdirSync(PATCHES).filter((name) => name.endsWith(".patch")).sort();
    assert.ok(patches.length > 0);
    for (const name of patches) run("git", ["apply", join(PATCHES, name)], tree);
  } finally {
    rmSync(tree, { recursive: true, force: true });
  }
});

test("build.sh builds the patched browse and the patches' own tests pass in it", () => {
  const root = mkdtempSync(join(tmpdir(), "web-browse-build-"));
  const dest = join(root, "browse");
  try {
    run("bash", [join(SKILL_DIR, "scripts", "build.sh"), "--dest", dest], SKILL_DIR);

    assert.ok(existsSync(join(dest, "browse", "dist", "browse")));
    const buildId = run("bash", [join(SKILL_DIR, "scripts", "build-id.sh")], SKILL_DIR);
    assert.match(buildId, /^[0-9a-f]{16}\n$/);
    assert.equal(readFileSync(join(dest, "browse", "dist", ".version"), "utf8"), buildId);
    run("bun", ["test", "browse/test/no-tab-recovery.test.ts", "browse/test/tab-scoped-logs.test.ts", "browse/test/binary-version.test.ts"], dest);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
