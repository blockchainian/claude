import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { openJournal } from '../scripts/journal.mjs';
import { runSyntaxFile } from '../scripts/syntax/stage.mjs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { mergeExternalImports, checkImportMerge, runMergeFile } from '../scripts/merge-imports.mjs';
import { applyChunkRenames, checkChunkRewrite } from '../scripts/chunks.mjs';
const fragment = (line, body) => `// Recovered from sample.js lines ${line}-${line + 9}\n${body}\n`;

test('header order wins over manifest order; cross-fragment shorthand and exports keep public names', () => {
  const before = new Map([
    ['later.js', fragment(40, 'import {join as b} from "path"; export {b};')],
    ['uses.js', fragment(60, 'const value = {b}; console.log(b("a", "b")); export {b as joined};')],
    ['first.js', fragment(10, 'import {join as a} from "path";')],
  ]);
  const after = mergeExternalImports(before);
  assert.doesNotMatch(after.get('later.js'), /import /);
  assert.match(after.get('later.js'), /export \{a as b\}/);
  assert.match(after.get('uses.js'), /\{b: a\}/);
  assert.match(after.get('uses.js'), /export \{a as joined\}/);
  assert.equal(checkImportMerge(before, after), null);
  const renamed = applyChunkRenames(after, new Map([['a', 'join']]));
  assert.equal(checkChunkRewrite(after, renamed), null);
});

test('default, namespace, string named and mixed declarations merge in a single file', () => {
  const before = new Map([['one.js', 'import a, * as n from "pkg";\nimport b, {x as c, "odd-name" as d} from "pkg";\nimport * as m from "pkg";\nimport {x as e, "odd-name" as f} from "pkg";\nconsole.log(b, m, e, f);']]);
  const after = mergeExternalImports(before);
  assert.match(after.get('one.js'), /console.log\(a, n, c, d\)/);
  assert.match(after.get('one.js'), /import \{x as c, "odd-name" as d\} from "pkg"/);
  assert.equal(checkImportMerge(before, after), null);
  assert.deepEqual(mergeExternalImports(after), after);
});

test('capture anywhere blocks the entire group and local uses stay local', () => {
  const before = new Map([['a.js', fragment(1, 'import {x as a} from "pkg";')], ['b.js', fragment(20, 'import {x as b} from "pkg"; console.log(b); function f(a) { return b; } function g(b) { return b; }')]]);
  assert.deepEqual(mergeExternalImports(before), before);
  const safe = new Map([['a.js', before.get('a.js')], ['b.js', fragment(20, 'import {x as b} from "pkg"; function g(b) { return b; } console.log(b);')]]);
  assert.match(mergeExternalImports(safe).get('b.js'), /function g\(b\) \{ return b; \} console.log\(a\)/);
});

test('load order, relative imports, unrelated bindings and source distinctions survive', () => {
  const before = new Map([['one.js', 'import {x as a} from "pkg"; import "effect"; import {x as b, y} from "pkg"; import {x as c} from "./local.js"; import {x as d} from "./local.js"; import {x as e} from "node:pkg"; const z = {b};']]);
  const after = mergeExternalImports(before);
  assert.match(after.get('one.js'), /import "effect"; import \{y\} from "pkg"/);
  assert.match(after.get('one.js'), /import \{x as d\} from "\.\/local.js"/);
  assert.equal(checkImportMerge(before, after), null);
  for (const [pattern, replacement] of [['{b: a}', '{b: y}'], ['const z', 'const other'], ['import "effect";', ''], ['node:pkg', 'different']]) {
    assert.ok(checkImportMerge(before, new Map([['one.js', after.get('one.js').replace(pattern, replacement)]])));
  }
});

test('selection, ignores, dynamic scope and ambiguous declarations freeze groups', () => {
  const before = new Map([['a.js', fragment(1, 'import {x as a} from "pkg";')], ['b.js', fragment(20, 'import {x as b} from "pkg"; console.log(b);')]]);
  assert.deepEqual(mergeExternalImports(before, { selected: new Set(['b.js']) }), before);
  assert.deepEqual(mergeExternalImports(before, { ignored: new Set(['a.js']) }), before);
  assert.notDeepEqual(mergeExternalImports(before, { selected: new Set(['b.js']), chunkSiblings: true }), before);
  for (const code of ['eval("b");', 'const a = 2;']) {
    assert.deepEqual(mergeExternalImports(new Map([...before, ['c.js', fragment(50, code)]])), new Map([...before, ['c.js', fragment(50, code)]]));
  }
});

