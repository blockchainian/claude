#!/usr/bin/env node
// ABOUTME: Runs one codex task as a persistent app-server daemon thread.
// ABOUTME: Drives one thread over the shared daemon client and reports the final agent message.

import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { WebSocketClient, daemonSocketPath } from "../../lib/daemon-client.mjs";

const usage = "usage: daemon-run.mjs -C <dir> -o <file> --name <name> --timeout <seconds> [-s <sandbox>] <prompt|->";

function parseArgs(argv) {
  const options = { sandbox: "workspace-write" };
  const values = new Map([["-C", "cwd"], ["-o", "output"], ["--name", "name"], ["--timeout", "timeout"], ["-s", "sandbox"]]);
  const positional = [];
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (values.has(argument)) {
      if (index + 1 >= argv.length) throw new Error("missing option value");
      options[values.get(argument)] = argv[++index];
    } else if (argument.startsWith("-") && argument !== "-") {
      throw new Error(`unknown option: ${argument}`);
    } else {
      positional.push(argument);
    }
  }
  const seconds = Number(options.timeout);
  if (!options.cwd || !options.output || !options.name || positional.length !== 1 || !Number.isFinite(seconds) || seconds <= 0) {
    throw new Error("invalid arguments");
  }
  return { ...options, cwd: path.resolve(options.cwd), timeout: seconds, prompt: positional[0] };
}

function completionPromise(client, state) {
  return new Promise((resolve, reject) => {
    client.onFailure = reject;
    client.onNotification = (message) => {
      const { method, params = {} } = message;
      if (params.threadId !== state.threadId) return;
      if (method === "item/started") {
        const item = params.item || {};
        if (item.type === "commandExecution") console.log(`[item commandExecution] ${item.command || ""}`);
        if (item.type === "fileChange") console.log(`[item fileChange] ${(item.changes || []).map((change) => change.path).join(", ")}`);
      }
      if (method === "item/completed" && params.item?.type === "agentMessage") {
        state.lastMessage = params.item.text || "";
        console.log(`[agent] ${state.lastMessage}`);
      }
      if (method === "turn/completed") resolve(params.turn);
    };
  });
}

async function session(client, options, state, version) {
  await client.connect();
  await client.request("initialize", { clientInfo: { title: "codex:implement", name: "codex-implement", version }, capabilities: { experimentalApi: false, requestAttestation: false, optOutNotificationMethods: [] } });
  client.send({ method: "initialized", params: {} });
  const started = await client.request("thread/start", { cwd: options.cwd, approvalPolicy: "never", sandbox: options.sandbox, serviceName: "codex-implement", ephemeral: false });
  state.threadId = started.thread.id;
  console.log(`[thread ${state.threadId}] ${options.name}`);
  await client.request("thread/name/set", { threadId: state.threadId, name: options.name });
  const completed = completionPromise(client, state);
  const turn = await client.request("turn/start", { threadId: state.threadId, input: [{ type: "text", text: options.prompt, text_elements: [] }] });
  state.turnId = turn.turn.id;
  return completed;
}

async function run(options) {
  const pluginPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../.claude-plugin/plugin.json");
  const plugin = JSON.parse(await readFile(pluginPath, "utf8"));
  const socketPath = daemonSocketPath(process.env.IMPLEMENT_DAEMON_SOCKET);
  const client = new WebSocketClient(socketPath);
  const state = { threadId: undefined, turnId: undefined, lastMessage: "" };
  let forcedCode;
  let force;
  const forced = new Promise((resolve) => { force = resolve; });
  const stop = (code) => {
    if (forcedCode !== undefined) return;
    forcedCode = code;
    if (state.turnId === undefined) {
      force(undefined);
      return;
    }
    client.request("turn/interrupt", { threadId: state.threadId, turnId: state.turnId }).catch(() => {});
    setTimeout(() => force(undefined), 10_000).unref();
  };
  const timeout = setTimeout(() => stop(124), options.timeout * 1000);
  const onTerm = () => stop(143);
  const onInt = () => stop(130);
  process.once("SIGTERM", onTerm);
  process.once("SIGINT", onInt);
  let result;
  try {
    try {
      result = await Promise.race([session(client, options, state, plugin.version), forced]);
    } catch (error) {
      if (forcedCode !== undefined) return forcedCode;
      throw error;
    }
    if (forcedCode !== undefined) return forcedCode;
    if (result.status === "completed") {
      const finalItem = result.items?.filter((item) => item.type === "agentMessage").at(-1);
      if (finalItem) state.lastMessage = finalItem.text || "";
      await writeFile(options.output, state.lastMessage);
      console.log("[turn] completed");
      return 0;
    }
    if (result.status === "interrupted") return 130;
    console.error(result.error?.message || result.error || "turn failed");
    return 1;
  } finally {
    clearTimeout(timeout);
    process.off("SIGTERM", onTerm);
    process.off("SIGINT", onInt);
    client.close();
  }
}

let options;
try {
  options = parseArgs(process.argv.slice(2));
  if (options.prompt === "-") options.prompt = await new Promise((resolve) => {
    let input = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => { input += chunk; });
    process.stdin.on("end", () => resolve(input));
  });
} catch (error) {
  console.error(`${error.message}\n${usage}`);
  process.exitCode = 2;
}

if (options) {
  try {
    process.exitCode = await run(options);
  } catch (error) {
    console.error(error.message);
    process.exitCode = error.exitCode || 1;
  }
}
