#!/usr/bin/env node
// ABOUTME: Starts the claude-client MCP server that lets Codex communicate with Claude.

import { readFile } from "node:fs/promises";
import { runClaude } from "./claude.mjs";

const usage = `usage:
  client.mjs mcp   serve notify_claude and ask_claude to codex over stdio`;

try {
  const command = process.argv[2];
  if (command !== "mcp") throw new Error(command ? `unknown command: ${command}` : "missing command");
  const manifest = JSON.parse(await readFile(new URL("../.codex-plugin/plugin.json", import.meta.url), "utf8"));
  process.exitCode = await runClaude(manifest.version);
} catch (error) {
  process.stderr.write(`${error.message}\n${usage}\n`);
  process.exitCode = error.exitCode || 2;
}
