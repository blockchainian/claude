#!/usr/bin/env node
// ABOUTME: Reports what an iPhone or a simulator needs before capture, discovering the UDID,
// ABOUTME: team id and WebDriverAgent bundle id itself; exit 0 ready, exit 1 lists what is missing.
//
// Report what an iPhone or a simulator needs before capture, discovering what it can.
//
// A real device drives through Appium, so most session capabilities can be found
// rather than asked for or remembered: the UDID comes from the device list, the
// team id from a provisioning profile, and the WebDriverAgent bundle id from the
// runner already installed on the device. Prints a suggestedCapabilities object
// ready for appium_session_management (action=create).
//
// A simulator drives through XcodeBuildMCP and needs none of that — only a booted
// simulator. Pass --target simulator for that report.
//
// Either way: exit 0 means ready, exit 1 lists what is missing.

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const DESCRIPTION = `Report what an iPhone or a simulator needs before capture, discovering what it can.

A real device drives through Appium, so most session capabilities can be found
rather than asked for or remembered: the UDID comes from the device list, the
team id from a provisioning profile, and the WebDriverAgent bundle id from the
runner already installed on the device. Prints a suggestedCapabilities object
ready for appium_session_management (action=create).

A simulator drives through XcodeBuildMCP and needs none of that — only a booted
simulator. Pass --target simulator for that report.

Either way: exit 0 means ready, exit 1 lists what is missing.`;

const HOME = os.homedir();
const PROFILE_DIRS = [
  path.join(HOME, "Library/Developer/Xcode/UserData/Provisioning Profiles"), // Xcode 16+
  path.join(HOME, "Library/MobileDevice/Provisioning Profiles"), // legacy
];
const SIGNED_WDA_GLOB = ".cache/appium-mcp/wda-real/*/signed/*/Payload-resigned.ipa";
const PREBUILT_WDA = path.join(HOME, ".appium/wda-dd");
const DEFAULT_WDA_BUNDLE = "com.facebook.WebDriverAgentRunner";

const PROG = path.basename(process.argv[1] || "discover-ios-setup.mjs");
const USAGE = `usage: ${PROG} [-h] [--device DEVICE] [--target {device,simulator}]`;

