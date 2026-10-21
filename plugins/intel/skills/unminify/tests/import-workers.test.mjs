import test from 'node:test';
import assert from 'node:assert/strict';
import { followImports } from '../scripts/rename.mjs';
import { createSyntaxPool } from '../scripts/syntax/pool.mjs';

const files = new Map([
  ['src/provider.js', 'export const stableValue = 7;\n'],
  ['src/bridge.js', 'export { stableValue as value } from "./provider.js";\n'],
  ['src/consumer.js', 'import { value as x } from "./bridge.js"; import { homedir as h } from "node:os"; console.log(x, h);\n'],
  ['src/independent.js', 'import { basename as b } from "node:path"; const basename = 1; console.log(b, basename);\n'],
]);
// Captured by running followImports against the unchanged 0.71.4 implementation.
const baseline = new Map([
  ['src/consumer.js', 'import { value as stableValue } from "./bridge.js"; import { homedir } from "node:os"; console.log(stableValue, homedir);\n'],
  ['src/independent.js', 'import { basename as pathBasename } from "node:path"; const basename = 1; console.log(pathBasename, basename);\n'],
]);

test('import analysis and edits use multiple workers and preserve the captured output', async t => {
  const events = [];
  const pool = createSyntaxPool({ size: 2, onWorker: event => events.push(event) });
  t.after(() => pool.close());
  assert.deepEqual(await followImports(files, undefined, undefined, undefined, { pool }), baseline);
  assert(new Set(events.filter(event => event.kind === 'import-analyze').map(event => event.threadId)).size > 1);
  assert(events.some(event => event.kind === 'import-resolve'));
  assert(events.some(event => event.kind === 'import-apply'));
  const verdicts = await pool.map([...baseline], ([file, after]) => pool.run({
    kind: 'import-check', before: files.get(file), after,
  }));
  assert.deepEqual(verdicts, [null, null]);
  assert.equal(await pool.run({ kind: 'import-check', before: 'const a = 1;', after: 'const a = 2;' }),
    'STRUCTURE_OR_TEXT_CHANGED');
  const resumed = new Map([...files, ...baseline]);
  assert.deepEqual(await followImports(resumed, undefined, undefined, undefined, { pool }), new Map());
});

test('syntax pool map bounds task construction and retains input order', async t => {
  const pool = createSyntaxPool({ size: 2 });
  t.after(() => pool.close());
  let active = 0, peak = 0;
  const result = await pool.map([0, 1, 2, 3, 4], async index => {
    peak = Math.max(peak, ++active);
    await pool.run({ kind: 'parse', source: `const value${index} = ${index};` });
    active--;
    return index;
  });
  assert.equal(pool.size, 2);
  assert.equal(peak, 2);
  assert.deepEqual(result, [0, 1, 2, 3, 4]);
});

test('syntax pool map drains running file tasks before reporting failure', async t => {
  const pool = createSyntaxPool({ size: 2 });
  t.after(() => pool.close());
  let active = 0;
  await assert.rejects(pool.map([0, 1, 2], async index => {
    active++;
    try {
      return await pool.run(index === 0
        ? { kind: 'import-resolve', source: 'const value = 1;', proposals: new Map([[999, 'missing']]) }
        : { kind: 'parse', source: Array.from({ length: 100 }, (_, i) => `const value${i} = ${i};`).join('\n') });
    } finally { active--; }
  }), /Unknown binding key/);
  assert.equal(active, 0);
});
