#!/usr/bin/env node
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";

const GROK_BIN = process.env.GROK_BIN || "grok";
const GIT_BIN = process.env.GIT_BIN || "git";
const MAX_DIFF_CHARS = 180_000;
const MAX_GROK_TURNS = "3";

const REVIEW_SYSTEM_PROMPT = `You are Grok reviewing code for Claude Code.

Review the supplied git context only. Do not edit files. Focus on correctness,
security, maintainability, tests, and user-visible regressions. Lead with the
highest-severity findings. Include file paths and concrete evidence when
possible. If there are no material findings, say so clearly.`;

async function main() {
  const [command, rawArguments = ""] = process.argv.slice(2);

  if (command === "review") {
    await review(rawArguments);
    return;
  }

  usage();
  process.exitCode = 2;
}

async function review(rawArguments) {
  const { base, focus } = parseReviewArguments(rawArguments);
  const context = base ? await branchDiffContext(base) : await workingTreeDiffContext();

  if (!context.hasChanges) {
    console.log(base ? `No changes found against ${base}.` : "No working tree changes found.");
    return;
  }

  const prompt = [
    REVIEW_SYSTEM_PROMPT,
    focus ? `\nReviewer focus:\n${focus}` : "",
    `\nRepository: ${process.cwd()}`,
    `\nReview target:\n${context.target}`,
    `\nGit status:\n${context.status || "(not requested)"}`,
    `\nDiff summary:\n${context.summary || "(no summary)"}`,
    `\nDiff:\n${truncate(context.diff, MAX_DIFF_CHARS)}`
  ].join("\n");

  await runGrokPrompt(prompt, { readOnly: true });
}

async function workingTreeDiffContext() {
  const [status, unstagedSummary, unstagedDiff, stagedSummary, stagedDiff, untracked] =
    await Promise.all([
      captureGit(["status", "--short"]),
      captureGit(["diff", "--stat"]),
      captureGit(["diff", "--find-renames"]),
      captureGit(["diff", "--cached", "--stat"]),
      captureGit(["diff", "--cached", "--find-renames"]),
      captureGit(["ls-files", "--others", "--exclude-standard"])
    ]);

  const diff = joinSections([
    ["Staged diff", stagedDiff.stdout],
    ["Unstaged diff", unstagedDiff.stdout],
    ["Untracked files", untracked.stdout]
  ]);

  return {
    target: "working tree changes",
    status: status.stdout.trimEnd(),
    summary: joinSections([
      ["Staged summary", stagedSummary.stdout],
      ["Unstaged summary", unstagedSummary.stdout]
    ]),
    diff,
    hasChanges: Boolean(status.stdout.trim() || diff.trim())
  };
}

async function branchDiffContext(base) {
  const [status, summary, diff] = await Promise.all([
    captureGit(["status", "--short"]),
    captureGit(["diff", "--stat", `${base}...HEAD`]),
    captureGit(["diff", "--find-renames", `${base}...HEAD`])
  ]);

  return {
    target: `branch diff against ${base}`,
    status: status.stdout.trimEnd(),
    summary: summary.stdout.trim(),
    diff: diff.stdout,
    hasChanges: Boolean(summary.stdout.trim() || diff.stdout.trim())
  };
}

async function runGrokPrompt(prompt, { readOnly }) {
  const directory = await mkdtemp(path.join(tmpdir(), "grok-plugin-"));
  const promptPath = path.join(directory, "prompt.md");

  try {
    await writeFile(promptPath, prompt, "utf8");

    const args = [
      "--cwd",
      process.cwd(),
      "--prompt-file",
      promptPath,
      "--no-memory",
      "--disable-web-search",
      "--no-subagents",
      "--max-turns",
      MAX_GROK_TURNS
    ];

    if (readOnly) {
      args.push("--tools", "");
    }

    await inherit(GROK_BIN, args);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function captureGit(args) {
  return capture(GIT_BIN, args);
}

function capture(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: process.cwd(),
      env: process.env,
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
      if (code === 0) {
        resolve({ stdout, stderr });
        return;
      }

      reject(new Error(`${command} ${args.join(" ")} failed with exit ${code}\n${stderr}`));
    });
  });
}

function inherit(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: process.cwd(),
      env: process.env,
      stdio: "inherit"
    });

    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(new Error(`${command} exited with ${code}`));
    });
  });
}

function parseReviewArguments(rawArguments) {
  const tokens = splitShellWords(rawArguments);
  const focus = [];
  let base = null;

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];

    if (token === "--base") {
      base = tokens[index + 1];
      index += 1;
      continue;
    }

    focus.push(token);
  }

  if (base === undefined) {
    throw new Error("Missing value for --base");
  }

  return { base, focus: focus.join(" ").trim() };
}

function splitShellWords(input) {
  const words = [];
  let word = "";
  let quote = null;
  let escaping = false;

  for (const character of input.trim()) {
    if (escaping) {
      word += character;
      escaping = false;
      continue;
    }

    if (character === "\\") {
      escaping = true;
      continue;
    }

    if (quote) {
      if (character === quote) {
        quote = null;
      } else {
        word += character;
      }
      continue;
    }

    if (character === "\"" || character === "'") {
      quote = character;
      continue;
    }

    if (/\s/.test(character)) {
      if (word) {
        words.push(word);
        word = "";
      }
      continue;
    }

    word += character;
  }

  if (escaping) {
    word += "\\";
  }

  if (quote) {
    throw new Error(`Unterminated ${quote} quote`);
  }

  if (word) {
    words.push(word);
  }

  return words;
}

function joinSections(sections) {
  return sections
    .filter(([, value]) => value.trim())
    .map(([heading, value]) => `## ${heading}\n${value.trim()}`)
    .join("\n\n");
}

function truncate(value, maxLength) {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, maxLength)}\n\n[Diff truncated at ${maxLength} characters.]`;
}

function usage() {
  console.error("Usage: grok-companion.mjs <review> [arguments]");
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
