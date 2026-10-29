import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile, execFileSync } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import { createNamer } from '../scripts/model.mjs';

const execute = promisify(execFile);
const perfScript = new URL('../scripts/perf.mjs', import.meta.url);

async function localRun(t, multipleBatches = false) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-perf-'));
  const codexHome = await mkdtemp(join(tmpdir(), 'unminify-perf-auth-'));
  t.after(async () => {
    await rm(root, { recursive: true, force: true });
    await rm(codexHome, { recursive: true, force: true });
  });
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'sample.js'), 'export const a = 1; console.log(a);\n');
  await writeFile(join(root, 'consumer.js'), "import { a as b } from './sample.js'; console.log(b);\n");
  if (multipleBatches) {
    await rm(join(root, 'sample.js'));
    await writeFile(join(root, 'a-sample.js'), "import { a as b } from './consumer.js'; const c = 1; console.log(b, c);\n");
    await writeFile(join(root, 'consumer.js'), 'export const a = 1; export const b = 2; export const c = 3; export const d = 4; console.log(a, b, c, d);\n');
  }
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    last_refresh: new Date().toISOString(), tokens: { access_token: token, account_id: 'local-test' } }));
  let attempts = 0;
  const sessions = new Set();
  const server = createServer(async (req, res) => {
    try {
      let body = '';
      for await (const chunk of req) body += chunk;
      const request = JSON.parse(body);
      const liveEvents = (await readFile(join(root, '.unminify', 'rename-bindings', 'performance.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line));
      assert(liveEvents.some(event => event.type === 'run_start'));
      assert(!liveEvents.some(event => event.type === 'summary'));
      if (attempts > 0) {
        const liveRequests = (await readFile(join(root, '.unminify', 'rename-bindings', 'requests.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line));
        assert.equal(liveRequests.length, attempts);
      }
      attempts++;
      if (attempts === 1) {
        res.writeHead(429, { 'retry-after': '0.015' });
        res.end(JSON.stringify({ detail: 'Rate limit exceeded' }));
        return;
      }
      await delay(multipleBatches && attempts > 2 ? 150 : 5);
      const names = Object.fromEntries(Object.keys(request.text.format.schema.properties).map(key => [key, multipleBatches ? `sampleCount${attempts}` : 'sampleCount']));
      // Responses completion envelope used by the local HTTP/2 client fixtures.
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
        output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
        usage: { input_tokens: 12, output_tokens: 8 } } })}\n\n`);
    } catch (error) { res.destroy(error); }
  });
  server.on('session', session => {
    sessions.add(session);
    session.on('error', () => {});
    session.on('close', () => sessions.delete(session));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => {
    for (const session of sessions) session.destroy();
    await new Promise(resolve => server.close(resolve));
  });
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const dependencies = { ...select, ...rename, ...checks,
    createNamer: options => createNamer({ ...options, baseUrl, retryDelayMs: 1 }), output: () => {} };
  const summary = await run({ paths: [root], codexHome, concurrency: 1, ...(multipleBatches ? { maxInputTokens: 80 } : {}) }, dependencies);
  assert.equal(summary.exitCode, 0);
  return { root, summary, codexHome, dependencies };
}

const records = async path => (await readFile(path, 'utf8')).trim().split('\n').map(line => JSON.parse(line));

test('perf summarizes recorded local HTTP/2 run without model calls', async t => {
  const { root, summary, codexHome, dependencies } = await localRun(t);
  const result = await execute(process.execPath, [perfScript.pathname, root, '--window-seconds', '0.01', '--slow-files', '10']);
  const report = JSON.parse(result.stdout);
  const requests = await records(join(root, '.unminify', 'rename-bindings', 'requests.jsonl'));
  const performance = await records(join(root, '.unminify', 'rename-bindings', 'performance.jsonl'));
  assert.deepEqual(requests.map(request => request.status), [429, 200]);
  assert(requests.every(request => request.estimated_input_tokens > 0));
  assert.equal(requests[0].estimated_input_tokens, requests[1].estimated_input_tokens);
  assert.equal(summary.estimated_input_tokens, requests.reduce((sum, row) => sum + row.estimated_input_tokens, 0));
  assert.equal(report.estimated_input_tokens, summary.estimated_input_tokens);
  assert.equal(report.input_tokens, summary.tokens.input_tokens);
  assert.equal(report.run_id, summary.run_id);
  assert.equal(report.total_ms, summary.total_ms);
  assert.deepEqual(report.phase_ms, summary.phase_ms);
  assert.equal(report.windows.reduce((sum, window) => sum + window.sent, 0), requests.length);
  assert.equal(report.windows.reduce((sum, window) => sum + window.completed, 0), requests.length);
  assert.equal(report.windows.reduce((sum, window) => sum + window.rate_limits, 0), 1);
  assert.equal(report.windows.at(-1).in_flight, 0);
  assert(report.windows.every(window => window.peak_in_flight <= 1));
  const durationTotal = report.windows.reduce((sum, window) => sum + (window.average_duration_ms ?? 0) * window.completed, 0);
  assert(Math.abs(durationTotal - requests.reduce((sum, request) => sum + request.duration_ms, 0)) < 0.001);
  assert(summary.total_ms > 0);
  assert.deepEqual(Object.keys(summary.phase_ms).sort(), ['check_write', 'followImports', 'merge_imports', 'model', 'selection_plan']);
  const durations = requests.map(request => request.duration_ms).sort((a, b) => a - b);
  assert.equal(summary.request_ms.p50, durations[0]);
  assert.equal(summary.request_ms.p90, durations[1]);
  assert.equal(summary.request_ms.max, durations[1]);
  assert(summary.pause_ms > 0);
  assert.equal(summary.average_in_flight, requests.reduce((sum, request) => sum + request.duration_ms, 0) / summary.total_ms);
  assert.equal(summary.max_in_flight, 1);
  assert.equal(summary.rate_limits, 1);
  assert.equal(summary.requests_per_minute, requests.length * 60000 / summary.total_ms);
  const file = performance.find(record => record.type === 'file_end' && record.phase === 'main' && record.file === 'sample.js');
  assert(file.started_at <= file.last_batch_at);
  assert(file.last_batch_at <= file.written_at);
  for (const step of ['parse', 'renderBatch', 'resolveNames', 'applyRenames', 'checkRewrite']) assert(file.cpu_ms[step] > 0, step);
  assert.equal(report.slow_files.length, 2);
  const sample = report.slow_files.find(row => row.file === 'sample.js');
  assert.equal(sample.total_ms, file.at - file.started_at);
  assert.equal(sample.last_batch_at, file.last_batch_at);
  assert.deepEqual(sample.cpu_ms, file.cpu_ms);
  const consumerEnds = performance.filter(record => record.type === 'file_end' && record.file === 'consumer.js');
  assert.equal(consumerEnds.length, 2);
  assert.equal(report.slow_files.find(row => row.file === 'consumer.js').written_at, consumerEnds[0].written_at);
  assert.equal(report.slow_files.find(row => row.file === 'consumer.js').followImports.written_at, consumerEnds[1].written_at);
  assert.equal(sample.followImports, undefined);
  const oneFile = JSON.parse((await execute(process.execPath, [perfScript.pathname, root, '--slow-files', '1'])).stdout);
  assert.equal(oneFile.slow_files.length, 1);
  const previous = report.run_id;
  await run({ paths: [root], codexHome }, dependencies);
  const latest = JSON.parse((await execute(process.execPath, [perfScript.pathname, root])).stdout);
  assert.notEqual(latest.run_id, previous);
  assert.equal(latest.windows.reduce((sum, window) => sum + window.sent, 0), 0);
  const selected = JSON.parse((await execute(process.execPath, [perfScript.pathname, root, '--run-id', previous])).stdout);
  assert.equal(selected.run_id, previous);
  assert.equal(selected.windows.reduce((sum, window) => sum + window.sent, 0), requests.length);
  await assert.rejects(execute(process.execPath, [perfScript.pathname, root, '--run-id', 'missing-run']),
    error => error.code === 1 && /Run not found/.test(error.stderr));
  const perfPath = join(root, '.unminify', 'rename-bindings', 'performance.jsonl');
  const requestsPath = join(root, '.unminify', 'rename-bindings', 'requests.jsonl');
  const interrupted = performance.filter(record => record.type !== 'summary' && record.type !== 'run_end').map(record => JSON.stringify(record)).join('\n') + '\n{';
  const partialRequests = (await readFile(requestsPath, 'utf8')) + '{';
  await writeFile(perfPath, interrupted);
  await writeFile(requestsPath, partialRequests);
  const unfinished = JSON.parse((await execute(process.execPath, [perfScript.pathname, root])).stdout);
  assert.deepEqual(unfinished.phase_ms, summary.phase_ms);
  assert.equal(unfinished.windows.reduce((sum, window) => sum + window.sent, 0), requests.length);
  assert.equal(await readFile(perfPath, 'utf8'), interrupted);
  assert.equal(await readFile(requestsPath, 'utf8'), partialRequests);
  await writeFile(perfPath, interrupted + '\n{}\n');
  await assert.rejects(execute(process.execPath, [perfScript.pathname, root]), error => /Invalid JSON/.test(error.stderr));
});

test('file duration ends at its main write while import writes are reported separately', async t => {
  const { root, summary } = await localRun(t, true);
  const performance = await records(join(root, '.unminify', 'rename-bindings', 'performance.jsonl'));
  const requests = await records(join(root, '.unminify', 'rename-bindings', 'requests.jsonl'));
  assert.equal(requests.filter(row => row.file === 'a-sample.js' && row.status === 200).length, 1);
  assert.equal(requests.filter(row => row.file === 'consumer.js' && row.status === 200).length, 4);
  const ends = performance.filter(row => row.type === 'file_end' && row.file === 'a-sample.js');
  assert.equal(ends.length, 2);
  const [main, imports] = ends;
  assert(imports.written_at > main.written_at);
  assert.match(await readFile(join(root, 'a-sample.js'), 'utf8'), /a as sampleCount3/);
  const report = JSON.parse((await execute(process.execPath, [perfScript.pathname, root])).stdout);
  const sample = report.slow_files.find(row => row.file === 'a-sample.js');
  const mainEndIndex = performance.indexOf(main);
  const mainWriteEndIndex = performance.findIndex(row => row.type === 'phase_end' && row.phase === 'check_write' && row.file === main.file);
  const importsStartIndex = performance.findIndex(row => row.type === 'phase_start' && row.phase === 'followImports');
  assert(mainWriteEndIndex >= 0 && mainWriteEndIndex < mainEndIndex);
  assert(mainEndIndex < importsStartIndex);
  assert.equal(sample.started_at, main.started_at);
  assert.equal(sample.total_ms, main.at - main.started_at);
  assert.equal(sample.written_at, main.written_at);
  assert.equal(main.phase, 'main');
  assert.equal(imports.phase, 'followImports');
  assert.equal(sample.followImports.written_at, imports.written_at);
  assert.equal(report.slow_files.length, 2);
  assert(report.slow_files.every((row, index, rows) => index === 0 || rows[index - 1].total_ms >= row.total_ms));
});

test('perf keeps main durations separate from write cleanup and later import writes', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-perf-records-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const main = { file: 'sample.js', started_at: 1000, last_batch_at: 1700, written_at: 1750, cpu_ms: { parse: 1 } };
  const consumer = { file: 'consumer.js', started_at: 1800, last_batch_at: 1805, written_at: 1815, cpu_ms: { parse: 2 } };
  const events = [
    { type: 'run_start', at: 1000 },
    { type: 'file_end', phase: 'main', ...main, at: 1800 },
    { type: 'file_end', phase: 'main', ...consumer, at: 1820 },
    { type: 'file_end', phase: 'followImports', ...main, written_at: 1980, at: 1990 },
    { type: 'run_end', at: 2000, total_ms: 1000 },
    { type: 'summary', at: 2000, total_ms: 1000, phase_ms: {} },
  ];
  await mkdir(join(root, '.unminify', 'rename-bindings'), { recursive: true });
  await writeFile(join(root, '.unminify', 'rename-bindings', 'performance.jsonl'),
    events.map(event => JSON.stringify({ ...event, run_id: 'main-duration-fixture' })).join('\n') + '\n');
  const report = JSON.parse((await execute(process.execPath, [perfScript.pathname, root])).stdout);
  assert.equal(report.total_ms, 1000);
  assert.deepEqual(report.slow_files, [
    { ...main, total_ms: 800, followImports: { written_at: 1980 } },
    { ...consumer, total_ms: 20 },
  ]);
});
