// Ported from wakaru v1.14.0 (Apache-2.0); independently authored context tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import * as fragments from '../scripts/fragments.mjs';
const { indexFragmentContext, loadFragmentContext } = fragments;
import { transformSyntax } from '../scripts/syntax/runner.mjs';
import { globalGuard, parseSyntax } from '../scripts/syntax/guards.mjs';
import { checkSyntaxRewrite } from '../scripts/syntax/checks.mjs';
import { runSyntaxFile } from '../scripts/syntax/stage.mjs';
const source = '// preserved\r\nvar shared = void 17;';
const context = sibling => indexFragmentContext(new Map([['selected.js', source], ['sibling.js', sibling]]));

test('globalGuard accepts a fragment when indexed program bindings do not shadow it', () => {
  assert.equal(globalGuard(parseSyntax(source),true,{topLevelBindings:new Set(['shared'])})('undefined'),true);
});

test('fragment with index evidence restores numeric void and preserves untouched text', () => {
  const fragmentContext = context('consume(shared);');
  assert(fragmentContext.fragments.has('selected.js'));
  assert.equal(transformSyntax(source, {fragment:true, fragmentContext}).source, '// preserved\r\nvar shared = undefined;');
});
test('program binding in a sibling blocks the corresponding intrinsic only', () => {
  const fragmentContext = context('var undefined = 7; consume(shared);');
  assert.equal(transformSyntax(source,{fragment:true,fragmentContext}).source,source);
  const permits = globalGuard(parseSyntax(source),true,fragmentContext);
  assert(!permits('undefined'));
  assert(permits('Infinity'));
});
test('all program declarations including import aliases participate in the evidence', () => {
  for (const declaration of ['let undefined;', 'const undefined = 7;', 'function undefined(){}', 'class undefined {}', 'import {value as undefined} from "pkg";', 'if (ready) { var undefined; }']) {
    assert.equal(transformSyntax(source,{fragment:true,fragmentContext:context(declaration)}).source,source);
  }
  assert.equal(transformSyntax(source,{fragment:true,fragmentContext:context('function sibling(undefined) {} consume(shared);')}).source,'// preserved\r\nvar shared = undefined;');
});
test('no context and local shadowing or dynamic scopes remain protected', () => {
  assert.equal(transformSyntax(source,{fragment:true}).source,source);
  for (const suffix of ['function local(undefined){}', 'with (record) { consume(); }', 'eval(code);', 'eval("undefined = 9");']) {
    const input = `${source}\n${suffix}`;
    assert.equal(transformSyntax(input,{fragment:true,fragmentContext:context('consume(shared);')}).source,input);
  }
});
test('checker requires both index evidence and per-pass authorization for fragment globals', () => {
  const after='// preserved\r\nvar shared = undefined;';
  assert.equal(checkSyntaxRewrite(source,after,{fragment:true,fragmentContext:context('consume(shared);'),allowedGlobals:['undefined']}),null);
  assert.match(checkSyntaxRewrite(source,after,{fragment:true,allowedGlobals:['undefined']}),/NEW_GLOBALS/);
  assert.match(checkSyntaxRewrite(source,after,{fragment:true,fragmentContext:context('var undefined;'),allowedGlobals:['undefined']}),/NEW_GLOBALS/);
});
for (const shadowed of [false,true]) test(`real CLI uses siblings outside its selection, shadowed=${shadowed}`,t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-context-cli-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  execFileSync('git',['init','--initial-branch=main',root],{stdio:'ignore'});
  writeFileSync(join(root,'selected.js'),source);
  writeFileSync(join(root,'sibling.js'),`${shadowed?'var undefined = 7; ':''}consume(shared);`);
  const result=spawnSync(process.execPath,[new URL('./helpers/run-stages.mjs',import.meta.url).pathname,join(root,'selected.js'),'--min-symbol-length','1'],{encoding:'utf8',env:{...process.env,CODEX_HOME:join(root,'absent-auth')},timeout:30000});
  assert.equal(result.status,0,result.stderr);
  assert.equal(readFileSync(join(root,'selected.js'),'utf8'),shadowed?source:'// preserved\r\nvar shared = undefined;');
});
test('outside Git the context covers exactly the supplied input set',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-context-inputs-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  writeFileSync(join(root,'selected.js'),source);
  writeFileSync(join(root,'sibling.js'),'var undefined; consume(shared);');
  const alone=loadFragmentContext(root,['selected.js']);
  const together=loadFragmentContext(root,['selected.js','sibling.js']);
  assert.equal(transformSyntax(source,{fragment:true,fragmentContext:alone}).source,'// preserved\r\nvar shared = undefined;');
  assert.equal(transformSyntax(source,{fragment:true,fragmentContext:together}).source,source);
});
test('syntax resume rejects changed sibling binding evidence',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-context-resume-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  writeFileSync(join(root,'selected.js'),source);
  runSyntaxFile(root,'selected.js',{fragment:true,fragmentContext:context('consume(shared);')});
  assert.throws(()=>runSyntaxFile(root,'selected.js',{fragment:true,fragmentContext:context('var undefined; consume(shared);')}),/options changed/);
});

test('ordinary sibling renames do not invalidate completed syntax checkpoints',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-context-names-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  writeFileSync(join(root,'selected.js'),source);
  runSyntaxFile(root,'selected.js',{fragment:true,fragmentContext:context('var a; consume(shared);')});
  assert.equal(runSyntaxFile(root,'selected.js',{fragment:true,fragmentContext:context('var readableName; consume(shared);')}).status,'syntax-skipped');
});

test('rules declare synthesized intrinsics so checkpoint evidence stays complete',()=>{
  const rule={name:'UndeclaredGlobal',visitor({canIntroduceGlobal}){return {NumericLiteral(path){canIntroduceGlobal(path,'undefined');}};}};
  assert.throws(()=>transformSyntax('consume(7);',{rules:[rule]}),/undeclared intrinsic/);
});
