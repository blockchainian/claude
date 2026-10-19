import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { parse } from '../scripts/node_modules/@babel/parser/lib/index.js';
import traverseModule from '../scripts/node_modules/@babel/traverse/lib/index.js';
import { listFiles, planFile, renderBatch } from '../scripts/select.mjs';
const traverse = traverseModule.default ?? traverseModule;
const source = await readFile(new URL('./fixtures/preact-hooks.mjs', import.meta.url), 'utf8');
const options = { shortLen: 3, maxIds: 60, contextBytes: 8000 };
function bindings(code) {
  const result = new Map();
  traverse(parse(code, { sourceType: 'module' }), { Scope(p) {
    for (const b of Object.values(p.scope.bindings)) result.set(b.identifier.start, b);
  } });
  return result;
}
const allBatches = p => p.stages.flat();
test('a small file keeps all candidates and the complete body in one batch', () => {
  const code = `function a(b, c) {
  const d = [];
  for (let e = 0; e < b.length; e++) {
    if (b[e] > c) d.push(b[e]);
  }
  return d;
}
console.log(a([1, 5, 9], 4));`;
  const plan = planFile(code, options);
  assert.equal(plan.stages.length, 1);
  assert.equal(allBatches(plan).length, 1);
  assert.deepEqual(plan.stages[0][0].keys, [...bindings(code).keys()].sort((a,b)=>a-b));
  const { context, placeholders } = renderBatch(code, plan.stages[0][0], new Map());
  assert.match(context, /for \(let v5 = 0; v5 < v2.length; v5\+\+\)/);
  assert.match(context, /if \(v2\[v5\] > v3\) v4.push\(v2\[v5\]\)/);
  assert.match(context, /return v4;/);
  assert.match(context, /console.log\(v1\(\[1, 5, 9\], 4\)\)/);
  assert.equal(Object.keys(placeholders).length, 5);
  assert.doesNotThrow(()=>parse(context,{sourceType:'module'}));
});
test('large Preact module contexts retain function bodies within the byte budget', () => {
  for (const contextBytes of [8000, 2500]) {
    const plan = planFile(source, { ...options, contextBytes });
    assert.equal(plan.stages.length, 2);
    for (const batch of plan.stages[0]) {
      const { context } = renderBatch(source, batch, new Map());
      assert.ok(Buffer.byteLength(context) <= contextBytes);
      assert.doesNotThrow(()=>parse(context,{sourceType:'module'}));
      const ast = parse(context,{sourceType:'module'});
      let bodyStatements = 0;
      traverse(ast, { FunctionDeclaration(p) { bodyStatements += p.node.body.body.length; } });
      assert.ok(bodyStatements > 0, 'top-level declarations retain implementation statements');
    }
  }
});
test('real Preact artifact: candidates exclude imports and long names; stages partition all bindings', () => {
  const bs = bindings(source);
  const plan = planFile(source, options);
  const actual = allBatches(plan).flatMap(b => b.keys);
  const expected = [...bs].filter(([, b]) => b.kind !== 'module' && b.identifier.name.length <= 3).map(([k]) => k);
  assert.deepEqual(actual.slice().sort((a,b)=>a-b), expected.sort((a,b)=>a-b));
  assert.equal(new Set(actual).size, actual.length);
  assert.deepEqual(planFile(source, options), plan);
  for (const batch of plan.stages[0]) for (const key of batch.keys) assert.equal(bs.get(key).scope.path.type, 'Program');
  assert.ok(plan.stages[1].length > 0);
  const small = planFile(source, { ...options, shortLen: 1 });
  assert.ok(allBatches(small).flatMap(b=>b.keys).every(k=>bs.get(k).identifier.name.length===1));
});
test('candidate and UTF-8 byte boundaries split without duplicates or omissions', () => {
  const base = planFile(source, options);
  for (const overrides of [{ maxIds: 1 }, { maxIds: 2, contextBytes: 220 }, { contextBytes: 100 }]) {
    const settings = { ...options, ...overrides };
    const plan = planFile(source, settings);
    assert.deepEqual(allBatches(plan).flatMap(b=>b.keys).sort((a,b)=>a-b), allBatches(base).flatMap(b=>b.keys).sort((a,b)=>a-b));
    assert.ok(allBatches(plan).every(b=>b.keys.length<=settings.maxIds));
    assert.ok(allBatches(plan).length > allBatches(base).length);
    for (const b of allBatches(plan)) assert.doesNotThrow(()=>parse(renderBatch(source,b,new Map()).context,{sourceType:'module'}));
    assert.deepEqual(plan, planFile(source, settings));
  }
});
test('same-name bindings get distinct placeholders mapped to original UTF-16 keys', () => {
  const unicode = '// 非 ASCII 😀\n' + source;
  const bs = bindings(unicode);
  let checked = false;
  for (const batch of allBatches(planFile(unicode, options))) {
    const rendered = renderBatch(unicode, batch, new Map());
    assert.deepEqual(Object.values(rendered.placeholders), batch.keys);
    assert.doesNotThrow(()=>parse(rendered.context,{sourceType:'module'}));
    const names = batch.keys.map(k=>bs.get(k).identifier.name);
    if (new Set(names).size === names.length) continue;
    checked = true;
    for (const [placeholder, key] of Object.entries(rendered.placeholders)) {
      assert.match(rendered.context, new RegExp(`\\b${placeholder}\\b`));
      assert.equal(unicode.slice(key,key+bs.get(key).identifier.name.length),bs.get(key).identifier.name);
    }
  }
  assert.ok(checked);
});
test('render applies resolved outer names before replacing batch names', () => {
  const plan = planFile(source, options);
  const bs = bindings(source);
  const renames = new Map(plan.stages[0].flatMap(b=>b.keys).map(k=>[k,`resolved_${bs.get(k).identifier.name}`]));
  assert.ok(plan.stages[1].some(b=>renderBatch(source,b,renames).context.includes('resolved_')));
});
test('ignore uses repository root and gitignore directory, glob, negation, comments', async () => {
  const root = await mkdtemp(path.join(tmpdir(),'unminify-select-'));
  try {
    execFileSync('git',['init',root],{stdio:'ignore'});
    for (const file of ['src/keep.js','src/drop.min.js','src/allow.min.mjs','src/vendor/deep.js','src/bad.js','src/text.txt']) {
      await mkdir(path.dirname(path.join(root,file)),{recursive:true});
      await writeFile(path.join(root,file),file.endsWith('bad.js')?'const =':source);
    }
    await writeFile(path.join(root,'.ignore'),'# comment\nvendor/\n*.min.*\n!allow.min.mjs\n');
    const selected = await listFiles(path.join(root,'src'),{});
    assert.deepEqual(selected,['src/allow.min.mjs','src/keep.js']);
    await writeFile(path.join(root,'src/custom.ignore'),'*.js\n!keep.js\n');
    assert.deepEqual(await listFiles(path.join(root,'src'),{ignoreFile:path.join(root,'src/custom.ignore')}),['src/allow.min.mjs','src/keep.js']);
  } finally { await rm(root,{recursive:true,force:true}); }
});

