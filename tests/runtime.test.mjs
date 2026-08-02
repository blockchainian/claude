import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";

const companion = path.resolve("plugins/grok/scripts/grok-companion.mjs");

test("review builds a prompt from git status and diffs", async () => {
  const grok = await fakeExecutable("grok", `#!/usr/bin/env node
import { readFileSync } from "node:fs";
const promptFile = process.argv[process.argv.indexOf("--prompt-file") + 1];
console.log(readFileSync(promptFile, "utf8"));
`);
  const git = await fakeExecutable("git", `#!/usr/bin/env node
const args = process.argv.slice(2).join(" ");
if (args === "status --short") {
  console.log(" M src/app.js");
} else if (args === "diff --stat") {
  console.log(" src/app.js | 2 +-");
} else if (args === "diff --find-renames") {
  console.log("diff --git a/src/app.js b/src/app.js");
} else if (args === "diff --cached --stat" || args === "diff --cached --find-renames" || args === "ls-files --others --exclude-standard") {
  process.exit(0);
} else {
  console.error(args);
  process.exit(1);
}
`);

  const result = await runCompanion(["review", "focus on regressions"], {
    GROK_BIN: grok,
    GIT_BIN: git
  });

  assert.equal(result.code, 0);
  assert.match(result.stdout, /Reviewer focus:\nfocus on regressions/);
  assert.match(result.stdout, /Git status:\n M src\/app\.js/);
  assert.match(result.stdout, /diff --git a\/src\/app\.js b\/src\/app\.js/);
});

async function fakeExecutable(name, source) {
  const directory = await mkdtemp(path.join(tmpdir(), "grok-plugin-test-"));
  const file = path.join(directory, name);

  await writeFile(file, source, { mode: 0o755 });
  return file;
}

function runCompanion(args, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [companion, ...args], {
      cwd: process.cwd(),
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"]
    });

    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      resolve({ code, stdout, stderr });
    });
  });
}
