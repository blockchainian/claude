// ABOUTME: Tests the domain checker's XML parsing and status classification on a fixed
// ABOUTME: Namecheap domains.check response — no network, real parse logic against a fixture.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseCheck, classify, loadCreds } from "../scripts/check.mjs";

const ok = `<?xml version="1.0" encoding="utf-8"?>
<ApiResponse Status="OK" xmlns="http://api.namecheap.com/xml.response">
  <Errors />
  <CommandResponse Type="namecheap.domains.check">
    <DomainCheckResult Domain="trovy.xyz" Available="false" IsPremiumName="false" PremiumRegistrationPrice="0" PremiumRenewalPrice="0" />
    <DomainCheckResult Domain="cache.xyz" Available="false" IsPremiumName="true" PremiumRegistrationPrice="650.0000" PremiumRenewalPrice="650.0000" />
    <DomainCheckResult Domain="goldfinger.fun" Available="true" IsPremiumName="false" PremiumRegistrationPrice="0" PremiumRenewalPrice="0" />
    <DomainCheckResult Domain="midas.fun" Available="true" IsPremiumName="true" PremiumRegistrationPrice="1625.0000" PremiumRenewalPrice="6500.0000" />
  </CommandResponse>
</ApiResponse>`;

const err = `<?xml version="1.0" encoding="utf-8"?>
<ApiResponse Status="ERROR" xmlns="http://api.namecheap.com/xml.response">
  <Errors><Error Number="1011150">API Key is invalid or API access has not been enabled</Error></Errors>
  <CommandResponse />
</ApiResponse>`;

test("parseCheck reads every result with its flags and prices", () => {
  const { errors, results } = parseCheck(ok);
  assert.deepEqual(errors, []);
  assert.equal(results.length, 4);
  const byDomain = Object.fromEntries(results.map((r) => [r.domain, r]));
  assert.deepEqual(byDomain["trovy.xyz"], { domain: "trovy.xyz", available: false, premium: false, regPrice: 0, renewPrice: 0 });
  assert.deepEqual(byDomain["cache.xyz"], { domain: "cache.xyz", available: false, premium: true, regPrice: 650, renewPrice: 650 });
  assert.equal(byDomain["goldfinger.fun"].available, true);
  assert.equal(byDomain["midas.fun"].regPrice, 1625);
  assert.equal(byDomain["midas.fun"].renewPrice, 6500);
});

test("parseCheck surfaces API errors", () => {
  const { errors, results } = parseCheck(err);
  assert.equal(results.length, 0);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /API Key is invalid/);
});

test("classify maps the four real states", () => {
  const s = (domain) => {
    const r = parseCheck(ok).results.find((x) => x.domain === domain);
    return classify(r);
  };
  assert.equal(s("trovy.xyz").status, "taken");
  assert.equal(s("trovy.xyz").price, "");
  assert.equal(s("cache.xyz").status, "reserved-premium");
  assert.equal(s("cache.xyz").price, "$650 buy / $650/yr");
  assert.equal(s("goldfinger.fun").status, "available-standard");
  assert.equal(s("goldfinger.fun").price, "");
  assert.equal(s("midas.fun").status, "available-premium");
  assert.equal(s("midas.fun").price, "$1625 buy / $6500/yr");
});

test("Namecheap credentials use a local dotenv file with quotes and comments", () => {
  const dir = mkdtempSync(join(tmpdir(), "namecheap-env-"));
  const config = join(dir, ".env");
  try {
    writeFileSync(config, 'INTEL_NAMECHEAP_USERNAME="fixture-user" # comment\nINTEL_NAMECHEAP_API_KEY="fixture#key"\n');
    assert.deepEqual(loadCreds(config), { apiUser: "fixture-user", apiKey: "fixture#key" });
    writeFileSync(config, 'INTEL_NAMECHEAP_USERNAME="fixture-user"\n');
    assert.throws(() => loadCreds(config), /INTEL_NAMECHEAP_API_KEY/);
    assert.throws(() => loadCreds(join(dir, "missing")), { code: "ENOENT" });
  } finally { rmSync(dir, { recursive: true }); }
});
