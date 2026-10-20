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
import { getEncoding } from '../scripts/node_modules/js-tiktoken/dist/index.js';
import { report } from '../scripts/perf.mjs';
import { openJournal } from '../scripts/journal.mjs';

const encoder = getEncoding('o200k_base');
const inputTokens = body => encoder.encode(body.instructions, [], []).length + encoder.encode(body.input[0].content[0].text, [], []).length;

const planOptions = options => ({ shortLen: 3, contextTokens: 1000, ...options });
const sourceWithFunctions = (count, padding = 17000) => `const a = 7;\n${Array.from({ length: count }, (_, index) =>
  `function calculateValue${index}(x) { const y = x + a; return y * 2; }`).join('\n')}\n/*${' padding'.repeat(Math.ceil(padding / 8))}*/\n`;
const records = async path => (await readFile(path, 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));

// This is the local HTTP/2 Responses endpoint; all planning, rendering, validation,
// rewriting, persistence and telemetry use the production implementations.
async function fixture(t, source) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-packing-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'input.mjs'), source);
  const codexHome = join(root, '.auth');
  await mkdir(codexHome);
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    tokens: { access_token: token, refresh_token: 'local-refresh', account_id: 'local-account' } }));
  const requests = [];
  const controls = { failAt: null };
  const server = createServer(async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), schema = body.text.format.schema;
    const text = body.input[0].content[0].text;
    const context = text.slice(text.indexOf('\n\n') + 2);
    const index = requests.length + 1;
    const names = Object.fromEntries(schema.required.map((key, offset) => [key, `readableName${index}_${offset}`]));
    requests.push({ body, context, names });
    if (controls.failAt === index) {
      res.writeHead(429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local test interruption' } }));
      return;
    }
    res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
      output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
      usage: { input_tokens: 12, output_tokens: 3 } } })}\n\n`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => {
    await new Promise(resolve => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  });
  const dependencies = { ...select, ...rename, ...checks, output() {},
    createNamer(options) { return createNamer({ ...options, codexHome,
      baseUrl: `http://127.0.0.1:${server.address().port}`, retryDelayMs: 1 }); } };
  return { root, requests, controls,
    run: options => run({ paths: [root], concurrency: 1, ...options }, dependencies),
    logs: async runId => (await records(join(root, '.unminify', 'requests.jsonl'))).filter(row => row.run_id === runId && row.kind === 'model'),
    journal: () => records(join(root, '.unminify', 'input.mjs.jsonl')) };
}

function assertSchema(request, expectedCount) {
  const { schema, strict } = request.body.text.format;
  assert.equal(strict, true);
  assert.equal(schema.additionalProperties, false);
  assert.equal(schema.required.length, expectedCount);
  assert.equal(new Set(schema.required).size, expectedCount);
  assert.deepEqual(Object.keys(schema.properties), schema.required);
  const contextPlaceholders = new Set(request.context.match(/\bv\d+\b/g));
  assert.deepEqual([...contextPlaceholders].sort(), [...schema.required].sort());
  for (const key of schema.required) assert.deepEqual(schema.properties[key], { type: 'string' });
}

