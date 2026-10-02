#!/usr/bin/env node
// ABOUTME: Drives one shared mitmproxy hub on port 8080 and opens app captures as named,
// ABOUTME: time-stamped views into its stream; start/check/read/stop/status/down subcommands.
/*
Drive one shared mitmproxy hub, and take app captures as views into its stream.

One long-lived mitmdump — the hub — serves the HTTP proxy (for a Mac browser via Zero Omega)
and, when asked, WireGuard (for a phone) at the same time, on the fixed port 8080. Every
session and agent shares it: the browser needs only one Zero Omega profile, pointed at 8080,
and the phone needs only one tunnel. The hub records everything routed to it into one flow
file, unfiltered — Zero Omega and WireGuard already decide what reaches it.

A capture is not a process; it is a named, time-stamped view into the hub's stream. `start`
notes the moment and the target hosts; the readers then show only that capture's window,
scoped to its hosts (and, for a shared host, its caller). So two agents capturing two apps at
once are two records over one hub, separated at read time — no second proxy, no second port,
no second Zero Omega profile.

Subcommands:
  start   ensure the hub is up and open a capture; prints its id and the 8080 address
  check   is the hub up, and is this capture's traffic arriving?
  read    show this capture's flows / websocket frames / hosts / callers
  stop    close a capture (the hub keeps running); prints its summary
  status  the hub's state and the open captures
  down    stop the hub itself; --wipe also deletes its flow file

Exit 0 on success, 1 on failure, 2 on bad arguments.
*/

import { execFileSync, spawn, spawnSync } from "node:child_process";
import {
  closeSync, existsSync, mkdirSync, openSync, readdirSync, readFileSync, realpathSync, rmSync,
  statSync, writeFileSync,
} from "node:fs";
import { createServer, createConnection } from "node:net";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DOC = `Drive one shared mitmproxy hub, and take app captures as views into its stream.

One long-lived mitmdump — the hub — serves the HTTP proxy (for a Mac browser via Zero Omega)
and, when asked, WireGuard (for a phone) at the same time, on the fixed port 8080. Every
session and agent shares it: the browser needs only one Zero Omega profile, pointed at 8080,
and the phone needs only one tunnel. The hub records everything routed to it into one flow
file, unfiltered — Zero Omega and WireGuard already decide what reaches it.

A capture is not a process; it is a named, time-stamped view into the hub's stream. \`start\`
notes the moment and the target hosts; the readers then show only that capture's window,
scoped to its hosts (and, for a shared host, its caller). So two agents capturing two apps at
once are two records over one hub, separated at read time — no second proxy, no second port,
no second Zero Omega profile.

Subcommands:
  start   ensure the hub is up and open a capture; prints its id and the 8080 address
  check   is the hub up, and is this capture's traffic arriving?
  read    show this capture's flows / websocket frames / hosts / callers
  stop    close a capture (the hub keeps running); prints its summary
  status  the hub's state and the open captures
  down    stop the hub itself; --wipe also deletes its flow file

Exit 0 on success, 1 on failure, 2 on bad arguments.`;

const SCRIPTS_DIR = dirname(fileURLToPath(import.meta.url));

export function outRoot() {
  return process.env.PROXY_DIR || "/tmp/proxy";
}

export function hubDir() {
  return join(outRoot(), "hub");
}

export function capturesDir() {
  return join(outRoot(), "captures");
}

export function capDir() {
  return join(outRoot(), "cap");
}

export function capFile(capId) {
  return join(capDir(), `${capId}.mitm`);
}

export function hubMetaPath() {
  return join(hubDir(), "meta.json");
}

export function hubLock() {
  return join(process.env.TMPDIR || "/tmp", "proxy-hub.json");
}

export const HUB_PORT = 8080;

// The default output sink; tests pass their own to capture what a command prints.
const stdio = {
  out: (s) => process.stdout.write(s),
  err: (s) => process.stderr.write(s),
};

