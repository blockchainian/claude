import test from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runRenameFiles } from '../scripts/rename-files.mjs';
import { appendRecord, readRecords } from '../scripts/journal.mjs';
import { executeMoves } from '../scripts/file-moves.mjs';

function fixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'unminify-file-pipeline-')));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'src'));
  for (const id of ['120048', '718895', '549213']) {
    copyFileSync(new URL(`./fixtures/webpack/${id}.js`, import.meta.url), join(root, 'src', `${id}.js`));
  }
  return root;
}

function options(root, overrides = {}) {
  // An irreducibly small real token budget stops before creating a model client.
  return { paths: [join(root, 'src')], concurrency: 2, maxInputTokens: 1, ...overrides };
}

async function run(root, overrides) {
  const events = [];
  const result = await runRenameFiles(root, options(root, overrides), { output: line => events.push(JSON.parse(line)) });
  return { result, events };
}

function stages(root) {
  return readRecords(join(root, '.unminify', 'rename-files', 'stages.jsonl'));
}

test('selection persists a real bounded graph before an oversized summary fails without requests', async t => {
  const root = fixture(t);
  writeFileSync(join(root, '.ignore'), 'src/549213.js\n');
  const original = readFileSync(join(root, 'src', '120048.js'), 'utf8');
  const { result, events } = await run(root, { paths: [join(root, 'src'), join(root, 'src', '120048.js')] });
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /cannot fit.*token budget/i);
  assert.equal(result.files, 2);
  assert.equal(result.requests, 0);
  assert.equal(result.tokens.total_tokens, 0);
  assert.equal(result.quota, '没测到');
  const snapshot = stages(root).find(record => record.stage === 'graph').value;
  assert.deepEqual(Object.keys(snapshot.sources).sort(), ['src/120048.js', 'src/718895.js']);
  assert.deepEqual(snapshot.graph.edges, [{ source: 'src/120048.js', target: 'src/718895.js' }]);
  assert.equal(readFileSync(join(root, 'src', '120048.js'), 'utf8'), original);
  assert.equal(events.at(-1).type, 'summary');
  assert(existsSync(join(root, '.unminify', 'rename-files', 'performance.jsonl')));
  assert(!existsSync(join(root, '.unminify', 'performance.jsonl')));
  assert(!existsSync(join(root, '.unminify', 'rename-files', 'requests.jsonl')));
  assert(!existsSync(join(root, '.unminify', 'rename-files', 'complete.json')));
});

test('the same interrupted selection resumes its graph without another completed-stage record', async t => {
  const root = fixture(t);
  await run(root);
  const before = stages(root);
  const { result } = await run(root);
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /cannot fit.*token budget/i);
  assert.deepEqual(stages(root), before);
  const events = readRecords(join(root, '.unminify', 'rename-files', 'performance.jsonl'));
  assert.equal(events.filter(record => record.type === 'phase_start' && record.phase === 'graph').length, 1);
  assert.equal(events.filter(record => record.type === 'run_start').length, 2);
  assert.equal(result.requests, 0);
});

test('resuming rejects a changed token budget before a model call', async t => {
  const root = fixture(t);
  await run(root);
  const { result } = await run(root, { maxInputTokens: 2 });
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /options changed/i);
  assert.equal(result.requests, 0);
});

test('resuming rejects a changed selection before a model call', async t => {
  const root = fixture(t);
  await run(root);
  const { result } = await run(root, { paths: [join(root, 'src', '120048.js')] });
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /options changed/i);
  assert.equal(result.requests, 0);
});

test('resuming rejects changed ignore contents at the same path', async t => {
  const root = fixture(t);
  writeFileSync(join(root, '.ignore'), 'src/549213.js\n');
  await run(root);
  writeFileSync(join(root, '.ignore'), 'src/718895.js\n');
  const { result } = await run(root);
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /ignore rules changed/i);
  assert.equal(result.requests, 0);
});