for (const contextTokens of [1000, 1600]) test(`same-file small function batches share bounded requests (${contextTokens} tokens)`, async t => {
  const source = sourceWithFunctions(60);
  const options = planOptions({ contextTokens });
  const stages = select.planFile(source, options).stages;
  assert.equal(stages.length, 2);
  assert.equal(stages[1].length, 60);
  const target = await fixture(t, source);
  const summary = await target.run(options);
  assert.equal(summary.exitCode, 0);
  const logs = await target.logs(summary.run_id);
  assert(logs.length < stages.flat().length / 2, `${logs.length} requests for ${stages.flat().length} batches`);
  assert.equal(summary.requests, logs.length);
  assert.deepEqual(logs.flatMap(row => row.batch_ids), stages.flat().map(batch => batch.id));
  const batches = new Map(stages.flat().map(batch => [batch.id, batch]));
  for (const [index, row] of logs.entries()) {
    assert(row.batch_ids.length > 0);
    assert.equal(row.file, 'input.mjs');
    assert.equal(row.name_count, row.batch_ids.reduce((sum, id) => sum + batches.get(id).keys.length, 0));
    assert.equal(row.estimated_input_tokens, inputTokens(target.requests[index].body));
    assert(row.estimated_input_tokens <= options.contextTokens);
    assertSchema(target.requests[index], row.name_count);
    if (row.batch_ids.length > 1) {
      const first = select.renderBatch(source, batches.get(row.batch_ids[0])).context;
      assert(target.requests[index].context.length > first.length, 'all packed contexts are included');
      assert.match(target.requests[index].context, /calculateValue/);
    }
  }
  assert.equal(logs[0].batch_ids.length, 1);
  const topName = Object.values(target.requests[0].names)[0];
  for (const request of target.requests.slice(1)) assert(request.context.includes(topName), 'functions see the accepted top-level name');
  assert(logs.slice(1).some(row => row.name_count > 8), 'names do not impose a cap');
  assert(logs.length > 2, 'token budget splits the stage');
  assert.equal(summary.estimated_input_tokens, logs.reduce((sum, row) => sum + row.estimated_input_tokens, 0));
  assert.equal(summary.tokens.input_tokens, logs.length * 12);
  for (let index = 1; index + 1 < logs.length; index++) {
    const row = logs[index], next = batches.get(logs[index + 1].batch_ids[0]);
    const extra = select.renderBatch(source, next, new Map([[stages[0][0].keys[0], topName]]), {
      placeholderOffset: row.name_count, placeholderCount: row.name_count + next.keys.length,
    });
    const text = target.requests[index].body.input[0].content[0].text;
    const placeholders = [...Object.keys(target.requests[index].names), ...Object.keys(extra.placeholders)].join(', ');
    const prefix = text.slice(0, text.indexOf('\n\n')).replace(/placeholders: .*?\. Use/, `placeholders: ${placeholders}. Use`);
    const context = (row.batch_ids.length === 1 ? `// Batch ${row.batch_ids[0]}\n` : '')
      + target.requests[index].context + `\n\n// Batch ${next.id}\n${extra.context}`;
    assert(encoder.encode(target.requests[index].body.instructions, [], []).length
      + encoder.encode(`${prefix}\n\n${context}`, [], []).length > options.contextTokens,
    'the next batch exceeds the budget including prompt and separators');
  }
  const counts = logs.map(row => row.name_count).sort((a, b) => a - b);
  const stats = { median: (counts[Math.floor((counts.length - 1) / 2)] + counts[Math.floor(counts.length / 2)]) / 2, mean: counts.reduce((sum, value) => sum + value, 0) / counts.length };
  assert.deepEqual(summary.names_per_request, stats);
  const performance = report({ dir: target.root, runId: summary.run_id });
  assert.equal(performance.requests, logs.length);
  assert.deepEqual(performance.names_per_request, stats);
  assert.equal(checks.checkRewrite(source, await readFile(join(target.root, 'input.mjs'), 'utf8')), null);
  assert.deepEqual((await target.journal()).filter(row => row.id).map(row => row.id), stages.flat().map(batch => batch.id));
});

test('large optional context is shortened and every request includes prompt overhead', async t => {
  const source = sourceWithFunctions(12, 0) + `function calculateLarge(z) { const q = "${'optional context '.repeat(1000)}"; return z + a + q; }`;
  const options = planOptions({ contextTokens: 400 });
  const target = await fixture(t, source);
  const summary = await target.run(options);
  assert.equal(summary.exitCode, 0);
  const logs = await target.logs(summary.run_id);
  assert(logs.length > 2);
  assert(logs.some(row => row.batch_ids.length > 1), 'small contexts still share requests');
  assert(target.requests.every(request => !request.context.includes('optional context')), 'oversized optional initializer is removed');
  for (const [index, row] of logs.entries()) {
    assert.equal(row.estimated_input_tokens, inputTokens(target.requests[index].body));
    assert(row.estimated_input_tokens <= options.contextTokens);
    assertSchema(target.requests[index], row.name_count);
  }
  assert.equal(checks.checkRewrite(source, await readFile(join(target.root, 'input.mjs'), 'utf8')), null);
});

test('default budget keeps hundreds of small functions in a single request', async t => {
  const source = sourceWithFunctions(300, 0);
  const target = await fixture(t, source);
  const summary = await target.run({});
  assert.equal(summary.exitCode, 0);
  assert.equal(target.requests.length, 1);
  assert(target.requests[0].body.text.format.schema.required.length > 600);
  const logs = await target.logs(summary.run_id);
  assert.equal(logs[0].estimated_input_tokens, inputTokens(target.requests[0].body));
  assert.equal(summary.estimated_input_tokens, logs[0].estimated_input_tokens);
  assert.equal(report({ dir: target.root }).estimated_input_tokens, summary.estimated_input_tokens);
  assert.equal(report({ dir: target.root }).input_tokens, summary.tokens.input_tokens);
  assert.equal(checks.checkRewrite(source, await readFile(join(target.root, 'input.mjs'), 'utf8')), null);
});

