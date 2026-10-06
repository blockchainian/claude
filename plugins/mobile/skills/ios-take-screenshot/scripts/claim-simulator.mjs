#!/usr/bin/env node
// ABOUTME: Holds one simulator or phone for one capture run via a claim file per UDID under
// ABOUTME: MOBILE_STATE_DIR, so two agents never drive the same target at once; --release drops it.
//
// Hold one simulator or phone for one capture run, so two agents never drive it at once.
//
// XcodeBuildMCP calls must explicitly name the claimed simulator; do not
// change shared session defaults. On a phone, WebDriverAgent
// serves one session, so a second Appium session ends the first one's mid-run.
// The lock is a file per UDID under $MOBILE_STATE_DIR/locks, which every session of the same user
// shares. A claim on a UDID another run holds exits 3 and says who holds it and
// for how long; the later agent decides whether to wait or abort. Pass --steal only for a run
// you know is dead. --release drops a claim this run holds.
//
// Prints JSON. Exit 0 claimed or released, 2 bad arguments, 3 held by another run.

import fs from "node:fs";
import os from "node:os";
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const DESCRIPTION = `Hold one simulator or phone for one capture run, so two agents never drive it at once.

XcodeBuildMCP calls must explicitly name the claimed simulator; do not
change shared session defaults. On a phone, WebDriverAgent
serves one session, so a second Appium session ends the first one's mid-run.
The lock is a file per UDID under $MOBILE_STATE_DIR/locks, which every session of the same user
shares. A claim on a UDID another run holds exits 3 and says who holds it and
for how long; the later agent decides whether to wait or abort. Pass --steal only for a run
you know is dead. --release drops a claim this run holds.

Prints JSON. Exit 0 claimed or released, 2 bad arguments, 3 held by another run.`;

const PROG = path.basename(process.argv[1] || "claim-simulator.mjs");
const USAGE = `usage: ${PROG} [-h] --run RUN [--release] [--steal] udid`;