// Serialize like Python's json.dumps(obj, indent=2): non-ASCII escaped, "[]"/"{}" for empties.
function pyDumps(value, indent = 2) {
  return JSON.stringify(value, null, indent).replace(/[\u0080-\uffff]/g,
    (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}

function isPlainObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isDir(p) {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

// Expand a pattern whose "*" segments stand for one directory entry each (no "**"),
// matching dotfiles too, like pathlib's glob does.
function globFixed(base, pattern) {
  let matches = [base];
  for (const segment of pattern.split("/")) {
    const next = [];
    for (const dir of matches) {
      if (segment.includes("*")) {
        const re = new RegExp("^" + segment.split("*").map((s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
        let entries = [];
        try {
          entries = fs.readdirSync(dir);
        } catch {
          continue;
        }
        for (const entry of entries) {
          if (re.test(entry)) next.push(path.join(dir, entry));
        }
      } else {
        const candidate = path.join(dir, segment);
        if (fs.existsSync(candidate)) next.push(candidate);
      }
    }
    matches = next;
  }
  return matches;
}

// Minimal XML plist reader: enough for a decoded provisioning profile.
function decodeEntities(text) {
  return text.replace(/&(amp|lt|gt|quot|apos|#x[0-9a-fA-F]+|#\d+);/g, (m, e) => {
    if (e === "amp") return "&";
    if (e === "lt") return "<";
    if (e === "gt") return ">";
    if (e === "quot") return '"';
    if (e === "apos") return "'";
    if (e.startsWith("#x")) return String.fromCodePoint(parseInt(e.slice(2), 16));
    return String.fromCodePoint(parseInt(e.slice(1), 10));
  });
}

export function parsePlist(text) {
  const tokens = [];
  const tagRe = /<\?[\s\S]*?\?>|<!--[\s\S]*?-->|<!DOCTYPE[^>]*>|<\/?([A-Za-z]+)([^>]*?)(\/?)>/g;
  let last = 0;
  let m;
  while ((m = tagRe.exec(text)) !== null) {
    const between = text.slice(last, m.index);
    if (between.trim()) tokens.push({ kind: "text", value: between });
    last = tagRe.lastIndex;
    if (!m[1]) continue;
    if (m[0].startsWith("</")) tokens.push({ kind: "close", name: m[1] });
    else tokens.push({ kind: "open", name: m[1], selfClosing: m[3] === "/" });
  }
  let pos = 0;
  const peek = () => tokens[pos];
  const expect = (kind, name) => {
    const t = tokens[pos++];
    if (!t || t.kind !== kind || (name && t.name !== name)) throw new Error("invalid plist");
    return t;
  };
  const readText = (name) => {
    let value = "";
    while (peek() && peek().kind === "text") value += tokens[pos++].value;
    expect("close", name);
    return value;
  };
  function parseValue() {
    const t = expect("open");
    switch (t.name) {
      case "dict": {
        const out = {};
        if (t.selfClosing) return out;
        while (peek() && !(peek().kind === "close" && peek().name === "dict")) {
          expect("open", "key");
          const key = decodeEntities(readText("key"));
          out[key] = parseValue();
        }
        expect("close", "dict");
        return out;
      }
      case "array": {
        const out = [];
        if (t.selfClosing) return out;
        while (peek() && !(peek().kind === "close" && peek().name === "array")) out.push(parseValue());
        expect("close", "array");
        return out;
      }
      case "string": return t.selfClosing ? "" : decodeEntities(readText("string"));
      case "integer": return t.selfClosing ? 0 : parseInt(readText("integer").trim(), 10);
      case "real": return t.selfClosing ? 0 : parseFloat(readText("real").trim());
      case "date": return t.selfClosing ? null : new Date(readText("date").trim());
      case "data": return t.selfClosing ? Buffer.alloc(0) : Buffer.from(readText("data").replace(/\s+/g, ""), "base64");
      case "true": return true;
      case "false": return false;
      default: throw new Error(`invalid plist element: ${t.name}`);
    }
  }
  expect("open", "plist");
  const value = parseValue();
  expect("close", "plist");
  return value;
}

function run(cmd, args, options = {}) {
  const proc = spawnSync(cmd, args, { encoding: "utf8", ...options });
  // A missing executable counts as a failed command rather than a crash.
  if (proc.error) return { status: proc.error.code === "ENOENT" ? 127 : 1, stdout: "", stderr: String(proc.error.message) };
  return { status: proc.status === null ? 1 : proc.status, stdout: proc.stdout || "", stderr: proc.stderr || "" };
}

export function devicectl(...args) {
  const out = path.join(os.tmpdir(), `tmp${crypto.randomBytes(6).toString("hex")}.json`);
  fs.writeFileSync(out, "");
  try {
    const proc = run("xcrun", ["devicectl", ...args, "--json-output", out]);
    if (proc.status !== 0) return {};
    try {
      return JSON.parse(fs.readFileSync(out, "utf8"));
    } catch (error) {
      if (error instanceof SyntaxError) return {};
      throw error;
    }
  } finally {
    fs.rmSync(out, { force: true });
  }
}

export function listDevices() {
  const data = deps.devicectl("list", "devices");
  const devices = [];
  for (const d of (data.result || {}).devices || []) {
    const hw = d.hardwareProperties || {};
    const props = d.deviceProperties || {};
    const conn = d.connectionProperties || {};
    if (conn.tunnelState === "unavailable") continue;
    devices.push({
      udid: hw.udid ?? null,
      name: props.name ?? null,
      osVersion: props.osVersionNumber ?? null,
      transport: conn.transportType ?? null,
      developerModeEnabled: props.developerModeStatus === "enabled",
    });
  }
  return devices;
}

// Booted simulators, newest runtime first.
//
// A shut-down simulator is not an error worth reporting in detail: booting one
// is a single command, and capture cannot use it until it is booted anyway.
export function listSimulators() {
  const proc = run("xcrun", ["simctl", "list", "devices", "booted", "--json"]);
  if (proc.status !== 0) return [];
  let data;
  try {
    data = JSON.parse(proc.stdout);
  } catch (error) {
    if (error instanceof SyntaxError) return [];
    throw error;
  }

  const sims = [];
  for (const [runtime, devices] of Object.entries(data.devices || {})) {
    const tail = runtime.slice(runtime.lastIndexOf(".") + 1);
    const version = (tail.startsWith("iOS-") ? tail.slice(4) : tail).replace(/-/g, ".");
    for (const d of devices) {
      sims.push({
        udid: d.udid ?? null,
        name: d.name ?? null,
        osVersion: version,
        state: d.state ?? null,
      });
    }
  }
  return sims;
}

// Swappable collaborators, so tests can stub discovery without Xcode.
export const deps = { devicectl, listSimulators };

export function simulatorReport(requested) {
  const sims = deps.listSimulators();
  const sim = requested
    ? (sims.find((s) => s.udid === requested) ?? null)
    : (sims.length === 1 ? sims[0] : null);

  const report = { target: "simulator", simulators: sims, selectedSimulator: sim, ready: sim !== null };
  if (sim) {
    // Preserve the existing report shape for callers using session defaults.
    // This plugin passes simulatorId explicitly on each tool call instead.
    report.sessionDefaults = { simulatorId: sim.udid, simulatorName: sim.name };
  } else if (requested) {
    report.missing = [`simulator ${requested} is not booted; boot it with `
      + `\`xcrun simctl boot ${requested}\` or pick a booted one`];
  } else if (sims.length === 0) {
    report.missing = ["no simulator booted; boot one with `xcrun simctl boot <udid>`"];
  } else {
    report.missing = [`${sims.length} simulators booted; pass --device with one of their UDIDs`];
  }
  return report;
}

// The installed WebDriverAgent runner, if any.
//
// Appium wants the bundle id without the .xctrunner suffix that Xcode appends.
export function wdaBundleId(udid) {
  const data = deps.devicectl("device", "info", "apps", "--device", udid, "--include-all-apps");
  const apps = (data.result || {}).apps || [];
  for (const app of apps) {
    const bundle = app.bundleIdentifier || "";
    if (bundle.endsWith(".xctrunner") && bundle.toLowerCase().replace(/-/g, "").includes("webdriveragent")) {
      return bundle.slice(0, -".xctrunner".length);
    }
  }
  for (const app of apps) {
    const bundle = app.bundleIdentifier || "";
    if (bundle.endsWith(".xctrunner")) return bundle.slice(0, -".xctrunner".length);
  }
  return null;
}

export function profiles(udid) {
  const found = [];
  for (const directory of PROFILE_DIRS) {
    if (!isDir(directory)) continue;
    const names = fs.readdirSync(directory)
      .filter((n) => n.endsWith(".mobileprovision"))
      .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    for (const name of names) {
      const file = path.join(directory, name);
      const raw = run("security", ["cms", "-D", "-i", file]).stdout;
      let pl;
      try {
        pl = parsePlist(raw);
        if (!isPlainObject(pl)) throw new Error("not a dict");
      } catch {
        continue;
      }
      const expires = pl.ExpirationDate;
      const days = expires instanceof Date ? Math.floor((expires.getTime() - Date.now()) / 86400000) : null;
      const devices = pl.ProvisionedDevices || [];
      const entitlements = isPlainObject(pl.Entitlements) ? pl.Entitlements : {};
      const appId = entitlements["application-identifier"];
      found.push({
        name: pl.Name ?? null,
        teamId: (Array.isArray(pl.TeamIdentifier) && pl.TeamIdentifier.length ? pl.TeamIdentifier : [null])[0] ?? null,
        appId: appId ?? null,
        expiresInDays: days,
        coversDevice: Boolean(udid && Array.isArray(devices) && devices.includes(udid)),
        wildcard: String(appId ?? "").endsWith(".*"),
      });
    }
  }
  return found;
}

// The newest WebDriverAgent that appium_prepare_ios_real_device signed.
//
// Preferred over a hand-built one: the tool downloads the release matching the
// driver, so there is no driver/WDA version skew to debug.
export function signedWdaIpa() {
  const found = globFixed(HOME, SIGNED_WDA_GLOB)
    .map((p) => ({ p, mtime: fs.statSync(p).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime);
  return found.length ? found[0].p : null;
}

function usageError(message) {
  process.stderr.write(`${USAGE}\n${PROG}: error: ${message}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const flags = { "--device": "device", "--target": "target", "--help": "help" };
  const args = { device: undefined, target: "device" };
  const positionals = [];
  let onlyPositionals = false;
  for (let i = 0; i < argv.length; i++) {
    let arg = argv[i];
    if (onlyPositionals || !arg.startsWith("-") || arg === "-") { positionals.push(arg); continue; }
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
    if (name === "--help") {
      process.stdout.write(`${USAGE}\n\n${DESCRIPTION}\n\noptions:\n  -h, --help            show this help message and exit\n  --device DEVICE       UDID; defaults to the only connected device or the only booted simulator\n  --target {device,simulator}\n                        what capture will drive (default: device)\n`);
      process.exit(0);
    }
    let value;
    if (inline !== undefined) value = inline;
    else if (i + 1 < argv.length && (!argv[i + 1].startsWith("-") || argv[i + 1] === "-")) value = argv[++i];
    else usageError(`argument ${name}: expected one argument`);
    if (name === "--target" && !["device", "simulator"].includes(value)) {
      usageError(`argument --target: invalid choice: '${value}' (choose from 'device', 'simulator')`);
    }
    args[flags[name]] = value;
  }
  if (positionals.length) usageError(`unrecognized arguments: ${positionals.join(" ")}`);
  return args;
}

export function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);

  if (args.target === "simulator") {
    const report = simulatorReport(args.device ?? null);
    process.stdout.write(pyDumps(report) + "\n");
    return report.ready ? 0 : 1;
  }

  const devices = listDevices();
  const udid = args.device || (devices.length === 1 ? devices[0].udid : null);
  const device = devices.find((d) => d.udid === udid) ?? null;

  const profs = profiles(udid);
  const usable = profs.filter((p) => p.coversDevice);
  const team = (usable.find((p) => p.wildcard) ?? usable[0] ?? { teamId: null }).teamId;
  const wda = udid ? wdaBundleId(udid) : null;

  const report = {
    target: "device",
    devices,
    selectedDevice: device,
    profiles: profs,
    webDriverAgent: {
      installed: Boolean(wda),
      bundleId: wda,
      signedIpa: signedWdaIpa(),
      prebuiltDerivedData: isDir(PREBUILT_WDA) ? PREBUILT_WDA : null,
    },
    ready: Boolean(udid && team && wda),
  };

  if (report.ready) {
    const caps = {
      "appium:udid": udid,
      "appium:xcodeOrgId": team,
      "appium:xcodeSigningId": "Apple Development",
      "appium:noReset": true,
    };
    if (wda !== DEFAULT_WDA_BUNDLE) caps["appium:updatedWDABundleId"] = wda;
    const signed = report.webDriverAgent.signedIpa;
    if (signed) {
      caps["appium:usePreinstalledWDA"] = true;
      caps["appium:prebuiltWDAPath"] = signed;
      caps["appium:wdaLaunchTimeout"] = 30000;
    } else if (report.webDriverAgent.prebuiltDerivedData) {
      caps["appium:usePrebuiltWDA"] = true;
      caps["appium:derivedDataPath"] = report.webDriverAgent.prebuiltDerivedData;
    }
    if (device && device.osVersion) caps["appium:platformVersion"] = device.osVersion;
    report.suggestedCapabilities = caps;
  } else {
    const missing = [];
    if (!udid) missing.push("no single connected device; pass --device");
    if (!team) missing.push("no provisioning profile covering this device");
    if (!wda) missing.push("WebDriverAgent is not installed; run appium_prepare_ios_real_device");
    report.missing = missing;
  }

  process.stdout.write(pyDumps(report) + "\n");
  return report.ready ? 0 : 1;
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
