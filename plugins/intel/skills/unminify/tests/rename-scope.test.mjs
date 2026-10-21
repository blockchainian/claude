import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';
import { createNamer } from '../scripts/model.mjs';
import { modelService, complete } from './helpers/model-service.mjs';

async function fixture(t, siblingBindings = true) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-rename-scope-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const files = {
    'selected.js': 'const a = 101;\nfunction selectedWorker(b) { const c = b + a; return c; }\n',
    'sibling.js': siblingBindings
      ? 'const d = 202;\nfunction siblingWorker(e) { const f = e + d; return f + a; }\n'
      : 'console.log(a);\n',
    'last.js': siblingBindings
      ? 'const g = 303;\nfunction anotherWorker(h) { return h + g; }\nconsole.log(a);\n'
      : 'console.log(a);\n',
  };
  for (const [file, source] of Object.entries(files)) await writeFile(join(root, file), source);
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'authored', files: Object.keys(files) }] }));
  const requests = [], symbols = [];
  const service = await modelService(t, root, async (req, res) => {
    let raw = ''; for await (const part of req) raw += part;
    const body = JSON.parse(raw), context = body.input[0].content[0].text;
    requests.push(body);
    const names = Object.fromEntries(body.text.format.schema.required.map(key => {
      let symbol, name;
      if (new RegExp(`const ${key} = 101`).test(context)) [symbol, name] = ['a', 'selectedValue'];
      else if (new RegExp(`selectedWorker\\(${key}\\)`).test(context)) [symbol, name] = ['b', 'inputValue'];
      else if (new RegExp(`const ${key} = \\w+ \\+ (?:a|selectedValue)`).test(context)) [symbol, name] = ['c', 'totalValue'];
      else [symbol, name] = [`sibling:${key}`, `siblingValue${key}`];
      symbols.push(symbol);
      return [key, name];
    }));
    complete(res, names, requests.length);
  });
  return { root, files, requests, symbols,
    read: file => readFile(join(root, file), 'utf8'),
    run: command => run({ command, paths: [join(root, 'selected.js')], chunks, concurrency: 1 }, {
      ...select, ...rename, ...checks, ...syntax,
      createNamer: options => createNamer({ ...options, ...service, retryDelayMs: 1 }),
      output() {},
    }),
  };
}

test('rename-bindings names only selected declarations and follows their sibling references', async t => {
  const f = await fixture(t);
  assert.equal((await f.run('restore-syntax')).exitCode, 0);
  const result = await f.run('rename-bindings');
  assert.equal(result.exitCode, 0);
  assert.equal(result.rejected, 0);
  assert.deepEqual([...f.symbols].sort(), ['a', 'b', 'c']);
  assert.equal(await f.read('selected.js'), 'const selectedValue = 101;\nfunction selectedWorker(inputValue) { const totalValue = inputValue + selectedValue; return totalValue; }\n');
  assert.equal(await f.read('sibling.js'), f.files['sibling.js'].replace('f + a', 'f + selectedValue'));
  assert.equal(await f.read('last.js'), f.files['last.js'].replace('console.log(a)', 'console.log(selectedValue)'));
  const count = f.requests.length;
  assert.equal((await f.run('rename-bindings')).exitCode, 0);
  assert.equal(f.requests.length, count);
});

test('rename-bindings HTTP request count matches a chunk containing only selected declarations', async t => {
  const selectedOnly = await fixture(t, false), withSiblings = await fixture(t);
  for (const f of [selectedOnly, withSiblings]) {
    assert.equal((await f.run('restore-syntax')).exitCode, 0);
    assert.equal((await f.run('rename-bindings')).exitCode, 0);
  }
  assert.deepEqual([...selectedOnly.symbols].sort(), ['a', 'b', 'c']);
  assert.equal(selectedOnly.requests.length, 2, 'one program request and one function request');
  assert.equal(withSiblings.requests.length, selectedOnly.requests.length);
});