test('render handles shorthand, destructuring, exports and preexisting placeholder names', () => {
  const code = 'const v1=1; const { a: x, b = v1 } = input; function f({ e }) { let t=e; return { e, t, v1 }; } export { x };';
  for (const batch of allBatches(planFile(code,options))) {
    const {context,placeholders} = renderBatch(code,JSON.parse(JSON.stringify(batch)),new Map());
    assert.doesNotThrow(()=>parse(context,{sourceType:'module'}));
    for (const name of Object.keys(placeholders)) assert.match(context,new RegExp(`\\b${name}\\b`));
    if (batch.keys.some(k=>code.slice(k,k+1)==='e')) {
      assert.match(context,/e: v\d/);
      assert.match(context,/t: v\d/);
    }
  }
});
test('module and function ranges starting at zero remain distinguishable', () => {
  const code = 'function f(e) { return e; }';
  const plan = planFile(code,{...options,maxIds:1});
  assert.equal(plan.stages[0].length,1);
  assert.equal(plan.stages[1].length,1);
  assert.notEqual(plan.stages[0][0].id,plan.stages[1][0].id);
  for (const batch of allBatches(plan)) {
    const {context,placeholders}=renderBatch(code,batch,new Map());
    for (const placeholder of Object.keys(placeholders)) assert.match(context,new RegExp(`\\b${placeholder}\\b`));
    assert.doesNotThrow(()=>parse(context,{sourceType:'module'}));
  }
});
test('budget contexts retain every selected binding, including classes and nested methods', () => {
  const code = source + '\nclass Short { #value=1; method(e) { const t=this.#value; return e+t; } }';
  for (const batch of [120,8000].flatMap(contextBytes=>allBatches(planFile(code,{...options,maxIds:1,contextBytes})))) {
    const {context,placeholders}=renderBatch(code,batch,new Map());
    assert.doesNotThrow(()=>parse(context,{sourceType:'module'}));
    for (const placeholder of Object.keys(placeholders)) assert.match(context,new RegExp(`\\b${placeholder}\\b`));
  }
});
test('resolved names cannot collide with intermediate names during rendering', () => {
  const code = 'let a=1,b=2;function f(e){return a+b+e}';
  const bs = bindings(code);
  const key = name => [...bs].find(([,b])=>b.identifier.name===name)[0];
  const batch = planFile(code,{...options,maxIds:2}).stages[1][0];
  const {context}=renderBatch(code,batch,new Map([[key('a'),'_binding2'],[key('b'),'_binding']]));
  assert.match(context,/_binding2 \+ _binding \+ v1/);
});
test('a real function fits exactly at both planning boundaries', () => {
  let functionSource;
  traverse(parse(source,{sourceType:'module'}),{ FunctionDeclaration(p) {
    if (!functionSource && p.node.params.length >= 2 && p.node.end-p.node.start<250) functionSource=source.slice(p.node.start,p.node.end);
  } });
  assert.ok(functionSource);
  const count=bindings(functionSource).size;
  const budget=Buffer.byteLength(functionSource);
  const exact=planFile(functionSource,{...options,maxIds:count,contextBytes:budget});
  assert.equal(exact.stages.length,1);
  assert.equal(allBatches(exact).length,1);
  assert.ok(allBatches(planFile(functionSource,{...options,maxIds:count-1,contextBytes:budget})).length>1);
  assert.ok(allBatches(planFile(functionSource,{...options,maxIds:count,contextBytes:budget-1})).length>1);
});