function print(io, s) {
  io.out(s + "\n");
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function pidAlive(pid) {
  try {
    process.kill(pid, 0);
  } catch (e) {
    if (e.code === "ESRCH") return false;
    if (e.code === "EPERM") return true;
    throw e;
  }
  return true;
}

// Python's re.escape: backslash every character that could be special in a pattern. The
// regex is saved on the capture record and consumed by the Python addons too, so it is
// built the way Python would build it.
function reEscape(s) {
  return s.replace(/[()[\]{}?*+\-|^$\\.&~# \t\n\r\v\f]/g, "\\$&");
}

/**
 * Build a host-matching regex from plain domains.
 *
 * Matches the domain itself and any subdomain, with an optional :port, anchored at the end
 * so "pump.fun" does not also match "notpump.fun". Saved on a capture and used by the
 * readers to scope to that app's hosts.
 */
export function hostRegex(domains) {
  const alts = domains.filter((d) => d.trim()).map((d) => reEscape(d.trim())).join("|");
  return `(?:^|\\.)(?:${alts})(?::\\d+)?$`;
}

function canConnect(port, timeoutMs) {
  return new Promise((resolve) => {
    const c = createConnection({ host: "127.0.0.1", port });
    const done = (ok) => { c.destroy(); resolve(ok); };
    c.setTimeout(timeoutMs, () => done(false));
    c.once("connect", () => done(true));
    c.once("error", () => done(false));
  });
}

function canBind(port) {
  return new Promise((resolve) => {
    const s = createServer();
    s.once("error", () => resolve(false));
    s.listen({ port, host: "0.0.0.0" }, () => s.close(() => resolve(true)));
  });
}

/**
 * True if anything already holds this port, mirroring how mitmdump binds.
 *
 * Two checks: a connect (catches a listener on any address), and a wildcard bind with
 * SO_REUSEADDR — the same options mitmdump uses (Node sets SO_REUSEADDR on every listening
 * socket). Binding the wildcard address detects a 0.0.0.0 listener such as a running mitmweb
 * (which a bind to 127.0.0.1 misses), while SO_REUSEADDR keeps a port in TIME_WAIT — e.g.
 * just after killing the previous holder — from reading as in-use, since mitmdump can bind it.
 */
export async function portInUse(port) {
  if (await canConnect(port, 300)) return true;
  return !(await canBind(port));
}

export function readJson(path) {
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if (e.code === "ENOENT") return null;
    throw e;
  }
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function unlinkIfExists(path) {
  rmSync(path, { force: true });
}

function listFiles(dir, suffix) {
  let names;
  try {
    names = readdirSync(dir);
  } catch (e) {
    if (e.code === "ENOENT" || e.code === "ENOTDIR") return [];
    throw e;
  }
  return names.filter((n) => n.endsWith(suffix)).sort().map((n) => join(dir, n));
}

function fileSize(path) {
  try {
    return statSync(path).size;
  } catch (e) {
    if (e.code === "ENOENT") return 0;
    throw e;
  }
}

export function human(n) {
  for (const unit of ["B", "KB", "MB", "GB"]) {
    if (n < 1024) return unit === "B" ? `${n.toFixed(0)}${unit}` : `${n.toFixed(1)}${unit}`;
    n /= 1024;
  }
  return `${n.toFixed(1)}TB`;
}

// json.dumps as Python prints it: ", " and ": " separators when compact, 2-space nesting when
// indented, and non-ASCII escaped — so the JSON a caller parses is byte-for-byte what the
// Python version printed.
export function dumps(value, indent = null, level = 0) {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : "NaN";
  if (typeof value === "string") {
    return JSON.stringify(value).replace(/[^\x20-\x7e]/g,
      (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  }
  const isArray = Array.isArray(value);
  const entries = isArray ? value : Object.entries(value);
  if (entries.length === 0) return isArray ? "[]" : "{}";
  const items = entries.map((e) => (isArray
    ? dumps(e, indent, level + 1)
    : `${dumps(String(e[0]))}: ${dumps(e[1], indent, level + 1)}`));
  const [open, close] = isArray ? ["[", "]"] : ["{", "}"];
  if (indent === null) return `${open}${items.join(", ")}${close}`;
  const pad = " ".repeat(indent * (level + 1));
  const end = " ".repeat(indent * level);
  return `${open}\n${pad}${items.join(`,\n${pad}`)}\n${end}${close}`;
}

export function lanIp() {
  try {
    const iface = execFileSync("route", ["-n", "get", "default"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    const m = /interface:\s*(\S+)/.exec(iface);
    if (m) {
      const ip = execFileSync("ipconfig", ["getifaddr", m[1]], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
      if (ip) return ip;
    }
  } catch {
    // no route / ipconfig here (not macOS, or no default route): fall through
  }
  return "<your-mac-lan-ip>";
}

export function hubRunning() {
  const meta = readJson(hubMetaPath());
  if (meta && pidAlive(meta.pid ?? -1)) return meta;
  return null;
}

/**
 * Return [meta, error]. Start the hub if it is not already running.
 *
 * The hub is one mitmdump serving `regular@8080` and, if asked, `wireguard`, writing every
 * flow to the shared file. If it is already up, it is reused as-is; a WireGuard request
 * against a hub that has no WireGuard mode is reported so the caller can restart it.
 */
export async function ensureHub(wireguard) {
  let meta = hubRunning();
  if (meta) {
    if (wireguard && !(meta.modes ?? []).includes("wireguard")) {
      return [meta, "hub is running without WireGuard; run `down` then `start --wireguard`"];
    }
    return [meta, null];
  }

  if (await portInUse(HUB_PORT)) {
    return [null, `port ${HUB_PORT} is held by another process — free it, then start `
      + "(the hub needs 8080)"];
  }

  mkdirSync(hubDir(), { recursive: true });
  const logFile = join(hubDir(), "mitmdump.log");
  // connlog.py records the timestamped connection/request log (for a fast `check`);
  // dispatch.py fans each flow out into the file of every active capture it matches.
  const modes = ["regular@8080", ...(wireguard ? ["wireguard"] : [])];
  const cmd = ["mitmdump", "-q", "-s", join(SCRIPTS_DIR, "connlog.py"), "-s", join(SCRIPTS_DIR, "dispatch.py")];
  for (const m of modes) cmd.push("--mode", m);

  const log = openSync(logFile, "w");
  let proc;
  let spawnError = null;
  try {
    proc = spawn(cmd[0], cmd.slice(1), { stdio: ["ignore", log, log], detached: true });
    proc.on("error", (e) => { spawnError = e; });
    proc.unref();
  } finally {
    closeSync(log);
  }
  await sleep(700);
  const alive = !spawnError && proc.pid !== undefined && pidAlive(proc.pid);
  if (!alive || !(await portInUse(HUB_PORT))) {
    let tail = existsSync(logFile) ? readFileSync(logFile, "utf8").slice(-500) : "";
    if (spawnError) tail = `${String(spawnError.message)}\n${tail}`;
    if (alive) proc.kill("SIGTERM");
    return [null, `hub failed to start: ${tail.trim()}`];
  }

  meta = {
    pid: proc.pid, since: Date.now() / 1000, port: HUB_PORT,
    modes: ["regular", ...(wireguard ? ["wireguard"] : [])],
    log: logFile,
  };
  writeFileSync(hubMetaPath(), dumps(meta, 2));
  writeFileSync(hubLock(), dumps({ pid: proc.pid, since: meta.since }));
  return [meta, null];
}

function stamp(d) {
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

export async function cmdStart(args, io = stdio) {
  const [meta, err] = await ensureHub(args.wireguard);
  if (err && !meta) {
    print(io, dumps({ started: false, reason: err }));
    return 1;
  }

  let capId = `${stamp(new Date())}-${process.pid}`;
  if (args.label) capId = `${capId}-${args.label.replace(/[^A-Za-z0-9_-]/g, "")}`;
  const domains = (args.hosts || "").split(",").filter((d) => d.trim());
  const regex = args.hostRegex || (domains.length ? hostRegex(domains) : "");
  const record = { id: capId, label: args.label ?? null, started_at: Date.now() / 1000,
    hosts: domains, hostRegex: regex };
  mkdirSync(capturesDir(), { recursive: true });
  writeFileSync(join(capturesDir(), `${capId}.json`), dumps(record, 2));

  const out = { started: true, capture: capId, hosts: domains,
    proxy: `${lanIp()}:${HUB_PORT}`, proxyLocal: `127.0.0.1:${HUB_PORT}`,
    hubModes: (meta ?? {}).modes ?? [] };
  if (err) out.warning = err;
  if (((meta ?? {}).modes ?? []).includes("wireguard")) {
    out.wireguardConfigHint = "run wg-config.mjs to emit the client config and QR";
    out.endpoint = `${lanIp()}:51820`;
  }
  if (!domains.length) {
    out.note = "no --hosts: readers show every host in the capture window; pass --hosts to scope";
  }
  print(io, dumps(out, 2));
  return 0;
}

function captureRecord(capId) {
  return readJson(join(capturesDir(), `${capId}.json`));
}

/**
 * Run a reader addon over this capture's own file — already scoped by host and window,
 * so a read touches only this app's data, not the whole hub.
 */
function readerOutput(record, addon, extra) {
  const f = capFile(record.id);
  if (!existsSync(f)) return "";
  const cmd = ["mitmdump", "-q", "-nr", f, "-s", join(SCRIPTS_DIR, addon), ...extra];
  const r = spawnSync(cmd[0], cmd.slice(1), { encoding: "utf8" });
  if (r.error) throw r.error;
  return r.stdout;
}

// str.split(None, maxsplit) as Python does it: split on runs of whitespace, at most `max`
// times, with the remainder kept whole.
function pySplit(line, max) {
  const parts = [];
  let rest = line.replace(/^\s+/, "");
  while (rest && parts.length < max) {
    const m = /\s+/.exec(rest);
    if (!m) break;
    parts.push(rest.slice(0, m.index));
    rest = rest.slice(m.index + m[0].length);
  }
  if (rest) parts.push(rest);
  return parts;
}

// float(s) as Python does it: a decimal number or nothing, never "", "0x10" or "1,000".
function pyFloat(s) {
  const t = s.trim();
  if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(t)) return NaN;
  return Number(t);
}

/**
 * Count this capture's client connections, requests, and TLS failures from the hub log.
 *
 * connlog.py writes a timestamped line per client connection and per request host, so a
 * capture's window (start time) and hosts are counted straight from the log — no re-parse of
 * the whole shared flow file, whatever its size.
 */
export function logCounts(record) {
  const since = Number(record.started_at || 0) || 0;
  const rx = record.hostRegex || "";
  let text;
  try {
    text = readFileSync(join(hubDir(), "mitmdump.log"), "utf8");
  } catch (e) {
    if (e.code === "ENOENT") return [0, 0, 0];
    throw e;
  }
  const re = rx ? new RegExp(rx) : null;
  let clients = 0, requests = 0, tlsFailed = 0;
  for (const line of text.split(/\r\n|\r|\n/)) {
    const parts = pySplit(line, 2);
    if (parts.length >= 2 && parts[0] === "PROXY_CLIENT_CONNECTED") {
      const t = pyFloat(parts[1]);
      if (!Number.isNaN(t) && t >= since) clients += 1;
    } else if (parts.length === 3 && (parts[0] === "PROXY_REQUEST" || parts[0] === "PROXY_TLS_FAILED")) {
      const t = pyFloat(parts[1]);
      if (Number.isNaN(t)) continue;
      const fresh = t >= since;
      if (fresh && (!re || re.test(parts[2]))) {
        if (parts[0] === "PROXY_REQUEST") requests += 1;
        else tlsFailed += 1;
      }
    }
  }
  return [clients, requests, tlsFailed];
}

export async function cmdCheck(args, io = stdio) {
  const record = captureRecord(args.capture);
  if (!record) {
    print(io, dumps({ ok: false, reason: "capture not found", capture: args.capture }));
    return 2;
  }
  const meta = hubRunning();
  if (!meta) {
    print(io, dumps({ ok: false, capture: args.capture, hubRunning: false,
      verdict: "the hub is not running — start a capture first" }));
    return 0;
  }

  const [clients, requests, tlsFailed] = logCounts(record);

  let verdict;
  if (clients === 0) {
    verdict = "nothing has connected to the hub — enable Zero Omega for the site (proxy "
      + "8080) or turn on the phone tunnel, then use the app";
  } else if (requests === 0 && tlsFailed > 0) {
    verdict = `connections are arriving but TLS is failing on ${tlsFailed} — the client `
      + "does not trust the mitmproxy CA (on the phone: install via mitm.it, then "
      + "Settings > General > About > Certificate Trust Settings), or the app pins "
      + "its certificate";
  } else if (requests === 0) {
    verdict = "the hub has traffic but none for this capture's hosts since it started — "
      + "widen --hosts, or whatever routes to the hub (Zero Omega for the browser, "
      + "the WireGuard tunnel for the phone) is not sending the app's hosts";
  } else {
    verdict = `capturing: ${requests} request(s) to this capture's hosts`;
  }
  print(io, dumps({ ok: requests > 0, capture: args.capture, hubRunning: true,
    clientsConnected: clients, requests, tlsFailed, verdict }, 2));
  return 0;
}

export const READERS = { flows: "flowlog.py", ws: "wslog.py", hosts: "hosts.py", origins: "origins.py" };

export async function cmdRead(args, io = stdio) {
  const record = captureRecord(args.capture);
  if (!record) {
    io.err(`capture not found: ${args.capture}\n`);
    return 2;
  }
  const extra = [];
  if (args.source) extra.push("--set", `source=${args.source}`);
  if (args.wsmax) extra.push("--set", `wsmax=${args.wsmax}`);
  io.out(readerOutput(record, READERS[args.kind], extra));
  return 0;
}

export async function cmdStop(args, io = stdio) {
  const path = join(capturesDir(), `${args.capture}.json`);
  const record = readJson(path);
  if (!record) {
    print(io, dumps({ stopped: false, reason: "capture not found", capture: args.capture }));
    return 2;
  }
  const [, n] = logCounts(record);
  unlinkIfExists(path);
  let wiped = false;
  if (args.wipe) {
    unlinkIfExists(capFile(args.capture));
    wiped = true;
  }
  print(io, dumps({ stopped: true, capture: args.capture, requests: n, wiped,
    note: "the hub keeps running; use `down` to stop it" }, 2));
  return 0;
}

export async function cmdStatus(_args, io = stdio) {
  const meta = hubRunning();
  const captures = [];
  for (const p of listFiles(capturesDir(), ".json")) {
    const r = readJson(p);
    if (r) {
      const size = fileSize(capFile(r.id));
      captures.push({ capture: r.id ?? null, label: r.label ?? null, hosts: r.hosts ?? null,
        bytes: size, human: human(size) });
    }
  }
  print(io, dumps({
    hubRunning: meta !== null,
    hubModes: (meta ?? {}).modes ?? [],
    captures,
  }, 2));
  return 0;
}

export async function cmdDown(args, io = stdio) {
  const meta = readJson(hubMetaPath());
  const pid = (meta ?? {}).pid ?? -1;
  if (pidAlive(pid)) {
    try {
      process.kill(pid, "SIGTERM");
    } catch (e) {
      if (e.code !== "ESRCH") throw e;
    }
    for (let i = 0; i < 30; i++) {
      if (!pidAlive(pid)) break;
      await sleep(100);
    }
    if (pidAlive(pid)) process.kill(pid, "SIGKILL");
  }
  unlinkIfExists(hubMetaPath());
  unlinkIfExists(hubLock());
  let wiped = false;
  if (args.wipe) {
    for (const f of listFiles(capDir(), ".mitm")) rmSync(f);
    for (const p of listFiles(capturesDir(), ".json")) rmSync(p);
    wiped = true;
  }
  print(io, dumps({ down: true, wiped }, 2));
  return 0;
}

// --- argument parsing: the same subcommands, flags, choices and exit codes as argparse ---

const PROG = basename(process.argv[1] || "capture.mjs");

const SUBCOMMANDS = {
  start: {
    help: "ensure the hub is up and open a capture",
    options: {
      "--label": { dest: "label", help: "short name for the capture" },
      "--hosts": { dest: "hosts", help: "comma-separated target domains to scope the readers to" },
      "--host-regex": { dest: "hostRegex", help: "raw host regex, overrides --hosts" },
      "--wireguard": { dest: "wireguard", flag: true, help: "also serve WireGuard for a phone" },
    },
    positionals: [],
    func: cmdStart,
  },
  check: {
    help: "is this capture's traffic arriving?",
    options: {},
    positionals: [{ dest: "capture", help: "capture id from start" }],
    func: cmdCheck,
  },
  read: {
    help: "show this capture's flows/ws/hosts/callers",
    options: {
      "--kind": { dest: "kind", choices: Object.keys(READERS), default: "flows" },
      "--source": { dest: "source", help: "origins: pull one caller (Origin/Referer/app-id substring)" },
      "--wsmax": { dest: "wsmax", type: "int", help: "ws: max chars per frame" },
    },
    positionals: [{ dest: "capture", help: "capture id from start" }],
    func: cmdRead,
  },
  stop: {
    help: "close a capture (hub keeps running)",
    options: {
      "--wipe": { dest: "wipe", flag: true, help: "also delete this capture's flow file" },
    },
    positionals: [{ dest: "capture", help: "capture id from start" }],
    func: cmdStop,
  },
  status: { help: "hub state and open captures", options: {}, positionals: [], func: cmdStatus },
  down: {
    help: "stop the hub itself",
    options: {
      "--wipe": { dest: "wipe", flag: true, help: "also delete the hub flow file" },
    },
    positionals: [],
    func: cmdDown,
  },
};

class ArgError extends Error {}

function usageLine(name) {
  if (!name) return `usage: ${PROG} [-h] {${Object.keys(SUBCOMMANDS).join(",")}} ...`;
  const sub = SUBCOMMANDS[name];
  const opts = Object.entries(sub.options).map(([flag, o]) => (o.flag ? `[${flag}]` : `[${flag} ${o.dest.toUpperCase()}]`));
  const pos = sub.positionals.map((p) => p.dest);
  return `usage: ${PROG} ${name} [-h] ${[...opts, ...pos].join(" ")}`.trimEnd();
}

function helpText(name) {
  const lines = [usageLine(name), ""];
  if (!name) {
    lines.push(DOC, "", "positional arguments:", `  {${Object.keys(SUBCOMMANDS).join(",")}}`);
    for (const [n, s] of Object.entries(SUBCOMMANDS)) lines.push(`    ${n.padEnd(20)}${s.help}`);
    lines.push("", "options:", "  -h, --help            show this help message and exit");
    return lines.join("\n");
  }
  const sub = SUBCOMMANDS[name];
  if (sub.positionals.length) {
    lines.push("positional arguments:");
    for (const p of sub.positionals) lines.push(`  ${p.dest.padEnd(20)}${p.help ?? ""}`);
    lines.push("");
  }
  lines.push("options:", "  -h, --help            show this help message and exit");
  for (const [flag, o] of Object.entries(sub.options)) {
    const left = o.flag ? flag : (o.choices ? `${flag} {${o.choices.join(",")}}` : `${flag} ${o.dest.toUpperCase()}`);
    lines.push(`  ${left.padEnd(20)}  ${o.help ?? ""}`.trimEnd());
  }
  return lines.join("\n");
}

function resolveFlag(sub, token, name) {
  if (token in sub.options) return token;
  const matches = Object.keys(sub.options).filter((f) => f.startsWith(token));
  if (matches.length === 1) return matches[0];
  if (matches.length > 1) throw new ArgError(`ambiguous option: ${token} could match ${matches.join(", ")}`, name);
  throw new ArgError(`unrecognized arguments: ${token}`);
}

export function parseArgs(argv) {
  const subNames = Object.keys(SUBCOMMANDS);
  if (argv.length === 0) throw new ArgError("the following arguments are required: cmd");
  if (argv[0] === "-h" || argv[0] === "--help") return { help: true, cmd: null };
  const name = argv[0];
  if (!subNames.includes(name)) {
    if (name.startsWith("-")) throw new ArgError(`unrecognized arguments: ${name}`);
    throw new ArgError(`argument cmd: invalid choice: '${name}' (choose from ${subNames.map((n) => `'${n}'`).join(", ")})`);
  }
  const sub = SUBCOMMANDS[name];
  const args = { cmd: name };
  for (const o of Object.values(sub.options)) args[o.dest] = o.flag ? false : (o.default ?? null);
  const positionals = [];
  const rest = argv.slice(1);
  let onlyPositional = false;
  for (let i = 0; i < rest.length; i++) {
    let tok = rest[i];
    if (!onlyPositional && tok === "--") { onlyPositional = true; continue; }
    if (!onlyPositional && (tok === "-h" || tok === "--help")) return { help: true, cmd: name };
    if (!onlyPositional && tok.startsWith("-") && tok.length > 1) {
      let value = null;
      const eq = tok.indexOf("=");
      if (tok.startsWith("--") && eq !== -1) { value = tok.slice(eq + 1); tok = tok.slice(0, eq); }
      const flag = resolveFlag(sub, tok, name);
      const opt = sub.options[flag];
      if (opt.flag) {
        if (value !== null) throw new ArgError(`argument ${flag}: ignored explicit argument '${value}'`, name);
        args[opt.dest] = true;
        continue;
      }
      if (value === null) {
        if (i + 1 >= rest.length || (rest[i + 1].startsWith("-") && rest[i + 1].length > 1)) {
          throw new ArgError(`argument ${flag}: expected one argument`, name);
        }
        value = rest[++i];
      }
      if (opt.choices && !opt.choices.includes(value)) {
        throw new ArgError(`argument ${flag}: invalid choice: '${value}' (choose from ${opt.choices.map((c) => `'${c}'`).join(", ")})`, name);
      }
      if (opt.type === "int") {
        if (!/^\s*[+-]?\d+\s*$/.test(value)) throw new ArgError(`argument ${flag}: invalid int value: '${value}'`, name);
        value = parseInt(value, 10);
      }
      args[opt.dest] = value;
      continue;
    }
    positionals.push(tok);
  }
  if (positionals.length < sub.positionals.length) {
    const missing = sub.positionals.slice(positionals.length).map((p) => p.dest);
    throw new ArgError(`the following arguments are required: ${missing.join(", ")}`, name);
  }
  if (positionals.length > sub.positionals.length) {
    throw new ArgError(`unrecognized arguments: ${positionals.slice(sub.positionals.length).join(" ")}`);
  }
  sub.positionals.forEach((p, i) => { args[p.dest] = positionals[i]; });
  args.func = sub.func;
  return args;
}

export async function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (e) {
    if (!(e instanceof ArgError)) throw e;
    const sub = argv.length && argv[0] in SUBCOMMANDS && !e.message.startsWith("unrecognized") ? argv[0] : null;
    process.stderr.write(`${usageLine(sub)}\n${PROG}${sub ? ` ${sub}` : ""}: error: ${e.message}\n`);
    return 2;
  }
  if (args.help) {
    process.stdout.write(helpText(args.cmd) + "\n");
    return 0;
  }
  return args.func(args);
}

const invokedDirectly = (() => {
  try {
    return process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
  } catch {
    return false;
  }
})();

if (invokedDirectly) {
  main().then((code) => { process.exitCode = code; });
}
