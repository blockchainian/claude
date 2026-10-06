// ABOUTME: Tests the marketplace manifest: it lists every plugin in the repository, and the check-design
// ABOUTME: engine shared by the mobile and web plugins and the window recorder and mover shared by secrets and creator stay byte-identical.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("the marketplace lists every plugin in the repository", async () => {
  const marketplace = JSON.parse(await readFile(".claude-plugin/marketplace.json", "utf8"));

  assert.equal(marketplace.name, "blockchainian");
  assert.deepEqual(marketplace.plugins.map((entry) => entry.name).sort(), ["cloudflare", "codex", "computer-use", "creator", "feature", "intel", "mobile", "proxy", "render", "secrets", "web"]);

  for (const entry of marketplace.plugins) {
    const plugin = JSON.parse(await readFile(`${entry.source}/.claude-plugin/plugin.json`, "utf8"));

    assert.equal(plugin.name, entry.name);
  }
});

test("the Codex marketplace exposes only the eight selected plugins", async () => {
  const marketplace = JSON.parse(await readFile(".agents/plugins/marketplace.json", "utf8"));
  assert.equal(marketplace.name, "blockchainian");
  assert.deepEqual(marketplace.plugins.map(({ name }) => name), ["cloudflare", "web", "proxy", "creator", "render", "mobile", "intel", "secrets"]);

  for (const entry of marketplace.plugins) {
    assert.equal(entry.source.source, "local");
    assert.equal(entry.source.path, `./plugins/${entry.name}`);
    assert.deepEqual(entry.policy, { installation: "AVAILABLE", authentication: "ON_USE" });
    const plugin = JSON.parse(await readFile(`${entry.source.path}/.claude-plugin/plugin.json`, "utf8"));
    assert.equal(plugin.name, entry.name);
  }
});

test("Render keeps OAuth clients and edit hooks scoped to their host", async () => {
  const root = "plugins/render";
  const [claude, codex, mcp] = await Promise.all([
    `${root}/.claude-plugin/plugin.json`,
    `${root}/.codex-plugin/plugin.json`,
    `${root}/.mcp.json`,
  ].map(async (path) => JSON.parse(await readFile(path, "utf8"))));
  assert.equal(codex.name, claude.name);
  assert.equal(codex.version, claude.version);
  assert.equal(codex.mcpServers.render.url, mcp.mcpServers.render.url);
  assert.equal(codex.mcpServers.render.oauth.clientId, "codex");
  assert.equal(mcp.mcpServers.render.oauth.clientId, "claude");
  const codexHooks = JSON.parse(await readFile(`${root}/${codex.hooks}`, "utf8"));
  assert.deepEqual(codexHooks.hooks, {});
  const claudeHooks = JSON.parse(await readFile(`${root}/hooks/hooks.json`, "utf8"));
  assert.equal(claudeHooks.hooks.PostToolUse[0].matcher, "Edit|Write|MultiEdit");
});

test("the shared check-design engine stays byte-identical across plugins", async () => {
  const copies = [
    "plugins/mobile/skills/check-mobile-design",
    "plugins/web/skills/check-web-design",
  ];
  for (const file of ["scripts/check_design.py", "tests/test_check_design.py"]) {
    const [a, b] = await Promise.all(copies.map((dir) => readFile(`${dir}/${file}`, "utf8")));
    assert.equal(a, b, `${file} has drifted between the mobile and web copies`);
  }
});

for (const script of ["recordWindows.swift", "moveWindows.swift"]) {
  test(`the window script ${script} stays byte-identical across plugins`, async () => {
    const copies = [`plugins/secrets/skills/secrets-manager/scripts/${script}`, `plugins/creator/skills/upload-tiktok-video/scripts/${script}`];
    const [a, b] = await Promise.all(copies.map((path) => readFile(path, "utf8")));
    assert.equal(a, b, `${script} has drifted between the secrets and creator copies`);
  });
}

test("Intel shares fourteen portable skills and keeps case-study Claude-only", async () => {
  const root = "plugins/intel";
  const claude = JSON.parse(await readFile(`${root}/.claude-plugin/plugin.json`, "utf8"));
  const codex = JSON.parse(await readFile(`${root}/.codex-plugin/plugin.json`, "utf8"));
  assert.equal(codex.name, claude.name);
  assert.equal(codex.version, claude.version);
  assert.deepEqual(codex.skills, [
    "analyze-appstore-reviews", "analyze-x-mentions", "analyze-x-user", "analyze-x-users",
    "digest", "download-book", "fetch-app-reviews", "fetch-tiktok-mentions",
    "fetch-x-mentions", "fetch-x-posts", "fetch-x-user-posts", "find-domain-names",
    "transcribe", "translate",
  ].map((name) => `./skills/${name}`));
  for (const skill of codex.skills) {
    const text = await readFile(`${root}/${skill}/SKILL.md`, "utf8");
    assert.ok(!text.includes("CLAUDE_PLUGIN_ROOT"), skill);
    assert.ok(text.includes("SKILL_DIR"), skill);
  }
  const excluded = await readFile(`${root}/skills/case-study/SKILL.md`, "utf8");
  assert.ok(excluded.includes("Workflow tool"));
});
