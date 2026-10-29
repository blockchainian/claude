import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync, spawn } from 'node:child_process';
import { parse } from '../scripts/node_modules/@babel/parser/lib/index.js';
import { checkRewrite } from '../scripts/checks.mjs';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { followImports } from '../scripts/rename.mjs';
import { hash } from '../scripts/journal.mjs';

// These small modules are authored here; the real service supplies every proposed name.
const samples = {
  'totals.mjs': 'export function a(b) { const c = b.reduce((d, e) => d + e.price * e.quantity, 0); return c; }\nexport const f = !0;\n',
  'receipt.mjs': "import { a as x, f as y } from './totals.mjs';\nexport function z(q) { const r = x(q); return { total: r, enabled: y }; }\n",
  'main.mjs': "import { z as n } from './receipt.mjs';\nconst o = [{ price: 7, quantity: 3 }];\nexport const p = n(o);\n",
};
const records = text => text.split('\n').filter(Boolean).map(line => JSON.parse(line));
const tree = source => parse(source, { sourceType: 'module' });
function exportNames(source) {
  const names = [];
  for (const node of tree(source).program.body) {
    if (node.type === 'ExportDefaultDeclaration') names.push('default');
    if (node.type !== 'ExportNamedDeclaration') continue;
    for (const specifier of node.specifiers) names.push(specifier.exported.name ?? specifier.exported.value);
    if (node.declaration?.id) names.push(node.declaration.id.name);
    for (const declaration of node.declaration?.declarations ?? []) names.push(declaration.id.name);
  }
  return names.sort();
}
function localExport(source, name) {
  for (const node of tree(source).program.body) {
    if (node.type !== 'ExportNamedDeclaration') continue;
    for (const specifier of node.specifiers) {
      if ((specifier.exported.name ?? specifier.exported.value) === name) return specifier.local.name;
    }
    if (node.declaration?.id?.name === name) return name;
    if (node.declaration?.declarations?.some(declaration => declaration.id.name === name)) return name;
  }
  assert.fail(`missing export ${name}`);
}

function invoke(root) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [fileURLToPath(new URL('./helpers/run-stages.mjs', import.meta.url)), root, '--concurrency', '2'],
      { stdio: ['ignore', 'pipe', 'pipe'], timeout: 240_000 });
    let stdout = '', stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({ code, signal, stdout, stderr }));
  });
}

test('real Codex service completes syntax, scope-safe naming and import following', {
  skip: process.env.UNMINIFY_E2E !== '1' && 'set UNMINIFY_E2E=1 to use the current Codex login',
  timeout: 270_000,
}, async t => {
  const root = await mkdtemp(join(tmpdir(), 'unminify-live-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  await writeFile(join(root, 'package.json'), '{"type":"module"}\n');
  await Promise.all(Object.entries(samples).map(([file, source]) => writeFile(join(root, file), source)));
  const result = await invoke(root);
  assert.equal(result.signal, null, 'the real CLI completes before its deadline');
  assert.equal(result.code, 0, `${result.stderr}\n${result.stdout}`);
  const events = records(result.stdout);
  const summary = events.find(record => record.type === 'summary');
  assert.ok(summary, 'CLI emits its actual model usage');
  assert.equal(summary.files, 3);
  assert.equal(summary.completed, 3);
  assert.equal(summary.rejected, 0);
  assert.equal(summary.failedBatches, 0);
  assert.ok(summary.requests > 0);
  assert.ok(summary.tokens.input_tokens > 0);
  assert.ok(summary.tokens.output_tokens > 0);
  const outputs = new Map();
  for (const [file, source] of Object.entries(samples)) {
    const after = await readFile(join(root, file), 'utf8');
    const baseline = transformSyntax(source).source;
    outputs.set(file, after);
    assert.doesNotThrow(() => tree(after), `${file} remains parseable`);
    assert.equal(checkRewrite(baseline, after), null, `${file} preserves scopes and module contracts`);
    assert.notEqual(after, baseline, `${file} contains actual model-proposed renames`);
    assert.deepEqual(exportNames(after), exportNames(source), `${file} retains public export names`);
    const syntax = events.find(record => record.file === file && record.status === 'syntax');
    assert.ok(syntax, `${file} passed through the real syntax stage`);
    const syntaxJournal = records(await readFile(join(root, '.unminify', 'restore-syntax', 'syntax', `${file}.syntax.jsonl`), 'utf8'));
    assert.equal(syntaxJournal.at(-1).status, 'syntax');
    assert.equal(syntaxJournal.at(-1).hash, hash(baseline));
    const journal = records(await readFile(join(root, '.unminify', 'rename-bindings', 'files', `${file}.jsonl`), 'utf8'));
    assert.ok(journal.some(record => record.id && Object.keys(record.names).length > 0), `${file} persists resolved model names`);
    assert.equal(journal.at(-1).type, 'done');
    assert.equal(journal.at(-1).hash, hash(after));
  }
  assert.ok(events.find(record => record.file === 'totals.mjs' && record.status === 'syntax').changed,
    'the authored !0 expression exercises a real syntax rewrite');
  for (const [file, provider, exported] of [['receipt.mjs', 'totals.mjs', 'a'], ['main.mjs', 'receipt.mjs', 'z']]) {
    const declaration = tree(outputs.get(file)).program.body.find(node => node.type === 'ImportDeclaration');
    const imported = declaration.specifiers.find(specifier => specifier.imported?.name === exported);
    assert.equal(imported.local.name, localExport(outputs.get(provider), exported), `${file} follows the provider binding name`);
  }
  assert.equal(followImports(outputs).size, 0, 'import following has reached its fixed point');
  const module = await import(pathToFileURL(join(root, 'main.mjs')).href);
  assert.deepEqual(module.p, { total: 21, enabled: true }, 'the rewritten modules execute together');
  t.diagnostic(JSON.stringify({ requests: summary.requests, tokens: summary.tokens, completed: summary.completed, total_ms: summary.total_ms }));
});
