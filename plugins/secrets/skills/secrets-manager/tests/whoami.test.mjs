import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir, homedir } from "node:os";

import { main, parseCli } from "../scripts/cli.mjs";
import { validateAdapter } from "../scripts/adapter.mjs";

let base;
let previous;
let previousHome;
const setAdapters = paths => {
  const dir = join(homedir(), ".config", "secrets-manager");
  mkdirSync(dir, {recursive: true});
  writeFileSync(join(dir, "config.json"), JSON.stringify({adapters: paths}));
};
beforeEach(() => {
  previousHome = process.env.HOME;
  previous = [process.env.SECRETS_STATE_DIR];
  base = mkdtempSync(join(tmpdir(), "sm-whoami-"));
  const adapter = join(base, "adapter.mjs");
  writeFileSync(adapter, `export default () => [{
    name: 'alpha', domain: 'alpha.example', startUrl: 'https://alpha.example/',
    entryTexts: ['Login'], signIn: async () => {}, ready: async () => true,
    whoami: async ({credential}) => {
      if (credential === 'broken') throw new Error('rejected broken');
      if (credential === 'missing') return {};
      if (credential === 'malformed') return {email: 'not an email'};
      if (credential !== 'refresh') throw new Error('wrong credential');
      return {email: 'user@example.com'};
    }
  }, {
    name: 'beta', domain: 'beta.example', startUrl: 'https://beta.example/',
    entryTexts: ['Login'], signIn: async () => {}, ready: async () => true
  }];`);
  process.env.SECRETS_STATE_DIR = join(base, "absent-state");
  process.env.HOME = base;
  setAdapters([adapter]);
});
afterEach(() => {
  if (previousHome === undefined) delete process.env.HOME; else process.env.HOME = previousHome;
  for (const [i, key] of ['SECRETS_STATE_DIR'].entries()) {
    if (previous[i] === undefined) delete process.env[key];
    else process.env[key] = previous[i];
  }
  rmSync(base, {recursive: true, force: true});
});
const output = () => ({out: [], err: [], log(s) {this.out.push(s);}, error(s) {this.err.push(s);}});

test("whoami parses an app, one credential and optional JSON", () => {
  const {command, opts} = parseCli(['whoami', 'alpha', '--select', 'refresh', '--json']);
  assert.equal(command, 'whoami');
  assert.deepEqual(opts.select, ['refresh']);
  assert.equal(opts.json, true);
});

test("whoami rejects missing, blank or repeated credentials and unrelated flags", () => {
  for (const args of [[], ['--select', ''], ['--select', '  '],
    ['--select', 'first', '--select', 'second']]) {
    assert.throws(() => parseCli(['whoami', 'alpha', ...args]), /exactly one nonempty --select/);
  }
  assert.throws(() => parseCli(['whoami', 'alpha', '--select', 'refresh', '--all']), /whoami does not support --all/);
});

test("whoami resolves an external credential without creating local state", async () => {
  const io = output();
  assert.equal(await main(['whoami', 'alpha', '--select', 'refresh'], io), 0);
  assert.deepEqual(io.out, ['user@example.com']);
  assert.deepEqual(io.err, []);
  assert.equal(existsSync(process.env.SECRETS_STATE_DIR), false);
});

test("whoami emits app and email as JSON", async () => {
  const io = output();
  assert.equal(await main(['whoami', 'alpha', '--select', 'refresh', '--json'], io), 0);
  assert.deepEqual(JSON.parse(io.out[0]), {app: 'alpha', email: 'user@example.com'});
});

test("whoami rejects unknown apps and adapters without a hook", async () => {
  for (const app of ['unknown', 'beta']) {
    const io = output();
    assert.equal(await main(['whoami', app, '--select', 'refresh'], io), 1);
    assert.deepEqual(io.out, []);
    assert.match(io.err[0], app === 'beta' ? /beta has no whoami hook/ : /unknown adapter/);
    assert.equal(existsSync(process.env.SECRETS_STATE_DIR), false);
  }
});

test("whoami fails on malformed identities and redacts credentials in errors", async () => {
  for (const credential of ['missing', 'malformed', 'broken']) {
    const io = output();
    assert.equal(await main(['whoami', 'alpha', '--select', credential], io), 1);
    assert.deepEqual(io.out, []);
    if (credential === 'broken') assert.deepEqual(io.err, ['rejected [redacted]']);
    else assert.match(io.err[0], /invalid whoami result/);
  }
});

test("adapter validation rejects a nonfunction whoami hook", () => {
  assert.throws(() => validateAdapter({name: 'alpha', domain: 'alpha.example',
    startUrl: 'https://alpha.example/', entryTexts: ['Login'], signIn() {}, ready() {},
    whoami: true}), /adapter whoami must be a function/);
});
