import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { reconsiderNames } from '../scripts/collision.mjs';
import { resolveNames, applyRenames } from '../scripts/rename.mjs';
import { openJournal } from '../scripts/journal.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

const source = 'function first(){const a=1,b=2,c=3; console.log(a,b,c);} function second(){const d=4,e=5,f=6; console.log(d,e,f);} first(); second();';
const keys = [...source.matchAll(/([a-f])=/g)].map(match => match.index);
const initial = () => new Map(keys.map((key, index) => [key, index < 3 ? 'count' : 'limit']));
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'collision-rounds-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const open = () => openJournal(root, 'a.js', source, {});
  return { root, open, journal: open(), resolve: (values, onCollision) => resolveNames(source, values, { onCollision }) };
}
async function serviceFor(t, root, reverse = false, stop = false, concurrency = 20) {
  let active = 0, maximum = 0;
  const requests = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), text = body.input[0].content[0].text;
    const count = text.includes('The name count is occupied');
    requests.push(body); maximum = Math.max(maximum, ++active);
    await delay(count === reverse ? 15 : 60);
    active--;
    if (stop && count) {
      res.writeHead(429, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { code: 'usage_limit_reached', message: 'local quota stop' } }));
    } else complete(res, Object.fromEntries(body.text.format.schema.required.map((key, index) => [key, `${count ? 'count' : 'limit'}Choice${index}`])));
  });
  const namer = createNamer({ ...service, concurrency });
  t.after(() => namer.close());
  return { namer, requests, maximum: () => maximum };
}

test('rounds group related collisions and run independent groups concurrently with durable deterministic replies', async t => {
  const outputs = [], ids = [];
  for (const reverse of [false, true]) {
    const f = fixture(t), model = await serviceFor(t, f.root, reverse);
    const options = { ...f, proposals: initial(), id: 'program', namer: model.namer, maxInputTokens: 4000 };
    const result = await reconsiderNames(options);
    assert.equal(model.requests.length, 2, 'two independent collision groups');
    assert.equal(model.maximum(), 2, 'independent groups must overlap');
    for (const request of model.requests) {
      assert.equal(request.text.format.schema.required.length, 2, 'related placeholders share one request');
      assert.match(request.input[0].content[0].text, /(?:mutually|each other|pairwise|one another|distinct from one another)/i);
    }
    outputs.push(applyRenames(source, result));
    ids.push([...f.journal.replies.keys()]);
    const resumed = await reconsiderNames({ ...options, journal: f.open(), proposals: initial() });
    assert.deepEqual(resumed, result);
    assert.equal(model.requests.length, 2, 'durable group replies must not be resent');
  }
  assert.equal(outputs[0], outputs[1], 'arrival order must not change output');
  assert.deepEqual(ids[0], ids[1], 'group IDs and journal order must be stable');
});

test('legacy single-name replies replay in resolver order without any requests', async t => {
  const f = fixture(t), proposals = initial();
  const attempted = new Set();
  for (;;) {
    const collisions = [];
    f.resolve(proposals, item => collisions.push(item));
    const collision = collisions.find(item => !attempted.has(item.key));
    if (!collision) break;
    attempted.add(collision.key);
    const name = `choice${collision.key}`;
    f.journal.response(`collision:program:${collision.key}`, new Map([[collision.key, name]]));
    proposals.set(collision.key, name);
  }
  const model = await serviceFor(t, f.root);
  const result = await reconsiderNames({ ...f, journal: f.open(), proposals: initial(), id: 'program', namer: model.namer, maxInputTokens: 4000 });
  assert.equal(model.requests.length, 0);
  assert.deepEqual(result, f.resolve(proposals));
});

test('a quota stop drains and saves other accepted group replies for resume', async t => {
  const f = fixture(t), model = await serviceFor(t, f.root, false, true);
  await assert.rejects(reconsiderNames({ ...f, proposals: initial(), id: 'program', namer: model.namer, maxInputTokens: 4000 }), { code: 'USAGE_LIMIT' });
  assert.equal(model.requests.length, 2);
  assert.equal(f.open().replies.size, 1, 'accepted in-flight group survives another group stopping');
});

