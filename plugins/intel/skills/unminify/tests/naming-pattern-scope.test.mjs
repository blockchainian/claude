import test from 'node:test';
import assert from 'node:assert/strict';
import { resolveNames, applyRenames } from '../scripts/rename.mjs';
import { checkRewrite } from '../scripts/checks.mjs';
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

test('collision context includes globals in default parameter scopes', () => {
  const source = 'function configure(a = external, b) { return a + b; }';
  const first = source.indexOf('a ='), second = source.indexOf('b)');
  const collisions = [];
  const names = resolveNames(source, new Map([[first, 'count'], [second, 'count']]), {
    onCollision: collision => collisions.push(collision),
  });
  assert.deepEqual(names, new Map([[first, 'count'], [second, 'count2']]));
  assert.equal(collisions.length, 1);
  assert.deepEqual(collisions[0].occupied, ['count', 'external']);
  assert.match(collisions[0].context, /function configure\(a = external, b\)/);
  assert.equal(checkRewrite(source, applyRenames(source, names)), null);
});

test('default parameter globals prevent capture without a collision callback', () => {
  const source = 'function configure(a = external) { return a; }';
  const key = source.indexOf('a =');
  const names = resolveNames(source, new Map([[key, 'external']]));
  assert.equal(names.get(key), 'external2');
  assert.equal(checkRewrite(source, applyRenames(source, names)), null);
});

for (const chunk of [false, true]) test(`default parameter collision completes through the local service (${chunk ? 'chunk' : 'file'})`, async t => {
  const root = await mkdtemp(join(tmpdir(), 'naming-pattern-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const source = 'function configure(a = external, b) { return a + b; }';
  await writeFile(join(root, 'a.js'), source);
  if (chunk) {
    await writeFile(join(root, 'z.js'), 'configure();');
    await writeFile(join(root, 'chunks.json'), JSON.stringify({ version: 1, chunks: [{ id: 'sample', files: ['a.js', 'z.js'] }] }));
  }
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests.push(body);
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, requests.length === 1 ? 'count' : 'limit'])));
  });
  const result = await run({ paths: [join(root, 'a.js')], ...(chunk ? { chunks: join(root, 'chunks.json') } : {}) }, {
    ...select, ...rename, ...checks, output() {},
    createNamer: options => createNamer({ ...options, ...service }),
  });
  assert.equal(result.exitCode, 0);
  assert.equal(result.completed, 1);
  assert.equal(requests.length, 2);
  assert.match(requests[1].input[0].content[0].text, /Occupied names: .*external/);
  const after = await readFile(join(root, 'a.js'), 'utf8');
  assert.equal(after, 'function configure(count = external, limit) { return count + limit; }');
  assert.equal(checkRewrite(source, after), null);
});
