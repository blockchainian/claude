import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const scripts = new URL('../scripts/', import.meta.url).href;

test('all refreshes completed worker history when an unfinished selection expands', { timeout: 20000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'worker-selection-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await exec('git', ['init', '--initial-branch=main', root]);
  await writeFile(join(root, 'left.js'), 'import { basename as b } from "node:path";\nconst a = 7;\n'
    + `/*${'padding '.repeat(34000)}*/\n`);
  await writeFile(join(root, 'right.js'), 'function helper(q) { return q + a + b("/tmp/name").length; }\nglobalThis.workerSelectionResult = helper(2);\n');
  await writeFile(join(root, 'chunks.json'), JSON.stringify({ version: 1,
    chunks: [{ id: 'shared', files: ['left.js', 'right.js'] }] }));
  const probe = `
    import assert from 'node:assert/strict';
    import { readFileSync } from 'node:fs';
    import { join } from 'node:path';
    import { createHash } from 'node:crypto';
    import { run } from ${JSON.stringify(scripts + 'cli.mjs')};
    import { createChunkRun } from ${JSON.stringify(scripts + 'chunk-run.mjs')};
    import * as select from ${JSON.stringify(scripts + 'select.mjs')};
    import * as rename from ${JSON.stringify(scripts + 'rename.mjs')};
    import * as checks from ${JSON.stringify(scripts + 'checks.mjs')};
    import * as syntax from ${JSON.stringify(scripts + 'syntax/stage.mjs')};
    const root = process.argv[1];
    const settings = { command: 'all', chunks: join(root, 'chunks.json'), chunkSiblings: true,
      concurrency: 2, minSymbolLength: 4, maxInputTokens: 100000 };
    const api = { ...select, ...rename, ...checks, ...syntax };
    const driver = await createChunkRun(root, settings);
    driver.select(['left.js'], new Set());
    await assert.rejects(driver.process(driver.jobs[0], { api,
      namer: { async name() { throw Object.assign(new Error('auth interruption'), { code: 'AUTH' }); } },
      withSlot: task => task(), emit() {}, phase: () => () => {}, sources: new Map(),
      summary: { completed: 0, skipped: 0, rejected: 0, failedBatches: 0 },
    }), { code: 'AUTH' });
    let sequence = 0;
    const events = [];
    const result = await run({ ...settings, paths: [join(root, 'right.js')] }, { ...api,
      output(line) { events.push(JSON.parse(line)); },
      async createNamer() { return { close() {}, async name(payload) {
        return { names: Object.fromEntries(Object.keys(payload.placeholders).map(key => [key, 'meaningful' + ++sequence])) };
      } }; },
    });
    assert.equal(result.exitCode, 0);
    const journal = readFileSync(join(root, '.unminify', 'chunks',
      createHash('sha256').update('shared').digest('hex') + '.jsonl'), 'utf8').trim().split('\\n').map(JSON.parse);
    assert.deepEqual(journal.findLast(record => record.type === 'chunk-selection').files, ['left.js', 'right.js']);
    const left = readFileSync(join(root, 'left.js'), 'utf8');
    const right = readFileSync(join(root, 'right.js'), 'utf8');
    assert.match(left, /basename/);
    assert.doesNotMatch(right, /\\bb\\(/);
    assert.doesNotMatch(right, /helper\\(q\\)/);
    await import('data:text/javascript;base64,' + Buffer.from(left + '\\n' + right).toString('base64'));
    assert.equal(globalThis.workerSelectionResult, 13);
    const planned = events.filter(event => event.status === 'planned');
    assert.equal(planned.length, 2);
    assert(events.some(event => event.status === 'cpu-timing' && event.threadId > 0));
    console.log(JSON.stringify({ passed: true, selection: ['left.js', 'right.js'], requests: sequence }));
  `;
  const { stdout } = await exec(process.execPath, ['--input-type=module', '--eval', probe, root],
    { timeout: 15000, killSignal: 'SIGKILL', maxBuffer: 1024 * 1024 });
  assert.equal(JSON.parse(stdout.trim()).passed, true);
});

