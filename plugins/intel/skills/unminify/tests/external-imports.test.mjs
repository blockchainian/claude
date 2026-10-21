import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { planFile } from '../scripts/select.mjs';
import { followImports, resolveNames, applyRenames } from '../scripts/rename.mjs';
import { analyzeChunk, resolveChunkNames, applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';
import { checkRewrite } from '../scripts/checks.mjs';
import { run } from '../scripts/cli.mjs';

const source = 'import d from "node:path"; import * as ns from "node:os"; import { homedir as b, e as x } from "os"; console.log(d, ns, b, x);';

test('file candidates include short default and namespace imports but no named imports', () => {
  const keys = planFile(source).stages.flat().flatMap(batch => batch.keys);
  assert.deepEqual(keys, [source.indexOf('d from'), source.indexOf('ns from')]);
  const names = resolveNames(source, new Map(keys.map((key, i) => [key, ['pathTools', 'operatingSystem'][i]])));
  const after = applyRenames(source, names);
  assert.match(after, /import pathTools from/);
  assert.match(after, /import \* as operatingSystem from/);
  assert.equal(checkRewrite(source, after), null);
  assert.equal(planFile(source, { minSymbolLength: 1 }).stages.flat().length, 0);
});

test('chunk model candidates and resolvers accept default and namespace imports', () => {
  const before = new Map([['imports.js', source], ['uses.js', 'console.log(d, ns);']]);
  assert.deepEqual(analyzeChunk(before).candidates.map(record => record.name), ['d', 'ns']);
  const names = resolveChunkNames(before, new Map([['d', 'pathTools'], ['ns', 'operatingSystem']]));
  assert.deepEqual([...names], [['d', 'pathTools'], ['ns', 'operatingSystem']]);
  const after = applyChunkRenames(before, names);
  assert.equal(after.get('uses.js'), 'console.log(pathTools, operatingSystem);');
  assert.equal(checkChunkRewrite(before, after), null);
});

test('external named aliases restore long imported names and retain short imported names', () => {
  const files = new Map([['main.js', 'import { homedir as b, e as x } from "os"; console.log(b, x);']]);
  const changes = followImports(files);
  assert.equal(changes.get('main.js'), 'import { homedir, e as x } from "os"; console.log(homedir, x);');
  assert.equal(checkRewrite(files.get('main.js'), changes.get('main.js')), null);
  assert.equal(followImports(changes).size, 0);
  assert.equal(followImports(files, undefined, undefined, undefined, { minSymbolLength: 8 }).size, 0);
});

test('external named alias collisions use the existing scope resolver', () => {
  const files = new Map([['main.js', 'import { homedir as b } from "os"; const homedir = 1; console.log(b, homedir);']]);
  const after = followImports(files).get('main.js');
  assert.equal(after, 'import { homedir as homedir2 } from "os"; const homedir = 1; console.log(homedir2, homedir);');
  assert.equal(checkRewrite(files.get('main.js'), after), null);
});

for (const chunk of [false, true]) for (const minSymbolLength of [4, 8]) {
  test(`CLI ${chunk ? 'chunk' : 'file'} external aliases honor threshold ${minSymbolLength} without model requests`, async t => {
    const root = await mkdtemp(join(tmpdir(), 'unminify-external-'));
    t.after(() => rm(root, { recursive: true, force: true }));
    execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
    const input = 'import { homedir as b, e as x } from "os"; console.log(b, x);\n';
    await writeFile(join(root, 'imports.js'), input);
    const options = { paths: [root], minSymbolLength };
    if (chunk) {
      await writeFile(join(root, 'uses.js'), 'console.log(b);\n');
      options.chunks = join(root, 'chunks.json');
      await writeFile(options.chunks, JSON.stringify({ version: 1, chunks: [{ id: 'queue', files: ['imports.js', 'uses.js'] }] }));
    }
    const result = await run(options);
    assert.equal(result.exitCode, 0);
    assert.equal(result.requests, 0);
    assert.equal(await readFile(join(root, 'imports.js'), 'utf8'), minSymbolLength === 4
      ? 'import { homedir, e as x } from "os"; console.log(homedir, x);\n' : input);
    if (chunk) assert.equal(await readFile(join(root, 'uses.js'), 'utf8'), minSymbolLength === 4 ? 'console.log(homedir);\n' : 'console.log(b);\n');
    assert.equal((await run(options)).exitCode, 0);
  });
}

test('restoring external aliases preserves block and line comments', () => {
  const before = 'import { homedir /* as marker */ as b, platform // as marker\n as p } from "node:os"; console.log(b, p);';
  const after = followImports(new Map([['main.js', before]])).get('main.js');
  assert.ok(after.includes('/* as marker */'));
  assert.ok(after.includes('// as marker\n'));
  assert.equal(checkRewrite(before, after), null);
});

test('chunk collision suffixes reach a fixed point when restoring external aliases', async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-external-collision-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'imports.js'), 'import { homedir as b } from "os";\n');
  await writeFile(join(root, 'uses.js'), 'const homedir = 1; console.log(b, homedir);\n');
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: [{ id: 'queue', files: ['imports.js', 'uses.js'] }] }));
  const options = { paths: [root], chunks };
  assert.equal((await run(options)).exitCode, 0);
  const expected = 'import { homedir as homedir2 } from "os";\n';
  assert.equal(await readFile(join(root, 'imports.js'), 'utf8'), expected);
  assert.equal(await readFile(join(root, 'uses.js'), 'utf8'), 'const homedir = 1; console.log(homedir2, homedir);\n');
  assert.equal((await run(options)).exitCode, 0);
  assert.equal(await readFile(join(root, 'imports.js'), 'utf8'), expected);
});
