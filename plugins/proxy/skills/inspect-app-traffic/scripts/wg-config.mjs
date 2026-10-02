#!/usr/bin/env node
// ABOUTME: Emits the WireGuard client config (and a QR via qrencode) for a mitmproxy
// ABOUTME: wireguard-mode capture, deriving the server public key with node:crypto X25519.
/*
Emit the WireGuard client config (and a QR) for a mitmproxy wireguard-mode capture.

mitmproxy writes ~/.mitmproxy/wireguard.conf with a server_key and a client_key when it
first starts in wireguard mode. This turns those into a config the phone's WireGuard app
imports: the client_key is the phone's PrivateKey, the server's public key is derived from
server_key, AllowedIPs is 0.0.0.0/0 so the whole phone routes through the tunnel, and the
Endpoint is this Mac on the LAN.

The X25519 public-key derivation is done here with node:crypto on purpose: it needs no npm
package and the `wg` tool is not always installed, and one small, dependency-free routine is
simpler than requiring either. AllowedIPs of 0.0.0.0/0 means the phone sends ALL its traffic
here while the tunnel is on — pair it with a host filter on the capture so unrelated traffic
is passed through and never saved, and tell the user to turn the tunnel off when done.

Prints the config text on stdout. With --qr and qrencode installed, also writes a PNG.
*/

import { execFileSync, spawnSync } from "node:child_process";
import { createPrivateKey, createPublicKey } from "node:crypto";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";

const DOC = `Emit the WireGuard client config (and a QR) for a mitmproxy wireguard-mode capture.

mitmproxy writes ~/.mitmproxy/wireguard.conf with a server_key and a client_key when it
first starts in wireguard mode. This turns those into a config the phone's WireGuard app
imports: the client_key is the phone's PrivateKey, the server's public key is derived from
server_key, AllowedIPs is 0.0.0.0/0 so the whole phone routes through the tunnel, and the
Endpoint is this Mac on the LAN.

Prints the config text on stdout. With --qr and qrencode installed, also writes a PNG.`;

// DER prefix of a PKCS#8 X25519 private key: the 32 raw key bytes follow it.
const PKCS8_X25519_PREFIX = Buffer.from("302e020100300506032b656e04220420", "hex");
// DER prefix of a SubjectPublicKeyInfo X25519 public key: the 32 raw key bytes follow it.
const SPKI_X25519_PREFIX = Buffer.from("302a300506032b656e032100", "hex");

/** RFC 7748 X25519 base-point multiplication: the raw 32-byte public key of a raw private key. */
export function x25519PublicKey(privateRaw) {
  if (privateRaw.length !== 32) throw new Error(`X25519 private key must be 32 bytes, got ${privateRaw.length}`);
  const priv = createPrivateKey({
    key: Buffer.concat([PKCS8_X25519_PREFIX, privateRaw]), format: "der", type: "pkcs8",
  });
  const spki = createPublicKey(priv).export({ format: "der", type: "spki" });
  if (!spki.subarray(0, SPKI_X25519_PREFIX.length).equals(SPKI_X25519_PREFIX)) {
    throw new Error("unexpected X25519 public key encoding");
  }
  return spki.subarray(SPKI_X25519_PREFIX.length);
}

