import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { followImports, resolveNames, applyRenames } from '../scripts/rename.mjs';
import { checkRewrite } from '../scripts/checks.mjs';
import { analyzeChunk, resolveChunkNames, applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';
import { run } from '../scripts/cli.mjs';

// Authored examples: no upstream application source is used as a fixture.
const imports = 'import { join as pathAlias } from "path"; import { join as posixAlias } from "node:path/posix"; import { basename as winAlias, isAbsolute as absoluteAlias } from "path/win32"; import { constants as fsAlias } from "fs"; import { constants as osAlias } from "node:os"; import { constants as bufferAlias } from "buffer"; import { setImmediate as timerAlias } from "timers/promises";';
const uses = 'const basename = 1; const isAbsolute = 2; console.log(pathAlias, posixAlias, winAlias, absoluteAlias, fsAlias, osAlias, bufferAlias, timerAlias, setImmediate);';
const expected = ['join', 'posixJoin', 'win32Basename', 'win32IsAbsolute', 'constants', 'osConstants', 'bufferConstants', 'promisesSetImmediate'];

for (const chunk of [false, true]) test(`${chunk ? 'chunk' : 'file'} external collisions use source prefixes and reach a fixed point`, async t => {
  const root = await mkdtemp(join(tmpdir(), 'import-prefix-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  const before = chunk ? new Map([['imports.js', imports], ['uses.js', uses]]) : new Map([['main.js', `${imports}\n${uses}`]]);
  for (const [file, source] of before) await writeFile(join(root, file), source);
  const options = { paths: [root], minSymbolLength: 1 };
  if (chunk) {
    options.chunks = join(root, 'chunks.json');
    await writeFile(options.chunks, JSON.stringify({ version: 1, chunks: [{ id: 'authored', files: [...before.keys()] }] }));
  }
  const result = await run(options);
  assert.equal(result.exitCode, 0);
  assert.equal(result.requests, 0);
  const after = new Map();
  for (const file of before.keys()) after.set(file, await readFile(join(root, file), 'utf8'));
  const declaration = after.get(chunk ? 'imports.js' : 'main.js');
  for (const name of expected) assert.match(declaration, new RegExp(`\\b${name}\\b`));
  assert.match(after.get(chunk ? 'uses.js' : 'main.js'), new RegExp(`console.log\\(${expected.join(', ')}, setImmediate\\)`));
  assert.equal(chunk ? checkChunkRewrite(before, after) : checkRewrite(before.get('main.js'), after.get('main.js')), null);
  assert.equal((await run(options)).requests, 0);
  for (const [file, source] of after) assert.equal(await readFile(join(root, file), 'utf8'), source);
});

for (const [label, source, pattern] of [
  ['scoped camel case', 'import { readFile as alias } from "@scope/file-tools"; const readFile = 1; console.log(alias);', /readFile as fileToolsReadFile/],
  ['reserved source segment', 'import { readFile as alias } from "@scope/default"; const readFile = 1; console.log(alias);', /readFile as defaultReadFile/],
  ['underscore camel case', 'import { readFile as alias } from "file_tools"; const readFile = 1; console.log(alias);', /readFile as fileToolsReadFile/],
  ['prefix occupied', 'import { homedir as alias } from "os"; const homedir = 1, osHomedir = 2; console.log(alias);', /homedir as osHomedir2/],
  ['reference capture', 'import { homedir as alias } from "os"; function read(homedir, osHomedir) { return alias; }', /homedir as osHomedir2/],
  ['unused nested name', 'import { homedir as alias } from "os"; function read(homedir) { return homedir; } console.log(alias);', /import \{ homedir \}/],
  ['free global', 'import { homedir as alias } from "node:os"; console.log(alias, homedir);', /homedir as osHomedir/],
]) test(label, () => {
  const after = followImports(new Map([['main.js', source]])).get('main.js');
  assert.match(after, pattern);
  assert.equal(checkRewrite(source, after), null);
  assert.equal(followImports(new Map([['main.js', after]])).size, 0);
  const chunk = new Map([['main.js', source]]);
  const record = analyzeChunk(chunk).bindings.get('alias');
  const imported = record.declarations[0].binding.path.node.imported.name;
  const names = resolveChunkNames(chunk, new Map([['alias', imported]]), { allowImports: true, restoreExternalImports: true });
  const rewritten = applyChunkRenames(chunk, names);
  assert.match(rewritten.get('main.js'), pattern);
  assert.equal(checkChunkRewrite(chunk, rewritten), null);
});

test('model and internal import collisions retain numeric suffixes', () => {
  const source = 'import { homedir as alias } from "os"; const homedir = 1; console.log(alias);';
  assert.match(applyRenames(source, resolveNames(source, new Map([[source.indexOf('alias'), 'homedir']]))), /homedir as homedir2/);
  const files = new Map([['dep.js', 'const homedir = 1; export { homedir as value };'], ['main.js', 'import { value as alias } from "./dep.js"; const homedir = 2; console.log(alias);']]);
  assert.match(followImports(files).get('main.js'), /value as homedir2/);
});

for (const chunked of [false, true]) test(`${chunked ? 'chunk' : 'file'} repeated imports of the same identity keep numeric suffixes`, () => {
  const source = 'import { basename as firstAlias, basename as secondAlias, basename as thirdAlias } from "path"; console.log(firstAlias, secondAlias, thirdAlias);';
  const before = new Map([['imports.js', source]]);
  const after = chunked
    ? applyChunkRenames(before, resolveChunkNames(before, new Map([['firstAlias', 'basename'], ['secondAlias', 'basename'], ['thirdAlias', 'basename']]), { allowImports: true, restoreExternalImports: true }))
    : new Map([...before, ...followImports(before)]);
  assert.equal(after.get('imports.js'), 'import { basename, basename as basename2, basename as basename3 } from "path"; console.log(basename, basename2, basename3);');
  assert.equal(checkChunkRewrite(before, after), null);
});