test('single-file checkpoint resumes without reapplying and respects rename ownership', () => {
  const root = mkdtempSync(join(tmpdir(), 'merge-imports-'));
  try {
    const source = 'import {x as a} from "pkg"; import {x as b} from "pkg"; console.log(b);';
    writeFileSync(join(root, 'one.js'), source);
    assert.equal(runMergeFile(root, 'one.js').changed, true);
    const merged = readFileSync(join(root, 'one.js'), 'utf8');
    assert.equal(runMergeFile(root, 'one.js').status, 'merge-imports-skipped');
    assert.equal(readFileSync(join(root, 'one.js'), 'utf8'), merged);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('real CLI merges before naming, checkpoints and resumes chunks and standalone files', async () => {
  const { execFileSync } = await import('node:child_process');
  for (const chunked of [false, true]) {
    const root = mkdtempSync(join(tmpdir(), 'merge-cli-'));
    try {
      execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
      const files = chunked ? {
        'z.js': fragment(1, 'import {join as a} from "node:path";'),
        'a.js': fragment(20, 'import {join as b} from "node:path";'),
        'use.js': fragment(40, 'export const result = {b};'),
      } : { 'one.js': 'import {join as a} from "node:path"; import {join as b} from "node:path"; export const result = {b};' };
      for (const [file, code] of Object.entries(files)) writeFileSync(join(root, file), code);
      writeFileSync(join(root, 'chunks.json'), JSON.stringify({ version: 1, chunks: [{ id: 'sample', files: ['a.js', 'use.js', 'z.js'] }] }));
      const cli = new URL('./helpers/run-stages.mjs', import.meta.url);
      const args = [cli.pathname, root, '--min-symbol-length', '1', ...(chunked ? ['--chunks', join(root, 'chunks.json')] : [])];
      const env = { ...process.env, CODEX_HOME: join(root, 'no-login') };
      const first = execFileSync(process.execPath, args, { env, encoding: 'utf8' });
      assert.equal(JSON.parse(first.trim().split('\n').at(-1)).requests, 0);
      if (!chunked) {
        const events = readFileSync(join(root, '.unminify/performance.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
        const started = new Set();
        for (const event of events) {
          if (event.type === 'file_start') started.add(event.file);
          if (event.type === 'phase_start' && event.phase === 'selection_plan' && event.file) {
            assert(started.has(event.file), 'file planning events must start inside the bounded worker');
          }
        }
      }
      const outputs = new Map(Object.keys(files).map(file => [file, readFileSync(join(root, file), 'utf8')]));
      assert.match(outputs.get(chunked ? 'use.js' : 'one.js'), /b: join/);
      if (chunked) {
        assert.doesNotMatch(outputs.get('a.js'), /import /);
        assert.match(outputs.get('z.js'), /import \{join\}/);
      }
      const second = execFileSync(process.execPath, args, { env, encoding: 'utf8' });
      assert.equal(JSON.parse(second.trim().split('\n').at(-1)).requests, 0);
      for (const [file, code] of outputs) assert.equal(readFileSync(join(root, file), 'utf8'), code);
    } finally { rmSync(root, { recursive: true, force: true }); }
  }
});


test('merge resumes a durable single-file intent and protects rename offsets', () => {
  const root = mkdtempSync(join(tmpdir(), 'merge-resume-'));
  try {
    const source = 'import {x as a} from "pkg"; import {x as b} from "pkg"; console.log(b);';
    writeFileSync(join(root, 'one.js'), source);
    const after = mergeExternalImports(new Map([['one.js', source]])).get('one.js');
    const journal = openJournal(root, 'merge-imports/one.js', source, { version: 1 });
    journal.prepare(source, after, 'merge-imports');
    writeFileSync(join(root, 'one.js'), after);
    assert.equal(runMergeFile(root, 'one.js').status, 'merge-imports-skipped');
    writeFileSync(join(root, 'two.js'), source);
    openJournal(root, 'two.js', source, {});
    assert.equal(runMergeFile(root, 'two.js').status, 'merge-imports-skipped');
    assert.equal(readFileSync(join(root, 'two.js'), 'utf8'), source);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('CLI resumes between the syntax, merge and rename checkpoints without a model', async () => {
  const { execFileSync } = await import('node:child_process');
  const root = mkdtempSync(join(tmpdir(), 'merge-checkpoints-'));
  try {
    execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
    writeFileSync(join(root, 'one.js'), 'import {join as a} from "node:path"; import {join as b} from "node:path"; console.log(b);');
    runSyntaxFile(root, 'one.js');
    runMergeFile(root, 'one.js');
    const output = execFileSync(process.execPath, [new URL('./helpers/run-stages.mjs', import.meta.url).pathname, root, '--min-symbol-length', '1'], {
      env: { ...process.env, CODEX_HOME: join(root, 'no-login') }, encoding: 'utf8',
    });
    assert.equal(JSON.parse(output.trim().split('\n').at(-1)).exitCode, 0);
    assert.match(readFileSync(join(root, 'one.js'), 'utf8'), /console.log\(join\)/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('namespace imports differ from a named star export; named default matches default import', () => {
  const before = new Map([['one.js', 'import a, * as n from "pkg"; import {default as b, "*" as star} from "pkg"; console.log(b, star, n);']]);
  const after = mergeExternalImports(before);
  assert.match(after.get('one.js'), /import \{"\*" as star\} from "pkg"/);
  assert.match(after.get('one.js'), /console.log\(a, star, n\)/);
  assert.equal(checkImportMerge(before, after), null);
});
