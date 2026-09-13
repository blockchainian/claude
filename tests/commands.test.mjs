import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const commands = ["review", "rescue"];

test("the marketplace lists every plugin in the repository", async () => {
  const marketplace = JSON.parse(await readFile(".claude-plugin/marketplace.json", "utf8"));

  assert.equal(marketplace.name, "blockchainian");
  assert.deepEqual(marketplace.plugins.map((entry) => entry.name).sort(), ["cloudflare", "codex", "feature", "grok", "intel", "mobile", "proxy", "render", "web"]);

  for (const entry of marketplace.plugins) {
    const plugin = JSON.parse(await readFile(`${entry.source}/.claude-plugin/plugin.json`, "utf8"));

    assert.equal(plugin.name, entry.name);
  }
});

test("the shared check-design engine stays byte-identical across plugins", async () => {
  const copies = [
    "plugins/mobile/skills/check-mobile-design/scripts",
    "plugins/web/skills/check-web-design/scripts",
  ];
  for (const file of ["check_design.py", "test_check_design.py"]) {
    const [a, b] = await Promise.all(copies.map((dir) => readFile(`${dir}/${file}`, "utf8")));
    assert.equal(a, b, `${file} has drifted between the mobile and web copies`);
  }
});

test("slash command files expose the expected Grok commands", async () => {
  for (const command of commands) {
    const markdown = await readFile(`plugins/grok/commands/${command}.md`, "utf8");

    assert.match(markdown, /^---\n/);
    assert.match(markdown, /description:/);
  }
});

test("direct commands execute the companion script through the plugin root", async () => {
  const review = await readFile("plugins/grok/commands/review.md", "utf8");

  for (const markdown of [review]) {
    assert.match(markdown, /disable-model-invocation: true/);
    assert.match(markdown, /node "\$\{CLAUDE_PLUGIN_ROOT\}\/scripts\/grok-companion\.mjs"/);
  }
});