test('quota interruption resumes only unrecorded original batch ids', async t => {
  const source = sourceWithFunctions(20);
  const options = planOptions({ contextTokens: 500 });
  const stages = select.planFile(source, options).stages;
  const target = await fixture(t, source);
  target.controls.failAt = 3;
  const interrupted = await target.run(options);
  assert.equal(interrupted.exitCode, 1);
  assert.equal(await readFile(join(target.root, 'input.mjs'), 'utf8'), source);
  const accepted = (await target.journal()).filter(row => row.id);
  assert(accepted.length > 1 && accepted.length < stages.flat().length);
  assert(accepted.every(row => !row.reason));
  const acceptedIds = new Set(accepted.map(row => row.id));
  const before = target.requests.length;
  const changed = await target.run({ ...options, contextTokens: 501 });
  assert.equal(changed.exitCode, 1);
  assert.equal(target.requests.length, before, 'option mismatch sends no request');
  target.controls.failAt = null;
  const resumed = await target.run(options);
  assert.equal(resumed.exitCode, 0);
  const resumedLogs = await target.logs(resumed.run_id);
  assert.deepEqual(resumedLogs.flatMap(row => row.batch_ids), stages.flat().map(batch => batch.id).filter(id => !acceptedIds.has(id)));
  assert.equal(resumed.requests, target.requests.length - before);
  const topName = Object.values(target.requests[0].names)[0];
  for (const request of target.requests.slice(before)) assert(request.context.includes(topName));
  const journalIds = (await target.journal()).filter(row => row.id).map(row => row.id);
  assert.equal(new Set(journalIds).size, stages.flat().length);
  assert.equal(journalIds.length, stages.flat().length);
  assert.equal(checks.checkRewrite(source, await readFile(join(target.root, 'input.mjs'), 'utf8')), null);
});


test('placeholders from other packed batches cannot shadow imported context names', async t => {
  const source = `import { external as v3 } from 'external';\n${sourceWithFunctions(24).replaceAll('x + a', 'x + v3')}`;
  const options = planOptions({ contextTokens: 500 });
  const target = await fixture(t, source);
  const summary = await target.run(options);
  assert.equal(summary.exitCode, 0);
  const logs = await target.logs(summary.run_id);
  assert(logs.some(row => row.batch_ids?.length > 1));
  for (const [index, row] of logs.entries()) {
    if (!row.batch_ids.some(id => id.startsWith('Function'))) continue;
    const context = target.requests[index].context;
    assert.match(context, /_context\d+/, 'untargeted imported names receive a distinct context alias');
    assert.doesNotMatch(context, /\+ v3[;\s]/, 'v3 is reserved for a target in the combined request');
    assertSchema(target.requests[index], row.name_count);
  }
  assert.equal(checks.checkRewrite(source, await readFile(join(target.root, 'input.mjs'), 'utf8')), null);
});

test('separate files never share a model request', async t => {
  const first = 'const a = 1, b = 3; console.log(a, b);';
  const second = 'const b = 2; console.log(b);';
  const target = await fixture(t, first);
  await writeFile(join(target.root, 'second.mjs'), second);
  const summary = await target.run({});
  assert.equal(summary.exitCode, 0);
  const logs = await target.logs(summary.run_id);
  assert.equal(logs.length, 2);
  assert.deepEqual(logs.map(row => row.file).sort(), ['input.mjs', 'second.mjs']);
  assert(logs.every(row => row.batch_ids?.length === 1));
  assert.deepEqual(logs.map(row => row.name_count).sort(), [1, 2]);
  assert.deepEqual(summary.names_per_request, { median: 1.5, mean: 1.5 });
  assert.deepEqual(report({ dir: target.root, runId: summary.run_id }).names_per_request, { median: 1.5, mean: 1.5 });
  assert.equal(checks.checkRewrite(first, await readFile(join(target.root, 'input.mjs'), 'utf8')), null);
  assert.equal(checks.checkRewrite(second, await readFile(join(target.root, 'second.mjs'), 'utf8')), null);
});

test('single-batch requests preserve their original rendered context', async t => {
  const source = "import { external as v60 } from 'external'; const a = 1; console.log(a, v60);";
  const batch = select.planFile(source).stages[0][0];
  const expected = select.renderBatch(source, batch).context;
  const target = await fixture(t, source);
  const summary = await target.run({});
  assert.equal(summary.exitCode, 0);
  assert.equal(target.requests.length, 1);
  assert.equal(target.requests[0].context, expected);
});


test('an irreducible batch over budget is rejected before HTTP and preserves source', async t => {
  const source = 'const a = 1; console.log(a);';
  const target = await fixture(t, source);
  const summary = await target.run({ contextTokens: 1 });
  assert.equal(summary.exitCode, 1);
  assert.equal(target.requests.length, 0);
  assert.equal(await readFile(join(target.root, 'input.mjs'), 'utf8'), source);
  assert.equal((await target.journal()).filter(row => row.id).length, 0);
});


test('an unfinished journal with removed batch options is refused', async t => {
  const source = sourceWithFunctions(4, 0);
  const target = await fixture(t, source);
  openJournal(target.root, 'input.mjs', source, { shortLen: 3, maxIds: 60, contextBytes: 8000 });
  const before = await target.journal();
  const summary = await target.run({});
  assert.equal(summary.exitCode, 1);
  assert.equal(target.requests.length, 0);
  assert.deepEqual(await target.journal(), before);
  assert.equal(await readFile(join(target.root, 'input.mjs'), 'utf8'), source);
});
