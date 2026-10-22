import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { reconsiderNames } from '../scripts/collision.mjs';
import { resolveNames, applyRenames } from '../scripts/rename.mjs';
import { openJournal } from '../scripts/journal.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

// Authored source exercises real binding contexts, scope resolution and durable replies.
const source = `function calculate(){const a=1,b=2,c=3,d=4,e=5,f=6; ${'console.log(a,b,c,d,e,f);'.repeat(24)}} calculate();`;
const keys = [...source.matchAll(/([a-f])=/g)].map(match => match.index);
const initial = () => new Map(keys.map(key => [key, 'count']));
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'collision-budget-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const open = () => openJournal(root, 'module.js', source, {});
  return { root, open, journal: open(), resolve: (values, onCollision) => resolveNames(source, values, { onCollision }) };
}
async function transport(t, f, stopAfter = Infinity) {
  const requests = [], telemetry = [];
  const service = await modelService(t, f.root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); requests.push(body);
    if (requests.length > stopAfter) {
      res.writeHead(429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local quota' } }));
      return;
    }
    complete(res, Object.fromEntries(body.text.format.schema.required.map((key, index) => [key, `choice${requests.length}_${index}`])));
  });
  const namer = createNamer({ ...service, concurrency: 1, log: row => telemetry.push(row) });
  t.after(() => namer.close());
  return { requests, telemetry, namer };
}

test('oversized collision groups split deterministically, retry every binding once and replay across budgets', async t => {
  const outputs = [], ids = [];
  for (let run = 0; run < 2; run++) {
    const f = fixture(t), model = await transport(t, f);
    const result = await reconsiderNames({ ...f, ...model, proposals: initial(), id: 'program', maxInputTokens: 150 });
    assert(model.requests.length > 1, 'the connected collision group must split');
    const records = model.telemetry.filter(row => row.kind === 'model');
    assert(records.every(row => row.estimated_input_tokens <= 150));
    const replies = [...f.journal.replies];
    const retried = replies.flatMap(([, names]) => Object.keys(names));
    assert.equal(retried.length, keys.length - 1);
    assert.equal(new Set(retried).size, retried.length, 'each colliding key is retried once');
    assert(replies.some(([id]) => id.startsWith('collision-group:')));
    assert(replies.some(([id]) => id.startsWith('collision:')));
    const sent = model.requests.length;
    const resumed = await reconsiderNames({ ...f, ...model, journal: f.open(), proposals: initial(), id: 'program', maxInputTokens: 100000 });
    assert.deepEqual(resumed, result);
    assert.equal(model.requests.length, sent, 'saved split groups and singles survive a budget change');
    outputs.push(applyRenames(source, result)); ids.push(replies.map(([id]) => id));
  }
  assert.equal(outputs[0], outputs[1]);
  assert.deepEqual(ids[0], ids[1]);
});

test('a saved whole group replays even when the current budget cannot fit one name', async t => {
  const f = fixture(t), model = await transport(t, f);
  const result = await reconsiderNames({ ...f, ...model, proposals: initial(), id: 'program', maxInputTokens: 100000 });
  assert.equal(model.requests.length, 1);
  const resumed = await reconsiderNames({ ...f, ...model, journal: f.open(), proposals: initial(), id: 'program', maxInputTokens: 1 });
  assert.deepEqual(resumed, result);
  assert.equal(model.requests.length, 1);
});

test('partial split replies survive quota interruption without retrying their keys', async t => {
  const f = fixture(t), model = await transport(t, f, 1);
  await assert.rejects(reconsiderNames({ ...f, ...model, proposals: initial(), id: 'program', maxInputTokens: 150 }), { code: 'USAGE_LIMIT' });
  assert.equal(f.open().replies.size, 1);
  const saved = [...f.open().replies.values()].flatMap(names => Object.keys(names));
  const next = await transport(t, fixture(t));
  const result = await reconsiderNames({ ...f, ...next, journal: f.open(), proposals: initial(), id: 'program', maxInputTokens: 100000 });
  const retried = [...f.open().replies.values()].flatMap(names => Object.keys(names));
  assert.equal(new Set(retried).size, retried.length);
  assert(saved.every(key => retried.includes(key)));
  assert.equal(result.size, keys.length);
});

test('an irreducible single still fails before sending any collision requests', async t => {
  const f = fixture(t), model = await transport(t, f);
  await assert.rejects(reconsiderNames({ ...f, ...model, proposals: initial(), id: 'program', maxInputTokens: 1 }), /Collision batch collision:program:.*cannot fit the input token budget/);
  assert.equal(model.requests.length, 0);
});
