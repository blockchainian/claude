import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { openJournal, hash } from '../scripts/journal.mjs';
import { reconsiderNames } from '../scripts/collision.mjs';

const captured = JSON.parse(await readFile(new URL('./fixtures/responses-partial-names.json', import.meta.url), 'utf8'));
const capturedText = captured.response.output[0].content[0].text;
const names = { v1: 'application', v2: 'configuration' };
const payload = { context: 'const v1 = 1, v2 = 2, v2031 = 3;', placeholders: { v1: 6, v2: 17, v2031: 28 } };
const records = async path => (await readFile(path, 'utf8')).trim().split('\n').map(JSON.parse);

function incomplete(res, text = capturedText, reason = 'max_output_tokens', envelope = true) {
  const event = structuredClone(captured);
  event.response.incomplete_details.reason = reason;
  event.response.output[0].content[0].text = text;
  if (!envelope) delete event.response.output;
  res.writeHead(200, { 'content-type': 'text/event-stream' });
  // Split SSE frames and output deltas across real HTTP/2 writes.
  const frames = [
    ...Array.from({ length: Math.ceil(text.length / 7) }, (_, i) =>
      ({ type: 'response.output_text.delta', delta: text.slice(i * 7, i * 7 + 7) })), event,
  ].map(value => `data: ${JSON.stringify(value)}\n\n`).join('');
  res.write(frames.slice(0, 23));
  res.end(frames.slice(23));
}

async function fixture(t, handler) {
  const root = await mkdtemp(join(tmpdir(), 'partial-names-'));
  const bodies = [], logs = [], events = [];
  const options = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); bodies.push(body);
    handler(res, body, bodies.length);
  });
  const namer = createNamer({ ...options, retryDelayMs: 0, log: row => logs.push(row) });
  t.after(async () => { await namer.close(); await rm(root, { recursive: true, force: true }); });
  return { root, bodies, logs, events, namer,
    run: settings => run({ command: 'rename-bindings', paths: [root], concurrency: 1, ...settings }, {
      ...select, ...rename, ...checks, output(row) { events.push(row); },
      createNamer: settings => createNamer({ ...settings, ...options, retryDelayMs: 0 }),
    }) };
}

for (const envelope of [true, false]) test(`captured incomplete names survive and only the missing name is supplemented (envelope=${envelope})`, async t => {
  const f = await fixture(t, (res, body, count) => count === 1
    ? incomplete(res, capturedText, 'max_output_tokens', envelope)
    : complete(res, { v2031: 'options' }));
  const result = await f.namer.name(payload, { batch_id: 'original', batch_ids: ['original'], name_count: 3 });
  assert.deepEqual(result.names, { ...names, v2031: 'options' });
  assert.deepEqual(f.bodies.map(body => body.text.format.schema.required), [['v1', 'v2', 'v2031'], ['v2031']]);
  assert.equal(f.bodies[1].input[0].content[0].text.split('\n\n').slice(1).join('\n\n'), payload.context);
  assert.deepEqual(f.logs.map(row => row.name_count), [3, 1]);
  assert(f.logs.every(row => row.batch_id === 'original'));
  assert.equal(result.usage.input_tokens, captured.response.usage.input_tokens + 10);
  assert.equal(result.usage.output_tokens, captured.response.usage.output_tokens + 3);
  assert(!JSON.stringify(f.logs).includes('application'));
});

test('a supplement with some complete values stops after one logical request', async t => {
  const f = await fixture(t, (res, body, count) => incomplete(res, count === 1
    ? '{"v1":"application","v2":"'
    : '{"v2":"configuration","v2031":"'));
  assert.deepEqual((await f.namer.name(payload)).names, names);
  assert.equal(f.bodies.length, 2);
});

test('a supplement with no complete values retries then leaves missing bindings unchanged', async t => {
  const f = await fixture(t, (res, body, count) => incomplete(res, count === 1 ? capturedText : '{"v2031":"'));
  const result = await f.namer.name(payload);
  assert.deepEqual(result.names, names);
  assert.equal(f.bodies.length, 5);
  assert(f.bodies.slice(1).every(body => JSON.stringify(body.text.format.schema.required) === '["v2031"]'));
  const source = 'const a=1,b=2,c=3; console.log(a,b,c);';
  const proposals = new Map([[source.indexOf('a='), result.names.v1], [source.indexOf('b='), result.names.v2]]);
  const after = rename.applyRenames(source, rename.resolveNames(source, proposals));
  assert.match(after, /c=3/);
  assert.equal(checks.checkRewrite(source, after), null);
});