test('all preserves a rejected worker journal and its external import alias', { timeout: 20000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), 'worker-rejected-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await exec('git', ['init', '--initial-branch=main', root]);
  const left = 'import { basename as b } from "node:path";\nconst a = 7;\n'
    + `/*${'padding '.repeat(34000)}*/\n`;
  const right = 'globalThis.rejectedResult = b("/tmp/name").length + a;\n';
  await writeFile(join(root, 'left.js'), left);
  await writeFile(join(root, 'right.js'), right);
  await writeFile(join(root, 'chunks.json'), JSON.stringify({ version: 1,
    chunks: [{ id: 'shared', files: ['left.js', 'right.js'] }] }));
  const probe = `
    import assert from 'node:assert/strict';
    import { readFileSync } from 'node:fs';
    import { join } from 'node:path';
    import { run } from ${JSON.stringify(scripts + 'cli.mjs')};
    import { loadChunks } from ${JSON.stringify(scripts + 'chunk-manifest.mjs')};
    import { openChunkJournal } from ${JSON.stringify(scripts + 'chunk-journal.mjs')};
    import { syntaxFingerprint } from ${JSON.stringify(scripts + 'syntax/stage.mjs')};
    import { importMergeVersion } from ${JSON.stringify(scripts + 'merge-imports.mjs')};
    import * as select from ${JSON.stringify(scripts + 'select.mjs')};
    import * as rename from ${JSON.stringify(scripts + 'rename.mjs')};
    import * as checks from ${JSON.stringify(scripts + 'checks.mjs')};
    import * as syntax from ${JSON.stringify(scripts + 'syntax/stage.mjs')};
    const root = process.argv[1];
    const settings = { command: 'all', chunks: join(root, 'chunks.json'), chunkSiblings: true,
      concurrency: 2, minSymbolLength: 4, maxInputTokens: 100000 };
    const manifest = await loadChunks(settings.chunks, root);
    const inputs = new Map(['left.js', 'right.js'].map(file => [file, readFileSync(join(root, file), 'utf8')]));
    const journal = openChunkJournal(root, 'shared', inputs, {
      manifest: manifest.fingerprint, ignored: [], chunkSiblings: true,
      minSymbolLength: 4, maxInputTokens: 100000, syntax: syntaxFingerprint, mergeImports: importMergeVersion,
    });
    journal.begin(['left.js', 'right.js']);
    journal.complete(inputs, 'rejected', 'CHUNK_BINDING_CHANGED');
    const journalBefore = readFileSync(journal.path, 'utf8');
    const events = [];
    const result = await run({ ...settings, paths: [root] }, { ...select, ...rename, ...checks, ...syntax,
      output(line) { events.push(JSON.parse(line)); },
      async createNamer() { return { close() {}, async name() { throw new Error('rejected journal requested a model'); } }; },
    });
    assert.equal(result.exitCode, 1);
    assert.equal(result.rejected, 2);
    assert.equal(readFileSync(journal.path, 'utf8'), journalBefore);
    for (const [file, source] of inputs) assert.equal(readFileSync(join(root, file), 'utf8'), source);
    assert(events.some(event => event.chunk === 'shared' && event.status === 'skipped' && event.previous === 'rejected'));
    assert(events.some(event => event.status === 'cpu-timing' && event.threadId > 0));
    assert(!events.some(event => event.chunk === 'shared' && event.status === 'imports'));
    console.log(JSON.stringify({ passed: true }));
  `;
  const { stdout } = await exec(process.execPath, ['--input-type=module', '--eval', probe, root],
    { timeout: 15000, killSignal: 'SIGKILL', maxBuffer: 1024 * 1024 });
  assert.equal(JSON.parse(stdout.trim()).passed, true);
});
