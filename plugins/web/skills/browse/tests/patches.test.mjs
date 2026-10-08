// ABOUTME: Tests the browse patch workflow: GSTACK_COMMIT matches the gstack submodule pointer, the patch scripts
// ABOUTME: behave like codex's, and build.sh fetches gstack at that commit, patches it, and passes the patches' own bun tests.
//
// Run: node --test tests/
//
// Needs the gstack submodule (`git submodule update --init`), network access to fetch gstack for the build,
// bun on PATH and a Playwright Chromium. Tests that open a visible window stay skipped (BROWSE_HEADED_TESTS unset).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SKILL_DIR = fileURLToPath(new URL("..", import.meta.url));
const GSTACK = join(SKILL_DIR, "gstack");
const SCRIPTS = join(SKILL_DIR, "scripts");
const PINNED = readFileSync(join(SKILL_DIR, "GSTACK_COMMIT"), "utf8").trim();
const PATCH_COUNT = readdirSync(join(SKILL_DIR, "patches")).filter((name) => /^\d{4}-.+\.patch$/.test(name)).length;
const PATCH_TESTS = ["browser-display", "background-tab", "tab-scoped-logs", "headed-newtab"]
  .map((stem) => `browse/test/${stem}.test.ts`);

function exec(command, args, cwd, env = process.env) {
  return spawnSync(command, args, { cwd, encoding: "utf8", env });
}

function run(command, args, cwd, env) {
  const result = exec(command, args, cwd, env);
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

// Runs fn with a path for a worktree of the submodule, and removes that worktree afterwards.
function withWorktree(fn) {
  const root = mkdtempSync(join(tmpdir(), "web-browse-worktree-"));
  const worktree = join(root, "tree");
  try {
    return fn(worktree);
  } finally {
    exec("git", ["worktree", "remove", "--force", worktree], GSTACK);
    rmSync(root, { recursive: true, force: true });
  }
}

test("GSTACK_COMMIT names the gstack submodule pointer", () => {
  assert.match(PINNED, /^[0-9a-f]{40}$/);
  assert.equal(run("git", ["rev-parse", ":./gstack"], SKILL_DIR).trim(), PINNED);
});

test("build.sh fetches from the gstack submodule's URL", () => {
  const url = run("git", ["config", "--file", ".gitmodules", "submodule.plugins/web/skills/browse/gstack.url"], join(SKILL_DIR, "..", "..", "..", "..")).trim();
  assert.match(readFileSync(join(SCRIPTS, "build.sh"), "utf8"), new RegExp(`^GSTACK_URL=${url.replace(/[.]/g, "\\.")}$`, "m"));
});

test("apply-patches.sh refuses a dirty source and keeps its changes", () => {
  withWorktree((worktree) => {
    run("git", ["worktree", "add", "--detach", worktree, PINNED], GSTACK);
    writeFileSync(join(worktree, "stray.txt"), "keep me\n");
    const result = exec(join(SCRIPTS, "apply-patches.sh"), [worktree], SKILL_DIR);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Source must be clean/);
    assert.equal(readFileSync(join(worktree, "stray.txt"), "utf8"), "keep me\n");
  });
});

test("develop.sh commits every patch as the baseline of a fresh worktree", () => {
  withWorktree((worktree) => {
    run(join(SCRIPTS, "develop.sh"), [worktree], SKILL_DIR);
    assert.equal(run("git", ["rev-parse", "HEAD~1"], worktree).trim(), PINNED);
    assert.equal(run("git", ["status", "--porcelain", "--untracked-files=all"], worktree), "");
    for (const file of PATCH_TESTS) assert.ok(existsSync(join(worktree, file)), file);

    const again = exec(join(SCRIPTS, "develop.sh"), [worktree], SKILL_DIR);
    assert.notEqual(again.status, 0);
    assert.match(again.stderr, /Worktree already exists/);
  });
});

test("check-patches.sh applies every patch in order and removes its worktree", () => {
  const out = run(join(SCRIPTS, "check-patches.sh"), [], SKILL_DIR);
  assert.match(out, new RegExp(`All patches apply in order to ${PINNED}`));
  assert.equal((out.match(/^Applying /gm) ?? []).length, PATCH_COUNT);
  assert.ok(!existsSync(join(SKILL_DIR, ".worktrees", "check-patches")));
});

test("build.sh fetches gstack at GSTACK_COMMIT, patches it, and the patches' own tests pass", () => {
  const root = mkdtempSync(join(tmpdir(), "web-browse-build-"));
  const dest = join(root, "browse");
  const env = { ...process.env };
  delete env.BROWSE_HEADED_TESTS;
  try {
    // The real install dir can sit inside a git work tree (a dotfiles repo holding ~/.claude).
    run("git", ["init", "-q"], root);
    run("bash", [join(SCRIPTS, "build.sh"), "--dest", dest], SKILL_DIR);

    assert.ok(existsSync(join(dest, "browse", "dist", "browse")));
    assert.equal(run("git", ["rev-parse", "HEAD"], dest).trim(), PINNED);
    const buildId = run("bash", [join(SCRIPTS, "build-id.sh")], SKILL_DIR);
    assert.match(buildId, /^[0-9a-f]{16}\n$/);
    assert.equal(readFileSync(join(dest, "browse", "dist", ".version"), "utf8"), buildId);
    run("bun", ["test", ...PATCH_TESTS], dest, env);

    // A headless daemon on its own state file, profile and HOME records the build it was started by,
    // which is what makes the next command from a different build restart it.
    const daemonEnv = {
      ...env,
      HOME: join(root, "home"),
      BROWSE_STATE_FILE: join(root, "state", "browse.json"),
      CHROMIUM_PROFILE: join(root, "profile"),
      PLAYWRIGHT_BROWSERS_PATH: env.PLAYWRIGHT_BROWSERS_PATH ?? join(homedir(), "Library", "Caches", "ms-playwright"),
    };
    delete daemonEnv.BROWSE_HEADED;
    const cli = join(dest, "browse", "dist", "browse");
    try {
      run(cli, ["status"], root, daemonEnv);
      assert.equal(JSON.parse(readFileSync(daemonEnv.BROWSE_STATE_FILE, "utf8")).binaryVersion, buildId.trim());
    } finally {
      exec(cli, ["stop"], root, daemonEnv);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