for (const text of ['{"v1":"', '{"foreign":"complete","v1":"', '{"foreign":{"v1":"nested"},"v2":"configuration"'])
  test(`no complete top-level requested names retains four attempts: ${text}`, async t => {
    const f = await fixture(t, res => incomplete(res, text));
    await assert.rejects(f.namer.name(payload));
    assert.equal(f.bodies.length, 4);
  });

test('string escapes and foreign keys cannot manufacture requested pairs', async t => {
  const escaped = 'contains "v2031":"injected" and \\ and ☃';
  const text = `{"foreign":"skip","v1":${JSON.stringify(escaped)},"v2":"configuration","v2031":"unfinished\\"`;
  const f = await fixture(t, (res, body, count) => count === 1 ? incomplete(res, text) : complete(res, { v2031: 'options' }));
  assert.deepEqual((await f.namer.name(payload)).names, { v1: escaped, v2: 'configuration', v2031: 'options' });
  assert.deepEqual(f.bodies[1].text.format.schema.required, ['v2031']);
});

for (const reason of ['content_filter', 'unknown']) test(`other incomplete reasons still fail: ${reason}`, async t => {
  const f = await fixture(t, res => incomplete(res, capturedText, reason));
  await assert.rejects(f.namer.name(payload));
  assert.equal(f.bodies.length, 4);
});

for (const code of ['AUTH', 'USAGE_LIMIT']) test(`supplement preserves ${code}`, async t => {
  const f = await fixture(t, (res, body, count) => {
    if (count === 1) return incomplete(res);
    res.writeHead(code === 'AUTH' ? 401 : 429);
    res.end(JSON.stringify({ error: { code: code === 'AUTH' ? 'unauthorized' : 'usage_limit_reached' } }));
  });
  // No refresh token exists in this local login, so a 401 fails locally without external auth traffic.
  const authPath = join(f.root, '.auth/auth.json');
  const auth = JSON.parse(await readFile(authPath, 'utf8'));
  delete auth.tokens.refresh_token;
  await writeFile(authPath, JSON.stringify(auth));
  await assert.rejects(f.namer.name(payload), { code });
});

test('ordinary partial replies write and replay with missing bindings retained', async t => {
  const f = await fixture(t, (res, body, count) => incomplete(res, count === 1
    ? capturedText.replaceAll('v2031', 'v3')
    : '{"v3":"'));
  execFileSync('git', ['init', '--initial-branch=main', f.root], { stdio: 'ignore' });
  const source = 'const a=1; const b=2; const c=3; console.log(a,b,c);\n';
  await writeFile(join(f.root, 'input.js'), source);
  const settings = {};
  const summary = await f.run(settings);
  assert.equal(summary.exitCode, 0, JSON.stringify({ summary, events: f.events, keys: f.bodies.map(body => body.text.format.schema.required) }));
  const after = await readFile(join(f.root, 'input.js'), 'utf8');
  assert.match(after, /c=3/);
  assert.match(after, /application/);
  assert.equal(checks.checkRewrite(source, after), null);
  const sent = f.bodies.length;
  assert.equal(sent, 5);
  assert.equal((await f.run(settings)).exitCode, 0);
  assert.equal(f.bodies.length, sent);
  {
    const journal = await records(join(f.root, '.unminify/input.js.jsonl'));
    const reply = journal.find(row => row.type === 'response');
    assert.equal(Object.keys(reply.names).length, 2);
    assert.equal(reply.request, `proposal:${select.planFile(source).stages[0][0].id}`);
    await writeFile(join(f.root, 'input.js'), source);
    await writeFile(join(f.root, '.unminify/input.js.jsonl'), journal.slice(0, 2).map(JSON.stringify).join('\n') + '\n');
    assert.equal((await f.run(settings)).exitCode, 0);
    assert.equal(f.bodies.length, sent);
  }
});

