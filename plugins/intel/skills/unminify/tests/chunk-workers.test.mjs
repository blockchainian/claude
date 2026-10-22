import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runChunkWorkers } from '../scripts/chunk-workers.mjs';
import { createRequire } from 'node:module';

const { parse } = createRequire(new URL('../scripts/select.mjs', import.meta.url))('@babel/parser');

test('worker dependency graph excludes the model client', async () => {
  const seen = new Set();
  async function inspect(url) {
    if (seen.has(url.href)) return;
    seen.add(url.href);
    assert(!url.pathname.endsWith('/model.mjs'), 'workers must delegate model calls to the main thread');
    const ast = parse(await readFile(url, 'utf8'), { sourceType: 'module' });
    for (const node of ast.program.body) {
      if (!['ImportDeclaration', 'ExportNamedDeclaration', 'ExportAllDeclaration'].includes(node.type)) continue;
      const specifier = node.source?.value;
      if (specifier?.startsWith('.') && specifier.endsWith('.mjs')) await inspect(new URL(specifier, url));
    }
  }
  await inspect(new URL('../scripts/chunk-worker.mjs', import.meta.url));
});

async function fixture(t, count = 2) {
  const root = await mkdtemp(join(tmpdir(), 'chunk-workers-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const jobs = Array.from({ length: count }, (_, i) => ({ id: `group${i}`, files: [`declare${i}.js`, `use${i}.js`] }));
  for (const job of jobs) {
    await writeFile(join(root, job.files[0]), 'const a = 7;\n');
    await writeFile(join(root, job.files[1]), 'globalThis.result = a + 2;\n');
  }
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: jobs }));
  return { root, jobs, settings: { command: 'rename-bindings', chunks, chunkSiblings: true,
    minSymbolLength: 4, maxInputTokens: 100000, concurrency: 2 } };
}

function context(data, name) {
  const events = [], phases = [], summary = { completed: 0, skipped: 0, rejected: 0, failedBatches: 0 };
  const sources = new Map();
  return { ...data, selected: new Set(data.jobs.flatMap(job => job.files)), namer: { name },
    withSlot: task => task(), emit: event => events.push(event),
    phase: (phase, file) => { phases.push(['start', phase, file]); return () => phases.push(['end', phase, file]); },
    summary, sources, events, phases, size: 2 };
}

test('chunks execute in separate workers, preserve semantics, and resume without requests', async t => {
  const data = await fixture(t);
  let requests = 0;
  const options = context(data, async payload => {
    requests++;
    return { names: Object.fromEntries(Object.keys(payload.placeholders).map(key => [key, 'amount'])) };
  });
  const results = await runChunkWorkers(data.jobs, options);
  assert.equal(new Set(results.map(result => result.threadId)).size, 2);
  assert(results.every(result => result.threadId > 0));
  assert.equal(options.summary.completed, 4);
  assert.equal(requests, 2);
  assert.equal(options.sources.size, 4);
  assert.equal(options.events.filter(event => event.status === 'planned').length, 2);
  assert.equal(options.events.filter(event => event.status === 'renamed').length, 2);
  assert.equal(options.phases.filter(([kind]) => kind === 'start').length,
    options.phases.filter(([kind]) => kind === 'end').length);
  for (const job of data.jobs) {
    const code = (await Promise.all(job.files.map(file => readFile(join(data.root, file), 'utf8')))).join('\n');
    assert.match(code, /const amount = 7/);
    assert.equal(Function(`${code}\nreturn globalThis.result;`)(), 9);
  }
  const replay = context(data, async () => { throw new Error('completed journal requested model'); });
  await runChunkWorkers(data.jobs, replay);
  assert.equal(replay.summary.skipped, 4);
  assert.equal(replay.summary.completed, 0);
});

test('fatal model errors retain their code and stop queued chunks', async t => {
  const data = await fixture(t, 3);
  const options = context(data, async () => { throw Object.assign(new Error('offline auth failure'), { code: 'AUTH' }); });
  options.size = 1;
  await assert.rejects(runChunkWorkers(data.jobs, options), { message: 'offline auth failure', code: 'AUTH' });
  assert.equal(options.events.filter(event => event.status === 'planned').length, 1);
  assert.equal(await readFile(join(data.root, data.jobs[1].files[0]), 'utf8'), 'const a = 7;\n');
});

test('fatal workers drain running chunks and global request slots remain bounded', async t => {
  const data = await fixture(t, 3);
  let active = 0, peak = 0, drained = false;
  let release;
  const bothStarted = new Promise(resolve => { release = resolve; });
  const options = context(data, async (payload, metadata) => {
    active++;
    peak = Math.max(peak, active);
    if (metadata.chunk === 'group1') release();
    try {
      if (metadata.chunk === 'group0') {
        await bothStarted;
        throw Object.assign(new Error('budget exhausted'), { code: 'USAGE_LIMIT' });
      }
      await new Promise(resolve => setTimeout(resolve, 50));
      drained = true;
      return { names: Object.fromEntries(Object.keys(payload.placeholders).map(key => [key, 'amount'])) };
    } finally { active--; }
  });
  await assert.rejects(runChunkWorkers(data.jobs, options), { code: 'USAGE_LIMIT' });
  assert.equal(drained, true);
  assert.equal(active, 0);
  assert.equal(peak, 2);
  assert.equal(options.summary.completed, 2);
  assert.equal(options.events.filter(event => event.status === 'planned').length, 2);
  assert.match(await readFile(join(data.root, data.jobs[1].files[0]), 'utf8'), /const amount = 7/);
  assert.equal(await readFile(join(data.root, data.jobs[2].files[0]), 'utf8'), 'const a = 7;\n');
});
