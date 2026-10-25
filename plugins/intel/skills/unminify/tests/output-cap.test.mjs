import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { createNamer } from '../scripts/model.mjs';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { openJournal, hash } from '../scripts/journal.mjs';
import { planChunk } from '../scripts/chunks.mjs';

const sse = event => `data: ${JSON.stringify(event)}\n\n`;
const records = async path => (await readFile(path, 'utf8')).trim().split('\n').map(JSON.parse);
const sourceWithNames = count => {
  const alphabet = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ';
  return Array.from({ length: count }, (_, i) =>
    `const a${alphabet[Math.floor(i / 52)]}${alphabet[i % 52]} = ${i};\n`).join('');
};

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'output-cap-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const codexHome = join(root, '.auth');
  await mkdir(codexHome);
  const access = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ tokens: {
    access_token: access, account_id: 'local-account', refresh_token: 'local-refresh',
  } }));
  const bodies = [], logs = [], controls = { failKey: null, invalidKey: null };
  const server = createServer(async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), keys = body.text.format.schema.required;
    bodies.push(body);
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    // Envelope from official Codex core/tests/suite/client.rs; reason from the
    // captured engine response.incomplete attempts in requests.jsonl.
    if (keys.includes(controls.failKey)) {
      res.end(sse({ type: 'response.output_text.delta', delta: '{"v1":"partial' })
        + sse({ type: 'response.incomplete', response: { id: 'resp_incomplete',
          object: 'response', status: 'incomplete', error: null,
          incomplete_details: { reason: 'max_output_tokens' },
          usage: { input_tokens: 71083, output_tokens: 12616 } } }));
      return;
    }
    const names = Object.fromEntries(keys.map(key => [key, `readableName${key.slice(1)}`]));
    if (keys.includes(controls.invalidKey)) delete names[controls.invalidKey];
    res.end(sse({ type: 'response.completed', response: { status: 'completed',
      output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
      usage: { input_tokens: 12, output_tokens: keys.length * 8 } } }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const options = { codexHome, baseUrl: `http://127.0.0.1:${server.address().port}`, retryDelayMs: 0 };
  const namer = createNamer({ ...options, log: row => logs.push(row) });
  t.after(async () => {
    await namer.close();
    await new Promise(resolve => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  });
  return { root, namer, bodies, logs, controls,
    run: settings => run({ command: 'rename-bindings', paths: [root], concurrency: 1, ...settings }, {
      ...select, ...rename, ...checks, output() {},
      createNamer: settings => createNamer({ ...settings, ...options }),
    }) };
}

test('naming preserves order, context, schema and original batch identity', async t => {
  const f = await fixture(t);
  const placeholders = Object.fromEntries(Array.from({ length: 2031 }, (_, i) => [`v${i + 1}`, i]));
  const payload = { context: 'const v1 = v2031; // shared context', placeholders };
  const result = await f.namer.name(payload, { file: 'input.js', batch_id: 'original', batch_ids: ['original'], name_count: 2031 });
  assert.deepEqual(Object.keys(result.names), Object.keys(placeholders));
  assert.equal(result.names.v2031, 'readableName2031');
  assert.deepEqual(f.bodies.map(body => body.text.format.schema.required.length), [2031]);
  assert.deepEqual(f.bodies.flatMap(body => body.text.format.schema.required), Object.keys(placeholders));
  for (const body of f.bodies) {
    assert.equal(body.input[0].content[0].text.split('\n\n').slice(1).join('\n\n'), payload.context);
    assert.deepEqual(Object.keys(body.text.format.schema.properties), body.text.format.schema.required);
    assert.equal(body.text.format.schema.additionalProperties, false);
  }
  assert.deepEqual(f.logs.map(row => [row.name_count, row.attempt]),
    [[2031, 1]]);
  assert(f.logs.every(row => row.batch_id === 'original' && row.batch_ids[0] === 'original'));
  assert.equal(result.usage.input_tokens, 12);
  assert.equal(result.usage.output_tokens, 2031 * 8);
  assert.equal(Object.keys(payload.placeholders).length, 2031);
});

test('naming 1000 bindings stays one request', async t => {
  const f = await fixture(t);
  await f.namer.name({ context: 'const v1 = 1;', placeholders:
    Object.fromEntries(Array.from({ length: 1000 }, (_, i) => [`v${i + 1}`, i])) });
  assert.equal(f.bodies.length, 1);
});

for (const mode of ['incomplete', 'invalid']) test(`an ${mode} reply leaves no partial journal reply and resumes the original batch`, async t => {
  const f = await fixture(t);
  const source = sourceWithNames(1001);
  await writeFile(join(f.root, 'input.js'), source);
  const batch = select.planFile(source).stages[0][0];
  assert.equal(batch.keys.length, 1001);
  f.controls[mode === 'incomplete' ? 'failKey' : 'invalidKey'] = 'v1001';
  assert.equal((await f.run()).exitCode, 1);
  const path = join(f.root, '.unminify/input.js.jsonl');
  const interrupted = await records(path);
  assert.equal(interrupted.length, 1, 'no child output is durable before all children validate');
  assert.equal(await readFile(join(f.root, 'input.js'), 'utf8'), source);
  const attempts = await records(join(f.root, '.unminify/requests.jsonl'));
  assert.deepEqual(attempts.map(row => [row.name_count, row.attempt]),
    [[1001, 1], [1001, 2], [1001, 3], [1001, 4]]);
  if (mode === 'incomplete') assert(attempts.every(row => row.failure.incomplete_reason === 'max_output_tokens'));
  assert.deepEqual(f.bodies.map(body => body.text.format.schema.required.length), [1001, 1001, 1001, 1001]);
  f.controls.failKey = f.controls.invalidKey = null;
  const before = f.bodies.length;
  assert.equal((await f.run()).exitCode, 0);
  assert.deepEqual(f.bodies.slice(before).map(body => body.text.format.schema.required.length), [1001]);
  const accepted = await records(path);
  assert.deepEqual(accepted.filter(row => row.id).map(row => row.id), [batch.id]);
  const replies = accepted.filter(row => row.type === 'response');
  assert.equal(replies.length, 1);
  assert.equal(replies[0].request, `proposal:${batch.id}`);
  assert.equal(Object.keys(replies[0].names).length, 1001);
  assert.equal(checks.checkRewrite(source, await readFile(join(f.root, 'input.js'), 'utf8')), null);
  const finished = f.bodies.length;
  assert.equal((await f.run()).exitCode, 0);
  assert.equal(f.bodies.length, finished);
  // An accepted proposal replays without sending another naming request.
  await writeFile(join(f.root, 'input.js'), source);
  await writeFile(path, accepted.slice(0, 2).map(JSON.stringify).join('\n') + '\n');
  const journal = openJournal(f.root, 'input.js', source, { minSymbolLength: 4, maxInputTokens: 100000 });
  assert.equal(journal.replies.size, 1);
  assert.equal((await f.run()).exitCode, 0);
  assert.equal(f.bodies.length, finished);
});

test('chunk program naming commits one complete reply after validation', async t => {
  const f = await fixture(t);
  const source = sourceWithNames(1001);
  await writeFile(join(f.root, 'input.js'), source);
  await writeFile(join(f.root, 'tail.js'), 'console.log(aaa);\n');
  const chunks = join(f.root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'one', files: ['input.js', 'tail.js'] }] }));
  f.controls.failKey = 'v1001';
  assert.equal((await f.run({ chunks, maxInputTokens: 1000000 })).exitCode, 1);
  const path = join(f.root, '.unminify/chunks', `${hash('one')}.jsonl`);
  assert.equal((await records(path)).filter(row => row.type === 'response' || row.type === 'chunk-batch').length, 0);
  assert.equal(await readFile(join(f.root, 'input.js'), 'utf8'), source);
  f.controls.failKey = null;
  const before = f.bodies.length;
  assert.equal((await f.run({ chunks, maxInputTokens: 1000000 })).exitCode, 0);
  assert.deepEqual(f.bodies.slice(before).map(body => body.text.format.schema.required.length), [1001]);
  const batches = (await records(path)).filter(row => row.type === 'chunk-batch' && row.id !== 'program-resolved');
  const planned = planChunk(new Map([['input.js', source], ['tail.js', 'console.log(aaa);\n']]), { maxInputTokens: 1000000 });
  assert.deepEqual(batches.map(row => row.id), planned.map(batch => batch.id));
  assert.equal(batches.reduce((sum, row) => sum + Object.keys(row.names).length, 0), 1001);
  assert.match(await readFile(join(f.root, 'tail.js'), 'utf8'), /console\.log\(readableName1\)/);
  const finished = f.bodies.length;
  assert.equal((await f.run({ chunks, maxInputTokens: 1000000 })).exitCode, 0);
  assert.equal(f.bodies.length, finished);
});
