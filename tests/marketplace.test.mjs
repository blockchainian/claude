import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("the marketplace lists every plugin in the repository", async () => {
  const marketplace = JSON.parse(await readFile(".claude-plugin/marketplace.json", "utf8"));

  assert.equal(marketplace.name, "blockchainian");
  assert.deepEqual(marketplace.plugins.map((entry) => entry.name).sort(), ["cloudflare", "codex", "feature", "intel", "mobile", "proxy", "render", "web"]);

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
