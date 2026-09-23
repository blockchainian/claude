#!/usr/bin/env node
// ABOUTME: Checks domain availability + premium pricing via Namecheap's official domains.check API.
// ABOUTME: Reads credentials from the shared plugin config, auto-detects the caller IP, prints a table.
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const CONFIG_PATH = join(homedir(), ".config", "blockchainian", "claude.json");
const SKILL_KEY = "find-domain-names";
const ENDPOINT = "https://api.namecheap.com/xml.response";
const TLDS = ["xyz", "ai", "fun"];

// Parse a domains.check XML response into flat results, surfacing any API errors.
export function parseCheck(xml) {
  const errors = [];
  for (const m of xml.matchAll(/<Error\b[^>]*>([\s\S]*?)<\/Error>/g)) {
    const text = m[1].trim();
    if (text) errors.push(text);
  }
  const results = [];
  for (const m of xml.matchAll(/<DomainCheckResult\b([^>]*?)\/?>/g)) {
    const attrs = {};
    for (const a of m[1].matchAll(/(\w+)="([^"]*)"/g)) attrs[a[1]] = a[2];
    if (!attrs.Domain) continue;
    results.push({
      domain: attrs.Domain,
      available: attrs.Available === "true",
      premium: attrs.IsPremiumName === "true",
      regPrice: Number(attrs.PremiumRegistrationPrice || 0),
      renewPrice: Number(attrs.PremiumRenewalPrice || 0),
    });
  }
  return { errors, results };
}

// Map one result to a human status and (for priced names) a buy/renew string.
export function classify(r) {
  const price = `$${r.regPrice} buy / $${r.renewPrice}/yr`;
  if (!r.available && !r.premium) return { status: "taken", price: "" };
  if (!r.available && r.premium) return { status: "reserved-premium", price };
  if (r.available && !r.premium) return { status: "available-standard", price: "" };
  return { status: "available-premium", price };
}

export function loadCreds() {
  let raw;
  try {
    raw = readFileSync(CONFIG_PATH, "utf8");
  } catch {
    throw new Error(`No config at ${CONFIG_PATH} — run the find-domain-names setup step first.`);
  }
  const nc = JSON.parse(raw)?.[SKILL_KEY]?.namecheap;
  if (!nc?.apiUser || !nc?.apiKey) {
    throw new Error(`Missing ${SKILL_KEY}.namecheap.{apiUser,apiKey} in ${CONFIG_PATH} — run the setup step.`);
  }
  return nc;
}

async function publicIp() {
  const r = await fetch("https://api.ipify.org", { signal: AbortSignal.timeout(10000) });
  return (await r.text()).trim();
}

async function check(domains, { apiUser, apiKey }, clientIp) {
  const out = [];
  for (let i = 0; i < domains.length; i += 50) {
    const batch = domains.slice(i, i + 50);
    const url = `${ENDPOINT}?${new URLSearchParams({
      ApiUser: apiUser, ApiKey: apiKey, UserName: apiUser, ClientIp: clientIp,
      Command: "namecheap.domains.check", DomainList: batch.join(","),
    })}`;
    const xml = await fetch(url, { signal: AbortSignal.timeout(30000) }).then((r) => r.text());
    const { errors, results } = parseCheck(xml);
    if (errors.length) throw new Error(`Namecheap API: ${errors.join("; ")}`);
    out.push(...results);
  }
  return out;
}

// available-standard first, then available-premium, then reserved, then taken; alpha within.
const RANK = { "available-standard": 0, "available-premium": 1, "reserved-premium": 2, taken: 3 };

function readArgs(argv) {
  const domains = [];
  for (const a of argv) {
    if (a.includes(".")) domains.push(a);
    else for (const t of TLDS) domains.push(`${a}.${t}`); // bare word → every TLD
  }
  return domains;
}

async function main() {
  const args = process.argv.slice(2);
  if (!args.length) {
    console.error("usage: check.mjs <name|domain> ...   (bare name expands to .xyz/.ai/.fun)");
    process.exit(2);
  }
  const domains = readArgs(args);
  const creds = loadCreds();
  const ip = await publicIp();
  const rows = (await check(domains, creds, ip))
    .map((r) => ({ domain: r.domain, ...classify(r) }))
    .sort((a, b) => RANK[a.status] - RANK[b.status] || a.domain.localeCompare(b.domain));
  console.log(`${"domain".padEnd(24)} ${"status".padEnd(20)} price`);
  console.log("-".repeat(60));
  for (const r of rows) console.log(`${r.domain.padEnd(24)} ${r.status.padEnd(20)} ${r.price}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] || "").href) {
  main().catch((e) => { console.error(e.message); process.exit(1); });
}
