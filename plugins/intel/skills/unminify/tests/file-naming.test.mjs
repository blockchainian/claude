import test from 'node:test';
import assert from 'node:assert/strict';
import { packFileTasks, reserveName } from '../scripts/file-naming.mjs';
import { countTokens } from '../scripts/prompt.mjs';

test('task packing measures instructions and actual rendered JSON', () => {
  const tasks = [{ id: 'a', text: 'one' }, { id: 'b', text: 'two' }];
  const budget = countTokens('instruction') + countTokens(JSON.stringify([tasks[0]]));
  const packs = packFileTasks(tasks, 'instruction', budget);
  assert.deepEqual(packs, [[tasks[0]], [tasks[1]]]);
  assert.throws(() => packFileTasks(tasks, 'instruction', 1), /budget/);
});
test('name reservation retries once before numeric suffix allocation', async () => {
  const occupied = new Set(['wallet', 'wallet2']);
  let retries = 0;
  assert.equal(await reserveName('wallet', occupied, async names => {
    retries++; assert.deepEqual(names, ['wallet', 'wallet2']); return 'wallet';
  }), 'wallet3');
  assert.equal(retries, 1);
  assert(occupied.has('wallet3'));
});
test('unsafe or invalid proposed names fail explicitly', async () => {
  for (const name of ['../wallet', 'wallet/name', '', '.', 'CON', 'wallet.js'])
    await assert.rejects(reserveName(name, new Set(), async () => 'unused'), /Invalid/);
});