// Serialize like Python's json.dumps: ", " and ": " separators, non-ASCII escaped.
function pyDumps(value) {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return JSON.stringify(value);
  if (typeof value === "string") {
    return JSON.stringify(value).replace(/[\u0080-\uffff]/g,
      (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  }
  if (Array.isArray(value)) return "[" + value.map(pyDumps).join(", ") + "]";
  return "{" + Object.entries(value).map(([k, v]) => `${pyDumps(k)}: ${pyDumps(v)}`).join(", ") + "}";
}

// Normalize like pathlib.PurePosixPath: collapse "//" and ".", keep "..", drop trailing "/".
function pyPath(p) {
  const absolute = p.startsWith("/");
  const parts = p.split("/").filter((s) => s !== "" && s !== ".");
  const joined = parts.join("/");
  if (absolute) return "/" + joined;
  return joined || ".";
}

function usageError(message) {
  process.stderr.write(`${USAGE}\n${PROG}: error: ${message}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const flags = {
    "--run": { value: true, dest: "run" },
    "--release": { value: false, dest: "release" },
    "--steal": { value: false, dest: "steal" },
    "--help": { value: false, dest: "help" },
  };
  const args = { udid: undefined, run: undefined, release: false, steal: false };
  const positionals = [];
  let onlyPositionals = false;
  for (let i = 0; i < argv.length; i++) {
    let arg = argv[i];
    if (onlyPositionals || !arg.startsWith("-") || arg === "-") {
      positionals.push(arg);
      continue;
    }
    if (arg === "--") { onlyPositionals = true; continue; }
    if (arg === "-h") arg = "--help";
    let inline;
    const eq = arg.indexOf("=");
    if (arg.startsWith("--") && eq > 0) { inline = arg.slice(eq + 1); arg = arg.slice(0, eq); }
    let name = flags[arg] ? arg : undefined;
    if (!name) {
      const matches = Object.keys(flags).filter((f) => f.startsWith(arg));
      if (matches.length === 1) name = matches[0];
      else if (matches.length > 1) usageError(`ambiguous option: ${arg} could match ${matches.join(", ")}`);
      else usageError(`unrecognized arguments: ${arg}`);
    }
    const spec = flags[name];
    if (name === "--help") {
      process.stdout.write(`${USAGE}\n\n${DESCRIPTION}\n\npositional arguments:\n  udid       simulator or phone UDID; never "booted"\n\noptions:\n  -h, --help show this help message and exit\n  --run RUN  RUN_ID of the capture run\n  --release  drop this run's claim\n  --steal    take the UDID from a run that is known to be dead\n`);
      process.exit(0);
    }
    if (spec.value) {
      if (inline !== undefined) args[spec.dest] = inline;
      else if (i + 1 < argv.length && (!argv[i + 1].startsWith("-") || argv[i + 1] === "-")) args[spec.dest] = argv[++i];
      else usageError(`argument ${name}: expected one argument`);
    } else {
      if (inline !== undefined) usageError(`argument ${name}: ignored explicit argument '${inline}'`);
      args[spec.dest] = true;
    }
  }
  if (positionals.length > 1) usageError(`unrecognized arguments: ${positionals.slice(1).join(" ")}`);
  const missing = [];
  if (positionals.length === 0) missing.push("udid");
  if (args.run === undefined) missing.push("--run");
  if (missing.length) usageError(`the following arguments are required: ${missing.join(", ")}`);
  args.udid = positionals[0];
  return args;
}

// The claims are state: deleting one mid-run lets a second agent drive the same target.
export function lockPath(udid) {
  const dir = `${process.env.MOBILE_STATE_DIR || `${os.homedir()}/.local/state/mobile`}/locks`;
  fs.mkdirSync(dir, { recursive: true });
  return pyPath(`${dir}/ios-screenshot-lock.${udid}.json`);
}

export function readClaim(lockFile) {
  let text;
  try {
    text = fs.readFileSync(lockFile, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
  const held = JSON.parse(text);
  if (!held || typeof held.run !== "string" || !held.run) {
    throw new Error(`Invalid device claim: ${lockFile}`);
  }
  return held;
}

export function main(argv = process.argv.slice(2), underLock = false) {
  const args = parseArgs(argv);

  if (args.udid === "booted") {
    process.stderr.write('"booted" names no simulator; pass the UDID\n');
    return 2;
  }

  if (!/^[A-Za-z0-9-]+$/.test(args.udid) || !args.run) {
    usageError("pass an explicit device UDID and a nonempty run id");
  }
  const lockFile = lockPath(args.udid);
  if (!underLock) {
    const helper = fileURLToPath(new URL("../../../hooks/claim_state.py", import.meta.url));
    const entry = `import { main } from ${JSON.stringify(import.meta.url)}; process.exitCode = main(JSON.parse(process.argv[1]), true);`;
    const result = spawnSync("uv", ["run", "--quiet", "--script", helper, lockFile,
      process.execPath, "--input-type=module", "-e", entry, JSON.stringify(argv)], { stdio: "inherit" });
    if (result.error) throw result.error;
    if (result.signal) throw new Error(`Device claim interrupted by ${result.signal}`);
    return result.status;
  }
  const held = readClaim(lockFile);
  const other = held !== null && held.run !== args.run;
  const now = Date.now() / 1000;

  if (held && (held.session || held.pending) && args.release) {
    process.stderr.write(`${args.udid} has an Appium session or create in progress; delete the session before releasing the claim.\n`);
    return 3;
  }

  if (args.release) {
    if (other) {
      process.stdout.write(pyDumps({ released: false, udid: args.udid, heldBy: held.run }) + "\n");
      process.stderr.write(`${args.udid} is held by run ${held.run}, not ${args.run}\n`);
      return 3;
    }
    try {
      fs.unlinkSync(lockFile);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    process.stdout.write(pyDumps({ released: true, udid: args.udid }) + "\n");
    return 0;
  }

  if (other && !args.steal) {
    const since = held.since === undefined ? now : held.since;
    const age = Math.trunc(now - since);
    process.stdout.write(pyDumps({
      claimed: false, udid: args.udid, heldBy: held.run,
      since: held.since === undefined ? null : held.since, ageSeconds: age,
      lock: lockFile,
    }) + "\n");
    process.stderr.write(`${args.udid} is in use by run ${held.run} for ${age}s. `
      + "Wait and claim again when it is released, or abort and pick another "
      + "target. Only if that run is known to be dead: --steal.\n");
    return 3;
  }

  if (held === null || other || args.steal) {
    const temporary = `${lockFile}.${randomUUID()}.tmp`;
    try {
      fs.writeFileSync(temporary, pyDumps({ run: args.run, since: now, pid: process.pid }), { flag: "wx" });
      fs.renameSync(temporary, lockFile);
    } finally {
      fs.rmSync(temporary, { force: true });
    }
  }
  process.stdout.write(pyDumps({
    claimed: true, udid: args.udid, heldBy: args.run,
    stolenFrom: other ? held.run : null, lock: lockFile,
  }) + "\n");
  return 0;
}

if (process.argv[1] && fs.existsSync(process.argv[1]) && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
