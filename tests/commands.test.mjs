import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const commands = ["review", "rescue"];

test("the marketplace lists every plugin in the repository", async () => {
  const marketplace = JSON.parse(await readFile(".claude-plugin/marketplace.json", "utf8"));

  assert.equal(marketplace.name, "blockchainian");
  assert.deepEqual(marketplace.plugins.map((entry) => entry.name).sort(), ["codex", "grok"]);

  for (const entry of marketplace.plugins) {
    const plugin = JSON.parse(await readFile(`${entry.source}/.claude-plugin/plugin.json`, "utf8"));

    assert.equal(plugin.name, entry.name);
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
