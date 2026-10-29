// Ported from wakaru v1.14.0 (Apache-2.0); real CLI and filesystem tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync, execFileSync } from 'node:child_process';
const cli=new URL('./helpers/run-stages.mjs',import.meta.url);
test('real CLI finishes all syntax writes before rename planning and resumes offsets',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-cli-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  execFileSync('git',['init','--initial-branch=main',root],{stdio:'ignore'});
  for(const file of ['one.js','two.js']) writeFileSync(join(root,file),'export const a = !0;');
  // An empty, isolated auth directory prevents requests to the real service.
  const run=()=>spawnSync(process.execPath,[cli.pathname,root],{encoding:'utf8',env:{...process.env,CODEX_HOME:join(root,'absent-auth')},timeout:30000});
  const first=run();
  assert.equal(first.status,1,first.stderr);
  const events=first.stdout.trim().split('\n').filter(x=>x.startsWith('{')).map(JSON.parse);
  const syntax=events.filter(e=>e.status==='syntax');
  assert.equal(syntax.length,2);
  assert(events.findIndex(e=>e.status==='syntax'&&e.file==='two.js') < events.findIndex(e=>e.status==='interrupted'));
  for(const file of ['one.js','two.js']) {
    assert.equal(readFileSync(join(root,file),'utf8'),'export const a = true;');
    const checkpoint=JSON.parse(readFileSync(join(root,'.unminify', 'rename-bindings', 'files', `${file}.jsonl`),'utf8').split('\n')[0]);
    assert(existsSync(join(root,'.unminify', 'restore-syntax', 'syntax',`${file}.syntax.jsonl`)));
    assert(!checkpoint.options.includes('pipeline'));
  }
  const second=run();
  assert.equal(second.status,1);
  assert(!second.stdout.includes('source changed'));
  assert.equal(second.stdout.split('syntax-skipped').length-1,2);
});
test('real CLI completes syntax, strict rename checks, and a completed resume without credentials',t=>{
  const root=mkdtempSync(join(tmpdir(),'syntax-complete-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
  execFileSync('git',['init','--initial-branch=main',root],{stdio:'ignore'});
  writeFileSync(join(root,'module.js'),'export const longFlag = !1; export const absentValue = void 3;');
  const execute=()=>spawnSync(process.execPath,[cli.pathname,root,'--min-symbol-length','1'],{
    encoding:'utf8',env:{...process.env,CODEX_HOME:join(root,'absent-auth')},timeout:30000});
  const first=execute();
  assert.equal(first.status,0,first.stderr);
  assert.equal(readFileSync(join(root,'module.js'),'utf8'),'export const longFlag = false; export const absentValue = undefined;');
  assert.match(first.stdout,/"requests":0/);
  assert.match(readFileSync(join(root,'.unminify', 'rename-bindings', 'files', 'module.js.jsonl'),'utf8'),/"status":"renamed"/);
  const resumed=execute();
  assert.equal(resumed.status,0,resumed.stderr);
  assert.match(resumed.stdout,/syntax-skipped/);
  assert.match(resumed.stdout,/"status":"skipped"/);
});
