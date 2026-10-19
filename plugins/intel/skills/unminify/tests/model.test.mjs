import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http2';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createNamer } from '../scripts/model.mjs';

async function fixture(t, outputs) {
  const codexHome = await mkdtemp(join(tmpdir(), 'unminify-model-test-'));
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.fake`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    tokens: { access_token: token, refresh_token: 'fake-refresh', account_id: 'fake-account' } }));
  const bodies = [];
  const server = createServer(async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    bodies.push(JSON.parse(raw));
    const output = outputs[Math.min(bodies.length - 1, outputs.length - 1)];
    res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
      output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(output) }] }],
      usage: { input_tokens: 12, output_tokens: 3 } } })}\n\n`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const namer = createNamer({ codexHome, concurrency: 2, baseUrl: `http://127.0.0.1:${server.address().port}`, retryDelayMs: 1 });
  t.after(async () => { await namer.close();
    await new Promise(resolve => server.close(resolve)); await rm(codexHome, { recursive: true, force: true }); });
  return { namer, bodies };
}
const batch = { context: 'class v1 {} const v2 = new v1();', placeholders: { v1: 6, v2: 18 } };

test('naming prompt, fixed settings, dynamic strict schema and return contract', async t => {
  const names = { v1: 'ValueCache', v2: 'cache' };
  const { namer, bodies } = await fixture(t, [names]);
  const result = await namer.name(batch);
  assert.deepEqual(result.names, names); assert.equal(result.usage.input_tokens, 12);
  const body = bodies[0];
  assert.equal(body.model, 'gpt-6-luna'); assert.deepEqual(body.reasoning, { effort: 'low' });
  assert.equal(body.instructions, 'You rename minified JavaScript identifiers. Reply only with JSON.');
  assert.equal(body.input.length, 1); assert.equal(body.input[0].role, 'user');
  const text = body.input[0].content[0].text;
  for (const part of [batch.context, 'v1', 'v2', 'camelCase', 'PascalCase']) assert(text.includes(part));
  assert.equal(body.text.format.strict, true);
  assert.deepEqual(body.text.format.schema, { type: 'object', properties: { v1: { type: 'string' }, v2: { type: 'string' } },
    required: ['v1', 'v2'], additionalProperties: false });
});

test('schema violations retry, then return validated names', async t => {
  const good = { v1: 'ValueCache', v2: 'cache' };
  const { namer, bodies } = await fixture(t, [{ v1: 'ValueCache' }, { ...good, extra: 'bad' }, { ...good, v2: 3 }, good]);
  assert.deepEqual((await namer.name(batch)).names, good);
  assert.equal(bodies.length, 4);
});

test('schema violations fail after three retries', async t => {
  const { namer, bodies } = await fixture(t, [null]);
  await assert.rejects(namer.name(batch), error => error.code === undefined);
  assert.equal(bodies.length, 4);
});
