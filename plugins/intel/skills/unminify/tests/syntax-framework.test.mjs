// Ported from wakaru v1.14.0 (Apache-2.0); framework integration tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { checkSyntaxRewrite } from '../scripts/syntax/checks.mjs';
import { runSyntaxFile } from '../scripts/syntax/stage.mjs';
import { openJournal, atomicWrite } from '../scripts/journal.mjs';
const swap = (name, from, to) => ({ name, visitor({replace}) { return { NumericLiteral(p) { if(p.node.value===from) replace(p,String(to)); } }; } });
test('ordered registry permits repeat passes with freshly parsed offsets', () => {
  const a=swap('First',7,8),b=swap('Second',8,7);
  const result=transformSyntax('// unchanged\r\nuse(7);  ',{rules:[a,b,a]});
  assert.equal(result.source,'// unchanged\r\nuse(8);  ');
  assert.deepEqual(result.steps.map(s=>s.name),['First','Second','First']);
});
test('invalid input and invalid replacement fail without accepting output', () => {
  assert.throws(()=>transformSyntax('const = ;'));
  assert.throws(()=>transformSyntax('use(7)',{rules:[{name:'Broken',visitor({replace}) {return { NumericLiteral(p){replace(p,')');} };}}]}), /PARSE_FAIL/);
});
test('syntax checks preserve fragment binding kinds and evaluation order', () => {
  for(const [before,after] of [['var shared=1;','let shared=1;'],['var shared=1;','var changed=1;'],['if(x){var shared=1;}','if(x){} var shared=1;']])
    assert.match(checkSyntaxRewrite(before,after,{fragment:true}),/FRAGMENT/);
  assert.equal(checkSyntaxRewrite('var shared=!0;','var shared=true;',{fragment:true}),null);
});
test('new free names are rejected, including globals in fragments', () => {
  assert.match(checkSyntaxRewrite('use(7);','use(missing);'),/NEW_GLOBALS/);
  assert.match(checkSyntaxRewrite('use(void 7);','use(undefined);',{fragment:true}),/NEW_GLOBALS/);
  assert.equal(checkSyntaxRewrite('use(void 7);','use(undefined);',{allowedGlobals:['undefined']}),null);
  assert.match(checkSyntaxRewrite('use(7);','use(Infinity);'),/NEW_GLOBALS/);
});
test('overlapping edits fail explicitly',()=>{
  assert.throws(()=>transformSyntax('use(7)',{rules:[{name:'Overlap',visitor({replace}){return {CallExpression(p){replace(p,'use(8)');},NumericLiteral(p){replace(p,'8');}};}}]}),/overlap/i);
});
test('syntax state resumes both sides of atomic write and never uses rename offsets',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-stage-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  writeFileSync(join(root,'sample.js'),'const longName = !0;');
  const result=runSyntaxFile(root,'sample.js');
  assert.equal(result.status,'syntax');
  assert.equal(readFileSync(join(root,'sample.js'),'utf8'),'const longName = true;');
  const journal=openJournal(root,'sample.js',readFileSync(join(root,'sample.js'),'utf8'),{});
  journal.complete('const longName = true;','renamed');
  assert.equal(runSyntaxFile(root,'sample.js').status,'syntax-skipped');
  writeFileSync(join(root,'sample.js'),'const longName=false;');
  assert.equal(runSyntaxFile(root,'sample.js').status,'syntax-skipped');
  assert.throws(()=>openJournal(root,'sample.js',readFileSync(join(root,'sample.js'),'utf8'),{}),/changed/);
});
test('range replacements retain token boundaries after compact keywords',()=>{
  assert.equal(transformSyntax('function longName(){return!0;} typeof!1; void!0;').source,
    'function longName(){return true;} typeof false; void true;');
});
test('dynamic global guards follow wakaru known-source shapes exactly',()=>{
  const parenthesized='eval(("inspect()")); use(void 5);';
  assert.equal(transformSyntax(parenthesized).source,parenthesized);
  assert.equal(transformSyntax('eval("quire".replace(/^/g,"re")); use(void 5);').source,
    'eval("quire".replace(/^/g,"re")); use(undefined);');
});

for (const written of [false,true]) test(`syntax checkpoint recovers after prepare, written=${written}`,t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-crash-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  const file='source.js',before='const visibleName=!0;',after='const visibleName=true;';
  writeFileSync(join(root,file),before);
  runSyntaxFile(root,file);
  const checkpoint=join(root,'.unminify','syntax',`${file}.syntax.jsonl`);
  const header=JSON.parse(readFileSync(checkpoint,'utf8').split('\n')[0]);
  writeFileSync(checkpoint,JSON.stringify(header)+'\n');
  writeFileSync(join(root,file),before);
  const journal=openJournal(root,join('syntax',`${file}.syntax`),before,JSON.parse(header.options));
  journal.prepare(before,after,'syntax');
  if(written) atomicWrite(join(root,file),after);
  runSyntaxFile(root,file);
  assert.equal(readFileSync(join(root,file),'utf8'),after);
  assert.equal(runSyntaxFile(root,file).status,'syntax-skipped');
  assert(!readFileSync(checkpoint,'utf8').includes('names'));
});
test('labels and public property names do not shadow the undefined binding',()=>{
  assert.equal(transformSyntax('undefined: { consume(void 3); break undefined; }').source,
    'undefined: { consume(undefined); break undefined; }');
  assert.equal(transformSyntax('const record={undefined:7}; consume(void 3);').source,
    'const record={undefined:7}; consume(undefined);');
});
test('fragment layout retains repeated and destructured declarator boundaries',()=>{
  assert.match(checkSyntaxRewrite('var shared=1,shared=2;','var shared=2;',{fragment:true}),/FRAGMENT/);
  assert.match(checkSyntaxRewrite('var first=1,second=2;','var [first,second]=[1,2];',{fragment:true}),/FRAGMENT/);
});
test('syntax resume repairs a torn first checkpoint record',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-torn-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  writeFileSync(join(root,'source.js'),'const visibleName=!0;');
  runSyntaxFile(root,'source.js');
  writeFileSync(join(root,'source.js'),'const visibleName=!0;');
  writeFileSync(join(root,'.unminify','syntax','source.js.syntax.jsonl'),'{"type":"source"');
  assert.equal(runSyntaxFile(root,'source.js').status,'syntax');
  assert.equal(readFileSync(join(root,'source.js'),'utf8'),'const visibleName=true;');
});