test('all group input budgets are checked before sending requests', async t => {
  const f = fixture(t), model = await serviceFor(t, f.root);
  await assert.rejects(reconsiderNames({ ...f, proposals: initial(), id: 'program', namer: model.namer, maxInputTokens: 1 }), /cannot fit the input token budget/);
  assert.equal(model.requests.length, 0);
});

test('different proposed names that can occupy one another share a request', async t => {
  const f = fixture(t);
  const text = 'const a=1,b=2,c=3,d=4; console.log(a,b,c,d);';
  const positions = [...text.matchAll(/([a-d])=/g)].map(match => match.index);
  const journal = openJournal(f.root, 'dependent.js', text, {});
  const received = [];
  const service = await modelService(t, f.root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); received.push(body);
    complete(res, Object.fromEntries(body.text.format.schema.required.map((key, index) => [key, `replacement${index}`])));
  });
  const namer = createNamer(service); t.after(() => namer.close());
  const result = await reconsiderNames({ journal, proposals: new Map(positions.map((key, index) => [key, index < 2 ? 'count' : 'limit'])),
    id: 'dependent', namer, maxInputTokens: 4000,
    resolve: (values, onCollision) => resolveNames(text, values, { onCollision }) });
  assert.equal(received.length, 1);
  assert.equal(received[0].text.format.schema.required.length, 2);
  assert.equal(result.get(positions[1]), 'replacement0');
  assert.equal(result.get(positions[3]), 'replacement1');
});

test('unsuccessful replacements are attempted once and retain deterministic suffix fallback', async t => {
  const f = fixture(t), received = [];
  const service = await modelService(t, f.root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw); received.push(body);
    const name = body.input[0].content[0].text.includes('The name count is occupied') ? 'count' : 'limit';
    complete(res, Object.fromEntries(body.text.format.schema.required.map(key => [key, name])));
  });
  const namer = createNamer(service); t.after(() => namer.close());
  const result = await reconsiderNames({ ...f, proposals: initial(), id: 'fallback', namer, maxInputTokens: 4000 });
  assert.equal(received.length, 2);
  assert.equal(result.get(keys[1]), 'count2');
  assert.equal(result.get(keys[2]), 'count3');
  assert.equal(result.get(keys[4]), 'limit2');
  assert.equal(result.get(keys[5]), 'limit3');
  const replayed = await reconsiderNames({ ...f, journal: f.open(), proposals: initial(), id: 'fallback', namer, maxInputTokens: 4000 });
  assert.deepEqual(replayed, result);
  assert.equal(received.length, 2);
});

test('authentication stop saves accepted peer replies without contacting an external service', async t => {
  const f = fixture(t);
  let requests = 0;
  const service = await modelService(t, f.root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    if (req.url === '/token') {
      res.writeHead(400, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'invalid_grant' }));
      return;
    }
    const body = JSON.parse(raw); requests++;
    if (body.input[0].content[0].text.includes('The name count is occupied')) {
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'local auth stop' } }));
      return;
    }
    await delay(50);
    complete(res, Object.fromEntries(body.text.format.schema.required.map((key, index) => [key, `limitChoice${index}`])));
  });
  const namer = createNamer({ ...service, refreshUrl: `${service.baseUrl}/token` });
  t.after(() => namer.close());
  await assert.rejects(reconsiderNames({ ...f, proposals: initial(), id: 'auth', namer, maxInputTokens: 4000 }), { code: 'AUTH' });
  assert.equal(requests, 2);
  assert.equal(f.open().replies.size, 1);
});

test('independent collision groups obey the shared request limit', async t => {
  const f = fixture(t), model = await serviceFor(t, f.root, false, false, 1);
  await reconsiderNames({ ...f, proposals: initial(), id: 'bounded', namer: model.namer, maxInputTokens: 4000 });
  assert.equal(model.requests.length, 2);
  assert.equal(model.maximum(), 1);
});

test('quota stop prevents a queued collision group from sending', async t => {
  const f = fixture(t), model = await serviceFor(t, f.root, false, true, 1);
  await assert.rejects(reconsiderNames({ ...f, proposals: initial(), id: 'queued', namer: model.namer, maxInputTokens: 4000 }), { code: 'USAGE_LIMIT' });
  assert.equal(model.requests.length, 1);
  assert.equal(model.maximum(), 1);
  assert.equal(f.open().replies.size, 0);
});
