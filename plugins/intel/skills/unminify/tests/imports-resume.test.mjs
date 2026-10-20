import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { createNamer } from '../scripts/model.mjs';
import { openJournal } from '../scripts/journal.mjs';

const previous = {
  'previous.mjs': "import { a as x } from './a-ready.mjs'; export const c = x;\n",
  'imported.mjs': "import { stableValue as x } from './stable.mjs'; import { a as y } from './a-ready.mjs'; console.log(x, y);\n",
  'stable.mjs': 'export const stableValue = 10;\n',
};
const extraContext = `function extraContext() { return "${'padding '.repeat(2000)}"; }\n`;
const added = {
  'a-ready.mjs': 'export const a = 101;\n',
  'b-fresh.mjs': "import { a as x } from './a-ready.mjs'; import { b as y } from './z-pending.mjs'; console.log(x, y);\n",
  'c-unfinished.mjs': "import { a as x } from './a-ready.mjs'; const n = 1; function calculateInterrupted(p) { return x + n + p; }\nexport {};\n"
    + extraContext,
  'z-pending.mjs': 'export const b = 2; function calculatePending(p) { return p + b; }\n' + extraContext,
};
const records = data => data.trim().split('\n').map(line => JSON.parse(line));

// Only the HTTP/2 Responses service is local; the complete unminify pipeline is real.
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-imports-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const codexHome = join(root, '.auth');
  await mkdir(codexHome);
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    tokens: { access_token: token, refresh_token: 'local-refresh', account_id: 'local-account' } }));
  const controls = { code: null, snapshot: null, requests: 0, refreshes: 0 };
  const read = file => readFile(join(root, file));
  const journal = file => read(`.unminify/${file}.jsonl`);
  const server = createServer(async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    if (req.url === '/oauth/token') {
      controls.refreshes++;
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'invalid_grant' }));
      return;
    }
    controls.requests++;
    const body = JSON.parse(raw), schema = body.text.format.schema;
    const context = body.input[0].content[0].text;
    if (controls.code && /function calculateInterrupted\(v\d+\)/.test(context)) {
      controls.snapshot = new Map(await Promise.all(['c-unfinished.mjs', 'z-pending.mjs'].map(async file =>
        [file, { source: await read(file), journal: await journal(file) }])));
      res.writeHead(controls.code === 'AUTH' ? 401 : 429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local test interruption' } }));
      return;
    }
    const names = Object.fromEntries(schema.required.map((key, index) =>
      [key, context.includes('= 101;') ? 'exportedValue' : `readableValue${index + 1}`]));
    res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
      output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
      usage: { input_tokens: 12, output_tokens: 3 } } })}\n\n`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => {
    await new Promise(resolve => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  });
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const lines = [];
  const dependencies = { ...select, ...rename, ...checks, output: line => lines.push(line),
    createNamer: options => createNamer({ ...options, codexHome, baseUrl,
      refreshUrl: `${baseUrl}/oauth/token`, retryDelayMs: 1 }) };
  return { root, read, journal, controls, lines,
    write: files => Promise.all(Object.entries(files).map(([file, source]) => writeFile(join(root, file), source))),
    run: () => run({ paths: [root], concurrency: 1, maxInputTokens: 500 }, dependencies) };
}

for (const code of ['USAGE_LIMIT', 'AUTH']) test(`${code} follows imports only in done files and resumes byte-identically`, async t => {
  const target = await fixture(t);
  await target.write(previous);
  assert.equal((await target.run()).exitCode, 0);
  assert.equal(records((await target.journal('previous.mjs')).toString()).at(-1).status, 'renamed');
  assert.equal(records((await target.journal('imported.mjs')).toString()).at(-1).status, 'imports');
  await target.write(added);
  target.controls.code = code;
  const interrupted = await target.run();
  assert.equal(interrupted.exitCode, 1);
  assert.match(target.lines.join('\n'), new RegExp(`${code}: .*Resume`));
  assert(target.controls.snapshot, 'the function batch interrupts after the top-level reply was saved');
  assert.equal(target.controls.refreshes, code === 'AUTH' ? 1 : 0);
  for (const file of ['b-fresh.mjs', 'previous.mjs', 'imported.mjs']) {
    assert.match((await target.read(file)).toString(), /a as exportedValue/, `${file} follows the completed exporter during interruption`);
    assert.equal(records((await target.journal(file)).toString()).at(-1).status, 'imports');
  }
  assert.match((await target.read('b-fresh.mjs')).toString(), /b as b/, 'an unfinished exporter still supplies its current short name');
  for (const [file, before] of target.controls.snapshot) {
    assert.deepEqual(await target.read(file), before.source, `${file} source stays untouched`);
    assert.deepEqual(await target.journal(file), before.journal, `${file} journal stays untouched`);
    assert(!records(before.journal.toString()).some(record => record.type === 'done'));
  }
  assert(records((await target.journal('c-unfinished.mjs')).toString()).some(record => record.id), 'the unfinished file retains a saved batch');
  target.controls.code = null;
  assert.equal((await target.run()).exitCode, 0, 'resume accepts the original source fingerprint and batch positions');

  const uninterrupted = await fixture(t);
  await uninterrupted.write({ ...previous, ...added });
  assert.equal((await uninterrupted.run()).exitCode, 0);
  for (const file of Object.keys({ ...previous, ...added })) {
    assert.deepEqual(await target.read(file), await uninterrupted.read(file), `${file} matches an uninterrupted run byte for byte`);
  }
  const requestCount = target.controls.requests;
  assert.equal((await target.run()).exitCode, 0);
  assert.equal(target.controls.requests, requestCount, 'completed journals require no model calls');
});

test('a journal error still follows completed imports without changing the unfinished file', async t => {
  const target = await fixture(t);
  const file = 'c-unfinished.mjs';
  await target.write(added);
  openJournal(target.root, file, added[file], { minSymbolLength: 4, maxInputTokens: 499 });
  const before = { source: await target.read(file), journal: await target.journal(file) };
  const summary = await target.run();
  assert.equal(summary.exitCode, 1);
  assert.match(target.lines.join('\n'), /options changed: c-unfinished.mjs/);
  assert.match((await target.read('b-fresh.mjs')).toString(), /a as exportedValue/);
  assert.deepEqual(await target.read(file), before.source);
  assert.deepEqual(await target.journal(file), before.journal);
});