test('explicit chunks do not expand rename-files selection to siblings', async t => {
  const root = fixture(t), manifest = join(root, 'chunks.json');
  writeFileSync(manifest, JSON.stringify({ version: 1, chunks: [{ id: 'recovered', files: ['src/120048.js', 'src/718895.js', 'src/549213.js'] }] }));
  const { result } = await run(root, { paths: [join(root, 'src', '120048.js')], chunks: manifest });
  assert.equal(result.files, 1);
  assert.match(result.reason, /cannot fit.*token budget/i);
  const snapshot = stages(root).find(record => record.stage === 'graph').value;
  assert.deepEqual(Object.keys(snapshot.sources), ['src/120048.js']);
  assert.deepEqual(snapshot.graph.edges, []);
});

test('an entirely ignored selection fails without creating a completion marker', async t => {
  const root = fixture(t);
  writeFileSync(join(root, '.ignore'), 'src/\n');
  const { result } = await run(root);
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /no JavaScript files selected/i);
  assert.equal(result.requests, 0);
  assert(!existsSync(join(root, '.unminify', 'rename-files', 'complete.json')));
});

test('resuming rejects changed chunk membership before consuming saved summaries', async t => {
  const root = fixture(t), manifest = join(root, 'chunks.json');
  writeFileSync(manifest, JSON.stringify({ version: 1, chunks: [{ id: 'recovered', files: ['src/120048.js', 'src/718895.js'] }] }));
  await run(root, { chunks: manifest });
  writeFileSync(manifest, JSON.stringify({ version: 1, chunks: [{ id: 'recovered', files: ['src/120048.js', 'src/549213.js'] }] }));
  const { result } = await run(root, { chunks: manifest });
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /chunk.*changed|changed.*chunk/i);
  assert.equal(result.requests, 0);
});

test('resuming rejects changed source before consuming saved summaries', async t => {
  const root = fixture(t);
  await run(root);
  const path = join(root, 'src', '120048.js');
  writeFileSync(path, readFileSync(path, 'utf8') + '\n');
  const { result } = await run(root);
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /source changed/i);
  assert.equal(result.requests, 0);
});

test('a saved completed move checkpoint still rejects a missing destination before continuing', async t => {
  const root = fixture(t);
  await run(root);
  const snapshot = stages(root).find(record => record.stage === 'graph').value;
  const moves = Object.entries(snapshot.sources).map(([from, source]) => ({
    from, to: from.replace('src/', 'moved/'), source, imports: [],
  }));
  const completed = await executeMoves(root, { moves });
  appendRecord(join(root, '.unminify', 'rename-files', 'stages.jsonl'), { stage: 'moves', value: completed });
  rmSync(join(root, moves[0].to));
  const { result } = await run(root);
  assert.equal(result.exitCode, 1);
  assert.match(result.reason, /source changed/i);
  assert.equal(result.requests, 0);
  assert(!existsSync(join(root, '.unminify', 'rename-files', 'complete.json')));
});

test('directory traversal does not follow an ignored symlink into an unselected directory', async t => {
  const root = fixture(t);
  mkdirSync(join(root, 'lib'));
  copyFileSync(join(root, 'src', '718895.js'), join(root, 'lib', '718895.js'));
  rmSync(join(root, 'src', '718895.js'));
  rmSync(join(root, 'src', '549213.js'));
  symlinkSync('../lib/718895.js', join(root, 'src', 'ignored.js'));
  writeFileSync(join(root, '.ignore'), 'src/ignored.js\n');
  const { result } = await run(root);
  assert.match(result.reason, /cannot fit.*token budget/i);
  assert.equal(result.files, 1);
  const snapshot = stages(root).find(record => record.stage === 'graph').value;
  assert.deepEqual(Object.keys(snapshot.sources), ['src/120048.js']);
  assert.deepEqual(snapshot.graph.edges, []);
  assert.equal(result.requests, 0);
});