export function publicKey(privateB64) {
  const priv = Buffer.from(privateB64, "base64");
  return x25519PublicKey(priv).toString("base64");
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

export function buildConfig(confPath, endpoint) {
  const data = JSON.parse(readFileSync(confPath, "utf8"));
  const clientPriv = data.client_key;
  const serverPub = publicKey(data.server_key);
  return [
    "[Interface]",
    `PrivateKey = ${clientPriv}`,
    "Address = 10.0.0.1/32",
    "DNS = 10.0.0.53",
    "",
    "[Peer]",
    `PublicKey = ${serverPub}`,
    "AllowedIPs = 0.0.0.0/0, ::/0",
    `Endpoint = ${endpoint}`,
    "",
  ].join("\n");
}

function expandUser(p) {
  if (p === "~") return homedir();
  if (p.startsWith("~/")) return join(homedir(), p.slice(2));
  return p;
}

function defaultConf() {
  return expandUser((process.env.MITMPROXY_CONFDIR ?? "~/.mitmproxy") + "/wireguard.conf");
}

const PROG = basename(process.argv[1] || "wg-config.mjs");
const OPTIONS = {
  "--conf": { dest: "conf", help: "path to mitmproxy's wireguard.conf" },
  "--endpoint": { dest: "endpoint", help: "host:port the phone connects to; default is this Mac's LAN IP on 51820" },
  "--qr": { dest: "qr", help: "write a QR PNG to this path" },
};

class ArgError extends Error {}

function usageLine() {
  return `usage: ${PROG} [-h] [--conf CONF] [--endpoint ENDPOINT] [--qr QR]`;
}

function helpText() {
  const lines = [usageLine(), "", DOC, "", "options:", "  -h, --help            show this help message and exit"];
  for (const [flag, o] of Object.entries(OPTIONS)) {
    lines.push(`  ${`${flag} ${o.dest.toUpperCase()}`.padEnd(20)}  ${o.help}`);
  }
  return lines.join("\n");
}

export function parseArgs(argv) {
  const args = { conf: defaultConf(), endpoint: null, qr: null };
  for (let i = 0; i < argv.length; i++) {
    let tok = argv[i];
    if (tok === "-h" || tok === "--help") return { help: true };
    let value = null;
    const eq = tok.indexOf("=");
    if (tok.startsWith("--") && eq !== -1) { value = tok.slice(eq + 1); tok = tok.slice(0, eq); }
    let flag = tok in OPTIONS ? tok : null;
    if (!flag && tok.startsWith("--")) {
      const matches = Object.keys(OPTIONS).filter((f) => f.startsWith(tok));
      if (matches.length === 1) flag = matches[0];
      else if (matches.length > 1) throw new ArgError(`ambiguous option: ${tok} could match ${matches.join(", ")}`);
    }
    if (!flag) throw new ArgError(`unrecognized arguments: ${argv.slice(i).join(" ")}`);
    if (value === null) {
      if (i + 1 >= argv.length || (argv[i + 1].startsWith("-") && argv[i + 1].length > 1)) {
        throw new ArgError(`argument ${flag}: expected one argument`);
      }
      value = argv[++i];
    }
    args[OPTIONS[flag].dest] = value;
  }
  return args;
}

export function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (e) {
    if (!(e instanceof ArgError)) throw e;
    process.stderr.write(`${usageLine()}\n${PROG}: error: ${e.message}\n`);
    return 2;
  }
  if (args.help) {
    process.stdout.write(helpText() + "\n");
    return 0;
  }

  const conf = args.conf;
  if (!existsSync(conf)) {
    process.stderr.write(`${conf} not found — start a wireguard-mode capture first so mitmproxy writes it\n`);
    return 1;
  }

  const endpoint = args.endpoint || `${lanIp()}:51820`;
  const config = buildConfig(conf, endpoint);
  process.stdout.write(config + "\n");

  if (args.qr) {
    const r = spawnSync("qrencode", ["-t", "PNG", "-s", "12", "-m", "4", "-l", "L", "-o", args.qr],
      { input: config, stdio: ["pipe", "inherit", "inherit"] });
    if (!r.error && r.status === 0) {
      process.stderr.write(`# QR written to ${args.qr}\n`);
    } else {
      process.stderr.write("# qrencode unavailable; import the text config manually "
        + "(brew install qrencode for a QR)\n");
    }
  }
  return 0;
}

const invokedDirectly = (() => {
  try {
    return process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
  } catch {
    return false;
  }
})();

if (invokedDirectly) {
  process.exitCode = main();
}
