import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { assertLogLayout } from '../scripts/log-layout.mjs';
import { openJournal } from '../scripts/journal.mjs';
import { journalHistory } from '../scripts/history.mjs';
import { parseArgs, report } from '../scripts/perf.mjs';

function workspace(t) {
  const root = mkdtempSync(join(tmpdir(), 'unminify-layout-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test('ordinary naming journals live under their command and history excludes other commands', t => {
  const root = workspace(t);
  const journal = openJournal(root, 'src/source.js', '', {});
  assert.equal(journal.path, join(root, '.unminify/rename-bindings/files/src/source.js.jsonl'));
  openJournal(root, 'ignored.js', '', {}, 'sync-imports');
  assert.deepEqual(journalHistory(root).map(entry => entry.file), ['src/source.js']);
});

test('performance reports select command directories independently', t => {
  const root = workspace(t);
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const command of ['restore-syntax', 'rename-bindings', 'sync-imports']) {
    const directory = join(root, '.unminify', command);
    mkdirSync(directory, { recursive: true });
    writeFileSync(join(directory, 'performance.jsonl'), JSON.stringify({ type: 'run_start', at: 10, run_id: command }) + '\n');
    assert.equal(report({ dir: root, command }).run_id, command);
  }
  assert.equal(parseArgs([root, '--command', 'sync-imports']).command, 'sync-imports');
  assert.throws(() => parseArgs([root, '--command', 'unknown']), /command/);
});

for (const item of ['syntax', 'merge-imports', 'chunks', 'syntax-preflight', 'imports', 'requests.jsonl', 'performance.jsonl', 'src/file.js.jsonl']) {
  test(`all commands reject legacy ${item} without moving logs`, t => {
    const root = workspace(t), target = join(root, '.unminify', item);
    mkdirSync(join(root, '.unminify'));
    if (item.endsWith('.jsonl')) { mkdirSync(join(target, '..'), { recursive: true }); writeFileSync(target, '{}\n'); }
    else mkdirSync(target);
    assert.throws(() => assertLogLayout(root), /migrat/i);
    assert(existsSync(target));
  });
}
