import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run, parseArgs } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

const exporter = 'function q() { return 42; }\nexport { q as q };\n';
const head = '// Recovered from consumer.js lines 1-1\n';
const tail = '// Recovered from consumer.js lines 2-3\n';
const importer = 'import { q as w } from "./lib.js";\nw();\n';

async function fixture(t, { ordinary = false, selectedReference = false, external = false,
  ignored = false, conflict = false, unchanged = false } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-sibling-follow-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const files = { 'lib.js': unchanged ? exporter.replaceAll('q', 'calculateAnswer') : exporter };
  if (ordinary) files['app.js'] = importer;
  else {
    files['frag-a.js'] = head + (selectedReference ? 'w();\n' : 'console.log("fragment a");\n');
    files['frag-b.js'] = tail + (external ? 'import { calculateAnswer as w } from "example";\nw();\n'
      : unchanged ? importer.replace('{ q as w }', '{ calculateAnswer as w }') : importer)
      + (conflict ? 'const calculateAnswer = 7; console.log(calculateAnswer);\n' : '');
    files['chunks.json'] = JSON.stringify({ version: 1, chunks: [{ id: 'consumer', files: ['frag-a.js', 'frag-b.js'] }] });
  }
  if (ignored) files['.ignore'] = 'frag-b.js\n';
  for (const [file, source] of Object.entries(files)) await writeFile(join(root, file), source);
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw);
    requests.push(body);
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, 'calculateAnswer'])), requests.length);
  });
  return { files, requests,
    read: file => readFile(join(root, file)),
    async run(paths, { manifest = true } = {}) {
      const result = await run(parseArgs([...paths.map(file => join(root, file)), '--concurrency', '1',
        ...(ordinary || !manifest ? [] : ['--chunks', join(root, 'chunks.json')])]), {
        ...select, ...rename, ...checks, ...syntax,
        createNamer: options => createNamer({ ...options, ...service, retryDelayMs: 1 }), output() {},
      });
      assert.equal(result.exitCode, 0);
      assert.equal(result.rejected, 0);
      assert.equal(result.failedBatches, 0);
    },
  };
}

test('history without a manifest retains recorded sibling authorization', async t => {
  const f = await fixture(t);
  await f.run(['frag-a.js']);
  await f.run(['lib.js'], { manifest: false });
  assert.equal((await f.read('frag-b.js')).toString(), tail + importer.replaceAll('w', 'calculateAnswer'));
});

test('probe scenarios 1 and 2: sibling-only relative imports follow identically together and in batches', async t => {
  const together = await fixture(t), batched = await fixture(t);
  await together.run(['lib.js', 'frag-a.js']);
  await batched.run(['frag-a.js']);
  assert.equal((await batched.read('frag-b.js')).toString(), batched.files['frag-b.js'], 'unchanged exporter does not authorize following');
  await batched.run(['lib.js']);
  for (const file of ['lib.js', 'frag-a.js', 'frag-b.js']) assert.deepEqual(await together.read(file), await batched.read(file), file);
  assert.equal((await together.read('frag-b.js')).toString(), tail + importer.replaceAll('w', 'calculateAnswer'));
  assert.equal(together.requests.length, 1);
  assert.equal(batched.requests.length, 1);
});

test('probe scenario 3: selecting all chunk members still follows', async t => {
  const f = await fixture(t);
  await f.run(['lib.js', 'frag-a.js', 'frag-b.js']);
  assert.equal((await f.read('frag-b.js')).toString(), tail + importer.replaceAll('w', 'calculateAnswer'));
});

test('probe scenario 4: unselected ordinary consumers stay untouched', async t => {
  const f = await fixture(t, { ordinary: true });
  await f.run(['lib.js']);
  assert.equal((await f.read('app.js')).toString(), importer);
});

for (const mode of ['selectedReference', 'external', 'ignored', 'conflict', 'unchanged']) {
  test(`sibling import authorization: ${mode}, current and historical selections`, async t => {
    const options = { [mode]: true };
    const together = await fixture(t, options), batched = await fixture(t, options);
    await together.run(['lib.js', 'frag-a.js']);
    await batched.run(['frag-a.js']);
    await batched.run(['lib.js']);
    for (const file of ['lib.js', 'frag-a.js', 'frag-b.js']) assert.deepEqual(await together.read(file), await batched.read(file), file);
    const source = (await together.read('frag-b.js')).toString();
    if (['ignored', 'unchanged'].includes(mode)) assert.equal(source, together.files['frag-b.js']);
    else if (mode === 'conflict') {
      assert.match(source, /q as calculateAnswer2/);
      assert.match(source, /calculateAnswer2\(\)/);
      assert.match(source, /const calculateAnswer = 7/);
    } else {
      if (mode === 'external') assert.equal(source, tail + 'import { calculateAnswer } from "example";\ncalculateAnswer();\n');
      else assert.match(source, /as calculateAnswer/);
      assert.match(source, /calculateAnswer\(\)/);
      if (mode === 'selectedReference') assert.equal((await together.read('frag-a.js')).toString(), head + 'calculateAnswer();\n');
    }
  });
}
