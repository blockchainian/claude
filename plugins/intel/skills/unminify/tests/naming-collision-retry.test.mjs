import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

for (const [chunk, local, retryName] of [[false, false, 'readLimit'], [true, false, 'readLimit'], [true, true, 'readLimit'], [false, false, 'readCount']]) test(`collision asks for a contextual distinct name and resumes (${chunk ? 'chunk' : 'file'}, local=${local}, retry=${retryName})`, async t => {
  const root = await mkdtemp(join(tmpdir(), 'naming-retry-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'a.js'), (local ? 'function container() {\n' : '') + 'function a() { return 1; }\nfunction b() { return 2; }\nconsole.log(a(), b());\n' + (local ? '}\ncontainer();\n' : ''));
  const manifest = join(root, 'chunks.json');
  if (chunk) await writeFile(join(root, 'z.js'), 'console.log(42);\n');
  if (chunk) await writeFile(manifest, JSON.stringify({ version: 1, chunks: [{ id: 'sample', files: ['a.js', 'z.js'] }] }));
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests.push(body);
    const keys = body.text.format.schema.required;
    complete(res, Object.fromEntries(keys.map(key => [key, requests.length === 1 ? 'readCount' : retryName])));
  });
  let interrupted = true;
  const dependencies = { ...select, ...rename, ...checks, output() {},
    checkRewrite(...args) { if (interrupted) { interrupted = false; throw new Error('interrupt after saved replies'); } return checks.checkRewrite(...args); },
    createNamer: options => createNamer({ ...options, ...service }),
  };
  const settings = { paths: [root], maxInputTokens: 2000, ...(chunk ? { chunks: manifest } : {}) };
  const first = await run(settings, dependencies);
  assert.equal(requests.length, 2, 'collision must send another model request');
  const prompt = requests[1].input[0].content[0].text;
  assert.match(prompt, /readCount/);
  assert.match(prompt, /return 2/);
  assert.match(requests[0].input[0].content[0].text, /same scope.*(?:duplicate|same name)|(?:duplicate|same name).*same scope/i);
  assert.match(requests[0].input[0].content[0].text, /is\/has\/should\/can prefixes only for boolean variables or functions returning boolean values/);
  assert.equal(first.requests, 2);
  assert.equal(first.exitCode, 1, 'write check interruption must precede resume');
  const second = await run(settings, dependencies);
  assert.equal(requests.length, 2, 'resume must replay retry reply');
  assert.equal(second.exitCode, 0);
  assert.match(await readFile(join(root, 'a.js'), 'utf8'), new RegExp(`function ${retryName === 'readCount' ? 'readCount2' : retryName}\\(\\)`));
});

test('retry-created collisions also ask once per binding', async t => {
  const root = await mkdtemp(join(tmpdir(), 'naming-chain-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'a.js'), 'const a = 1; const b = 2; const c = 3; console.log(a, b, c);');
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests.push(body);
    const keys = body.text.format.schema.required;
    complete(res, Object.fromEntries(keys.map((key, index) => [key, requests.length === 1 ? ['readCount', 'readCount', 'readLimit'][index] : requests.length === 2 ? 'readLimit' : 'readTotal'])));
  });
  const result = await run({ paths: [root], maxInputTokens: 2000 }, { ...select, ...rename, ...checks, output() {}, createNamer: options => createNamer({ ...options, ...service }) });
  assert.equal(result.exitCode, 0);
  assert.equal(requests.length, 3);
  assert.match(await readFile(join(root, 'a.js'), 'utf8'), /const readTotal = 3/);
});

test('parameter collision retry includes its owning function context', async t => {
  const root = await mkdtemp(join(tmpdir(), 'naming-parameter-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'a.js'), 'function configure(a, b) { return a + b; }\nconsole.log(configure(1, 2));');
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests.push(body);
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, requests.length === 1 ? 'count' : 'limit'])));
  });
  const result = await run({ paths: [root], maxInputTokens: 2000 }, { ...select, ...rename, ...checks, output() {}, createNamer: options => createNamer({ ...options, ...service }) });
  assert.equal(result.exitCode, 0);
  assert.equal(requests.length, 2);
  assert.match(requests[1].input[0].content[0].text, /function configure\(a, b\).*return a \+ b/);
  assert.match(await readFile(join(root, 'a.js'), 'utf8'), /configure\(count, limit\)/);
});

