// ABOUTME: Tests the marketplace manifest: it lists every plugin in the repository, and the check-design
// ABOUTME: engine shared by the mobile and web plugins and the window recorder shared by secrets and creator stay byte-identical.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("the marketplace lists every plugin in the repository", async () => {
  const marketplace = JSON.parse(await readFile(".claude-plugin/marketplace.json", "utf8"));

  assert.equal(marketplace.name, "blockchainian");
  assert.deepEqual(marketplace.plugins.map((entry) => entry.name).sort(), ["cloudflare", "codex", "creator", "feature", "intel", "mobile", "proxy", "render", "secrets", "web"]);

  for (const entry of marketplace.plugins) {
    const plugin = JSON.parse(await readFile(`${entry.source}/.claude-plugin/plugin.json`, "utf8"));

    assert.equal(plugin.name, entry.name);
  }
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

test("the window recorder stays byte-identical across plugins", async () => {
  const copies = [
    "plugins/secrets/skills/secrets-manager/scripts/recordWindows.swift",
    "plugins/creator/skills/upload-tiktok-video/scripts/recordWindows.swift",
  ];
  const [a, b] = await Promise.all(copies.map((path) => readFile(path, "utf8")));
  assert.equal(a, b, "recordWindows.swift has drifted between the secrets and creator copies");
});
