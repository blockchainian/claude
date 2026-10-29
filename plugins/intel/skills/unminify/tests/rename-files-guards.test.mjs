import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { assertLogLayout } from '../scripts/log-layout.mjs';
import { parseArgs } from '../scripts/cli.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'unminify-guards-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, '.unminify'));
  return root;
}
for (const item of ['syntax', 'merge-imports', 'chunks', 'syntax-preflight', 'imports', 'requests.jsonl', 'performance.jsonl', 'src/file.js.jsonl']) {
  test(`all commands reject legacy ${item} without moving logs`, t => {
    const root = fixture(t), target = join(root, '.unminify', item);
    if (item.endsWith('.jsonl')) { mkdirSync(join(target, '..'), { recursive: true }); writeFileSync(target, '{}\n'); }
    else mkdirSync(target);
    for (const command of ['rename-files', 'restore-syntax', 'rename-bindings', 'sync-imports']) {
      assert.throws(() => assertLogLayout(root, command), /migrat/i);
      assert(existsSync(target));
    }
  });
}
for (const command of ['restore-syntax', 'rename-bindings', 'sync-imports']) {
  test(`rename-files refuses existing ${command}`, t => {
    const root = fixture(t); mkdirSync(join(root, '.unminify', command));
    assert.throws(() => assertLogLayout(root, 'rename-files'), /rename-files.*before/i);
  });
  test(`${command} refuses unfinished rename-files`, t => {
    const root = fixture(t); mkdirSync(join(root, '.unminify', 'rename-files'));
    assert.throws(() => assertLogLayout(root, command), /rename-files.*incomplete/i);
    writeFileSync(join(root, '.unminify', 'rename-files', 'complete.json'), JSON.stringify({ complete: true }));
    assert.doesNotThrow(() => assertLogLayout(root, command));
  });
}
test('rename-files accepts naming budgets and rejects binding length', () => {
  const options = parseArgs(['rename-files', 'src', '--concurrency', '2', '--max-input-tokens', '300']);
  assert.equal(options.command, 'rename-files');
  assert.equal(options.concurrency, 2);
  assert.throws(() => parseArgs(['rename-files', 'src', '--min-symbol-length', '4']), /invalid option/);
});
