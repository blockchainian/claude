import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const config = JSON.parse(readFileSync(new URL("../.mcp.json", import.meta.url), "utf8"));

test("parallel simulator calls expose explicit targets rather than shared defaults", () => {
  const server = config.mcpServers.xcodebuildmcp;
  assert.equal(server.env.XCODEBUILDMCP_DISABLE_SESSION_DEFAULTS, "true");
  const workflows = server.env.XCODEBUILDMCP_ENABLED_WORKFLOWS.split(",");
  assert.ok(workflows.includes("simulator"));
  assert.ok(workflows.includes("ui-automation"));
  assert.ok(server.args.includes("mcp"), "use the MCP entry point that applies this configuration");
});
