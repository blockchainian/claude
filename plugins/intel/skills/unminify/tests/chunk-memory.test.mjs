import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import * as packing from '../scripts/pack.mjs';
import { planChunk, renderChunkBatch } from '../scripts/chunks.mjs';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

test('chunk payloads are rendered on demand with identical packing and stable batch IDs', () => {
  const sources = new Map([['module.js', Array.from({ length: 80 }, (_, i) =>
    `const a${i} = ${i}; console.log(a${i});`).join('\n')]]);
  const options = { maxInputTokens: 1000 };
  const batches = planChunk(sources, options);
  let rendered = 0;
  assert.equal(typeof packing.iterateBatches, 'function');
  const iterator = packing.iterateBatches(batches, (batch, placeholders) => {
    rendered++;
    return renderChunkBatch(sources, batch, new Map(), placeholders);
  }, options);
  assert.equal(rendered, 0);
  const first = iterator.next().value;
  assert(first.parts.length > 0);
  assert(rendered < batches.length, 'one request must not render the remaining stage');
  const actual = [first, ...iterator];
  const expected = packing.packBatches(batches,
    (batch, placeholders) => renderChunkBatch(sources, batch, new Map(), placeholders), options);
  assert.deepEqual(actual, expected);
  assert.deepEqual(actual.flatMap(request => request.parts.map(part => part.batch.id)), batches.map(batch => batch.id));
});

test('chunk analyses are bounded independently of request concurrency and completed chunks resume', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-chunk-memory-'));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  t.after(() => rm(root, { recursive: true, force: true }));
  const groups = Array.from({ length: 6 }, (_, i) => ({ id: `group${i}`, files: [`module${i}.js`, `use${i}.js`] }));
  for (const group of groups) {
    await writeFile(join(root, group.files[0]), 'const a = 1;\n');
    await writeFile(join(root, group.files[1]), 'console.log(a);\n');
  }
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: groups }));
  let requests = 0, active = 0, peak = 0;
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw);
    requests++;
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, 'amount'])));
  });
  const dependencies = { ...select, ...rename, ...checks, ...syntax,
    createNamer: options => createNamer({ ...options, ...service }),
    output(line) {
      const event = JSON.parse(line);
      if (event.chunk && event.status === 'planned') { active++; peak = Math.max(peak, active); }
      if (event.chunk && ['renamed', 'rejected'].includes(event.status)) active--;
    } };
  const options = { command: 'rename-bindings', paths: [root], chunks, concurrency: 2 };
  const result = await run(options, dependencies);
  assert.equal(result.exitCode, 0);
  assert.equal(result.completed, 12);
  assert.equal(peak, 1, 'only one chunk may retain its Babel analysis while requests drain');
  assert.equal(active, 0);
  assert.equal(requests, 6);
  const replay = await run(options, dependencies);
  assert.equal(replay.exitCode, 0);
  assert.equal(replay.skipped, 12);
  assert.equal(requests, 6);
});
