import test from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { analyzeChunk, resolveChunkNames, applyChunkRenames, checkChunkRewrite, planChunk, renderChunkBatch } from '../scripts/chunks.mjs';
import { estimateInputTokens } from '../scripts/prompt.mjs';

const sources = () => new Map([
  ['decl.js', 'const a = 7; function b(x) { return a + x; }'],
  ['use.js', 'console.log(a, b(2), {a}); export { a, b as run };'],
]);
test('chunk declarations, free references, shorthand and public exports rename together', () => {
  const before = sources();
  const names = resolveChunkNames(before, new Map([['a', 'amount'], ['b', 'calculate']]));
  const after = applyChunkRenames(before, names);
  assert.match(after.get('decl.js'), /const amount = 7/);
  assert.match(after.get('use.js'), /amount, calculate\(2\), \{a: amount\}/);
  assert.match(after.get('use.js'), /export \{ amount as a, calculate as run \}/);
  assert.equal(checkChunkRewrite(before, after), null);
});
test('duplicate declarations, ignored declarations and readers, known globals freeze', () => {
  const files = new Map([['a.js', 'const a = 1; const b = 2; const c = 3; const NaN = 0;'], ['b.js', 'const a = 4; console.log(b);'], ['ignored.js', 'console.log(c);']]);
  const analysis = analyzeChunk(files, { ignored: new Set(['ignored.js']) });
  assert.equal(analysis.frozen.get('a'), 'duplicate');
  assert.equal(analysis.frozen.get('c'), 'ignored');
  assert.equal(analysis.frozen.get('NaN'), 'knownGlobal');
  assert.deepEqual([...resolveChunkNames(files, new Map([['a','first'],['c','third']]), {ignored:new Set(['ignored.js'])})], []);
});
test('default selection freezes sibling readers and extended mode includes them', () => {
  const selected = new Set(['decl.js']);
  assert.equal(resolveChunkNames(sources(), new Map([['a','amount']]), { selected }).size, 0);
  assert.equal(resolveChunkNames(sources(), new Map([['a','amount']]), { selected, chunkSiblings: true }).get('a'), 'amount');
});
test('import local names follow explicit proposals and propagate to sibling readers', () => {
  const files = new Map([['a.js','import { value as a } from "./other.js";'],['b.js','console.log(a); export { a };']]);
  assert.equal(analyzeChunk(files).candidates.length, 0);
  const names = resolveChunkNames(files,new Map([['a','amount']]),{allowImports:true});
  const after=applyChunkRenames(files,names);
  assert.match(after.get('a.js'),/value as amount/);
  assert.match(after.get('b.js'),/console.log\(amount\)/);
  assert.equal(checkChunkRewrite(files,after),null);
});
test('candidate avoids chunk bindings, lexical capture and used globals', () => {
  const files=new Map([['a.js','const a=1;'],['b.js','function use(amount){ return a + amount; } console.log(a, amount2);']]);
  assert.equal(resolveChunkNames(files,new Map([['a','amount']])).get('a'),'amount3');
});
test('chunk check rejects changed global identity, capture, public exports and bytes', () => {
  const before=sources(); const valid=applyChunkRenames(before,new Map([['a','amount']]));
  for (const source of ['console.log(amount, b(2), {a: amount}); export { amount as amount, b as run };','console.log(amount + 1, b(2), {a: amount}); export { amount as a, b as run };']) {
    const after=new Map(valid); after.set('use.js',source); assert.ok(checkChunkRewrite(before,after));
  }
  const capture=new Map([['a.js','const a=1;'],['b.js','function f(value){ return a; }']]);
  assert.ok(checkChunkRewrite(capture,applyChunkRenames(capture,new Map([['a','value']]))));
});
test('direct export declarations retain external names',()=>{
  const before=new Map([['a.js','export const a=1;'],['b.js','console.log(a);']]);
  const after=applyChunkRenames(before,new Map([['a','amount']]));
  assert.match(after.get('a.js'),/export \{ amount as a \}/);
  assert.equal(checkChunkRewrite(before,after),null);
});
test('planning includes declaration and sibling uses under actual token budget',()=>{
  const before=sources(); const batches=planChunk(before,{maxInputTokens:500});
  const keys=batches.flatMap(batch=>batch.keys); assert.deepEqual(keys,['a','b']);
  for(const batch of batches){const rendered=renderChunkBatch(before,batch); assert.match(rendered.context,/decl.js/); assert.match(rendered.context,/use.js/); assert.ok(estimateInputTokens(rendered)<=500);}
});
test('identical short names in separate manifest chunks remain independent',()=>{
  const first=new Map([['a.js','const a=1;'],['b.js','console.log(a);']]);
  const second=new Map([['c.js','const a=2;'],['d.js','console.log(a);']]);
  const changed=applyChunkRenames(first,resolveChunkNames(first,new Map([['a','firstAmount']])));
  assert.match(changed.get('a.js'),/firstAmount/);
  assert.equal(second.get('c.js'),'const a=2;');
  assert.equal(resolveChunkNames(second,new Map([['a','secondAmount']])).get('a'),'secondAmount');
});
test('ignored declarations freeze and nested shadow bindings stay untouched',()=>{
  const before=new Map([['a.js','const a=1;'],['b.js','function f(a){ return a; } console.log(a);']]);
  assert.equal(analyzeChunk(before,{ignored:new Set(['a.js'])}).frozen.get('a'),'ignored');
  const after=applyChunkRenames(before,new Map([['a','amount']]));
  assert.equal(after.get('b.js'),'function f(a){ return a; } console.log(amount);');
  assert.equal(checkChunkRewrite(before,after),null);
});
test('chunk guard permits checked function-local renames after the chunk pass',()=>{
  const before=new Map([['a.js','const a=1; function f(x){return a+x;}'],['b.js','console.log(a);']]);
  const after=applyChunkRenames(before,new Map([['a','amount']]));
  after.set('a.js',after.get('a.js').replace('f(x)','f(input)').replace('+x','+input'));
  assert.equal(checkChunkRewrite(before,after),null);
});
test('chunk guard permits a local rename reusing a program binding original name',()=>{
  const before=new Map([['a.js','const a=1; function f(x){return a+x;}']]);
  const after=new Map([['a.js','const amount=1; function f(a){return amount+a;}']]);
  assert.equal(checkChunkRewrite(before,after),null);
});
test('builtin globals freeze with the measurement global tables',()=>{
  const files=new Map([['a.js','const parseInt=1; const JSON=2; const Promise=3;']]);
  const analysis=analyzeChunk(files);
  for(const name of ['parseInt','JSON','Promise']) assert.equal(analysis.frozen.get(name),'knownGlobal');
});
test('extended imports can rename an outside declaration used by selected sibling',()=>{
  const files=new Map([['imports.js','import { value as a } from "./other.js";'],['selected.js','console.log(a);']]);
  const options={selected:new Set(['selected.js']),chunkSiblings:true};
  assert.equal(analyzeChunk(files,options).candidates.length,0);
  assert.equal(resolveChunkNames(files,new Map([['a','amount']]),options).size,0);
  assert.equal(resolveChunkNames(files,new Map([['a','amount']]),{...options,allowImports:true}).get('a'),'amount');
  assert.equal(resolveChunkNames(files,new Map([['a','amount']]),{...options,chunkSiblings:false,allowImports:true}).size,0);
});
test('chunk guard rejects an unbound export alias retargeted to another binding',()=>{
  const before=new Map([['decl.js','const a=1; const b=2;'],['export.js','export { a as value, b as second };']]);
  const after=applyChunkRenames(before,new Map([['a','amount'],['b','count']]));
  after.set('export.js','export { count as value, amount as second };');
  assert.ok(checkChunkRewrite(before,after));
});
test('chunk guard rejects a direct declaration export interface change',()=>{
  const before=new Map([['decl.js','export const a=1;']]);
  const after=new Map([['decl.js','export const amount=1;']]);
  assert.ok(checkChunkRewrite(before,after));
});
test('direct eval freezes the whole chunk including import following', () => {
  const before = new Map([['a.js', 'const a = 7;'], ['b.js', 'globalThis.answer = eval("a");']]);
  const analysis = analyzeChunk(before);
  assert.equal(analysis.frozen.get('a'), 'dynamicScope');
  assert.equal(analysis.statistics.dynamicScope, 1);
  assert.equal(resolveChunkNames(before, new Map([['a', 'amount']]), { allowImports: true }).size, 0);
  assert.ok(checkChunkRewrite(before, applyChunkRenames(before, new Map([['a', 'amount']]))));
  assert.equal(checkChunkRewrite(before, before), null);
  const after = applyChunkRenames(before, resolveChunkNames(before, new Map([['a', 'amount']])));
  const context = {};
  runInNewContext([...after.values()].join('\n'), context);
  assert.equal(context.answer, 7);
});
test('dynamic scope rejects function-local binding renames as well', () => {
  const before = new Map([['a.js', 'function f(a) { return eval("a"); }']]);
  const after = new Map([['a.js', 'function f(amount) { return eval("a"); }']]);
  assert.ok(checkChunkRewrite(before, after));
});
test('with freezes chunk bindings and explicit import proposals', () => {
  const before = new Map([['a.js', 'const a = 7; with (holder) { console.log(a); }']]);
  assert.equal(analyzeChunk(before).frozen.get('a'), 'dynamicScope');
  assert.equal(resolveChunkNames(before, new Map([['a', 'amount']]), { allowImports: true }).size, 0);
  assert.ok(checkChunkRewrite(before, applyChunkRenames(before, new Map([['a', 'amount']]))));
});
test('shadowed eval and indirect eval do not freeze the chunk', () => {
  for (const source of ['function f(eval) { return eval("a"); }', '(0, eval)("a");']) {
    const before = new Map([['a.js', 'const a = 7;'], ['b.js', source]]);
    assert.equal(resolveChunkNames(before, new Map([['a', 'amount']])).get('a'), 'amount');
  }
});

test('direct exported declaration comments survive shared-name edits', () => {
  const before = new Map([['a.js', 'export /* keep */ const a = 1;'], ['b.js', 'console.log(a);']]);
  const after = applyChunkRenames(before, new Map([['a', 'amount']]));
  assert.match(after.get('a.js'), /\/\* keep \*\//);
  assert.equal(checkChunkRewrite(before, after), null);
});
test('a direct declaration export can coexist with a sibling unbound export', () => {
  const before = new Map([['a.js', 'export const a = 1; export { b };'], ['b.js', 'const b = 2; console.log(a);']]);
  const after = applyChunkRenames(before, new Map([['a', 'amount'], ['b', 'count']]));
  assert.equal(checkChunkRewrite(before, after), null);
});