test('collision supplements omit missing proposals rather than inventing names', async t => {
  const f = await fixture(t, (res, body, count) => incomplete(res, count === 1
    ? '{"v1":"application","v2":"configuration","v3":"' : '{"v3":"'));
  const source = 'const a=1,b=2,c=3,d=4; console.log(a,b,c,d);';
  const keys = [...source.matchAll(/([a-d])=/g)].map(match => match.index);
  const proposals = () => new Map(keys.map(key => [key, 'sameName']));
  const journal = openJournal(f.root, 'input.js', source, {});
  const resolve = (values, onCollision) => rename.resolveNames(source, values, { onCollision });
  const result = await reconsiderNames({ proposals: proposals(), resolve, journal, id: 'program', namer: f.namer, maxInputTokens: 100000 });
  assert.equal(result.has(keys[3]), false);
  assert.equal(result.get(keys[1]), 'application');
  assert.equal(f.bodies.length, 5);
  const replayed = await reconsiderNames({ proposals: proposals(), resolve,
    journal: openJournal(f.root, 'input.js', source, {}), id: 'program', namer: f.namer, maxInputTokens: 100000 });
  assert.deepEqual(replayed, result);
  assert.equal(f.bodies.length, 5);
});

for (const chunk of [false, true]) test(`empty original batches stay pending while packed peers are journaled (chunk=${chunk})`, async t => {
  let resume = false;
  const f = await fixture(t, (res, body, count) => resume
    ? complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, 'options'])))
    : incomplete(res, count === 1 ? capturedText.replaceAll('v2031', 'v3') : '{"v3":"'));
  execFileSync('git', ['init', '--initial-branch=main', f.root], { stdio: 'ignore' });
  const source = chunk ? 'const a=1; const b=2; const c=3; console.log(a,b,c);\n'
    : 'function calculateFirst(a) { return a; }\nfunction calculateSecond(b) { return b; }\nfunction calculateThird(c) { return c; }\n' + `const description = "${' padding'.repeat(1000)}";\n`;
  await writeFile(join(f.root, 'input.js'), source);
  const settings = { maxInputTokens: chunk ? 100000 : 500 };
  if (chunk) {
    await writeFile(join(f.root, 'tail.js'), 'console.log(c);\n');
    settings.chunks = join(f.root, 'chunks.json');
    await writeFile(settings.chunks, JSON.stringify({ version: 1, chunks: [{ id: 'one', files: ['input.js', 'tail.js'] }] }));
  }
  const summary = await f.run(settings);
  assert.equal(summary.exitCode, 1, JSON.stringify(f.events));
  assert.equal(summary.failedBatches, 1);
  assert.equal(f.bodies.length, 5);
  assert.deepEqual(f.bodies[0].text.format.schema.required, ['v1', 'v2', 'v3']);
  assert.equal(await readFile(join(f.root, 'input.js'), 'utf8'), source);
  const path = chunk ? join(f.root, '.unminify/chunks', `${hash('one')}.jsonl`) : join(f.root, '.unminify/input.js.jsonl');
  const history = await records(path);
  const accepted = history.filter(row => chunk ? row.type === 'chunk-batch' : row.type === 'response');
  assert.equal(accepted.length, 2);
  assert(accepted.every(row => Object.keys(row.names).length === 1));
  assert.equal(history.some(row => row.type === 'done' || row.type === 'chunk-done'), false);
  resume = true;
  const sent = f.bodies.length;
  assert.equal((await f.run(settings)).exitCode, 0, JSON.stringify(f.events));
  assert.equal(f.bodies.length, sent + 1);
  assert.equal(f.bodies.at(-1).text.format.schema.required.length, 1);
  assert.equal(checks.checkRewrite(source, await readFile(join(f.root, 'input.js'), 'utf8')), null);
  assert.equal((await f.run(settings)).exitCode, 0);
  assert.equal(f.bodies.length, sent + 1);
});

test('chunk local partial replies pass chunk validation and preserve missing local names', async t => {
  const f = await fixture(t, (res, body, count) => incomplete(res, count === 1
    ? capturedText.replaceAll('v2031', 'v3') : '{"v3":"'));
  execFileSync('git', ['init', '--initial-branch=main', f.root], { stdio: 'ignore' });
  const source = 'function calculate(a,b,c) { return a+b+c; }\n';
  await writeFile(join(f.root, 'input.js'), source);
  await writeFile(join(f.root, 'tail.js'), 'console.log(calculate(1,2,3));\n');
  const chunks = join(f.root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'one', files: ['input.js', 'tail.js'] }] }));
  const summary = await f.run({ chunks });
  assert.equal(summary.exitCode, 0, JSON.stringify(f.events));
  const after = await readFile(join(f.root, 'input.js'), 'utf8');
  assert.match(after, /application/);
  assert.match(after, /configuration/);
  assert.match(after, /,c\)/);
  assert.equal(checks.checkRewrite(source, after), null);
  assert.equal(f.bodies.length, 5);
  assert.equal((await f.run({ chunks })).exitCode, 0);
  assert.equal(f.bodies.length, 5);
});