test('collision retries enforce the input budget and log their own token estimate and batch', async t => {
  const { reconsiderNames } = await import('../scripts/collision.mjs');
  const { openJournal } = await import('../scripts/journal.mjs');
  const { countTokens } = await import('../scripts/prompt.mjs');
  const root = await mkdtemp(join(tmpdir(), 'naming-budget-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = 'const occupied = 1; const a = 2; console.log(a, occupied);';
  await writeFile(join(root, 'a.js'), source);
  const key = source.indexOf('a =');
  const requests = [], telemetry = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests.push(body);
    complete(res, { v1: requests.length === 1 ? 'occupied' : 'count' });
  });
  const namer = createNamer({ ...service, log: record => telemetry.push(record) });
  t.after(() => namer.close());
  const initial = await namer.name({ context: source, placeholders: { v1: key } }, { file: 'a.js', batch_id: 'initial', batch_ids: ['initial'], name_count: 1 });
  const journal = openJournal(root, 'a.js', source, {});
  const parameters = { proposals: new Map([[key, initial.names.v1]]), journal, id: 'tiny', namer, telemetry: { file: 'a.js' },
    resolve: (proposals, onCollision) => rename.resolveNames(source, proposals, { onCollision }) };
  await assert.rejects(reconsiderNames({ ...parameters, maxInputTokens: 1 }), /cannot fit the input token budget/);
  assert.equal(requests.length, 1, 'over-budget retry sends no HTTP request');
  const names = await reconsiderNames({ ...parameters, maxInputTokens: 2000 });
  assert.equal(names.get(key), 'count');
  assert.equal(requests.length, 2);
  const retry = telemetry.filter(record => record.kind === 'model').at(-1);
  assert.equal(retry.batch_id, `collision:tiny:${key}`);
  assert.deepEqual(retry.batch_ids, [`collision:tiny:${key}`]);
  assert.equal(retry.name_count, 1);
  assert.equal(retry.estimated_input_tokens, countTokens(requests[1].instructions) + countTokens(requests[1].input[0].content[0].text));
  assert.deepEqual(journal.replies.get(`collision:tiny:${key}`), { [key]: 'count' });
});

test('large parameter owners provide bounded declaration and reference excerpts', () => {
  const source = `function configure(a, b) { const explanation = "${'padding '.repeat(500)}"; return a + b; }`;
  const collisions = [];
  rename.resolveNames(source, new Map([[source.indexOf('a,'), 'count'], [source.indexOf('b)'), 'count']]), { onCollision: collision => collisions.push(collision) });
  assert.equal(collisions.length, 1);
  assert.match(collisions[0].context, /Binding b:\nfunction configure\(a, b\)/);
  assert.match(collisions[0].context, /return a \+ b/);
  assert(collisions[0].context.length < 2000);
});

test('quota stop journals accepted batches from its own unfinished stage before returning', async t => {
  const root = await mkdtemp(join(tmpdir(), 'naming-stage-stop-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const source = 'const a = 3;\n' + Array.from({ length: 16 }, (_, index) => `function calculateAmount${index}(p) { const q = p + a; return q; }`).join('\n') + `\n/*${' context'.repeat(900)}*/`;
  await writeFile(join(root, 'a.js'), source);
  let requests = 0, stop = true;
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests++;
    if (stop && requests === 3) {
      res.writeHead(429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local quota stop' } }));
      return;
    }
    complete(res, Object.fromEntries(body.text.format.schema.required.map((key, index) => [key, `amount${requests}_${index}`])));
  });
  const dependencies = { ...select, ...rename, ...checks, output() {}, createNamer: options => createNamer({ ...options, ...service }) };
  const options = { paths: [root], concurrency: 1, maxInputTokens: 500 };
  const first = await run(options, dependencies);
  assert.equal(first.exitCode, 1);
  assert.equal(requests, 3, 'quota stop must prevent queued model calls');
  const journal = (await readFile(join(root, '.unminify/a.js.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  assert(journal.filter(record => record.id).length > 1, 'acceptance includes drained batches from the interrupted function stage');
  assert.equal(await readFile(join(root, 'a.js'), 'utf8'), source);
  stop = false;
  const resumed = await run(options, dependencies);
  assert.equal(resumed.exitCode, 0);
  assert.equal(checks.checkRewrite(source, await readFile(join(root, 'a.js'), 'utf8')), null);
});
