import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, writeFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {run} from '../scripts/cli.mjs';
import * as select from '../scripts/select.mjs';
import * as rename from '../scripts/rename.mjs';
import * as checks from '../scripts/checks.mjs';
import * as syntax from '../scripts/syntax/stage.mjs';

test('large independent chunks execute off the main thread and reuse durable replies', async t => {
  const root=await mkdtemp(join(tmpdir(),'unminify-rename-workers-'));
  t.after(()=>rm(root,{recursive:true,force:true}));
  execFileSync('git',['init',root],{stdio:'ignore'});
  const chunks=[{id:'first',files:['first.js','first-use.js']},{id:'second',files:['second.js','second-use.js']}];
  for(const chunk of chunks) { await writeFile(join(root,chunk.files[0]),`/*${'x'.repeat(270000)}*/\nconst a = 1; console.log(a);\n`); await writeFile(join(root,chunk.files[1]),'console.log(a);\n'); }
  const manifest=join(root,'chunks.json');await writeFile(manifest,JSON.stringify({version:1,chunks}));
  let calls=0;const events=[];
  const dependencies={...select,...rename,...checks,...syntax,syntaxPool:{size:2},
    output:line=>events.push(JSON.parse(line)),
    createNamer:()=>({async name(payload){calls++;return {names:Object.fromEntries(Object.keys(payload.placeholders).map(key=>[key,'amount']))};},async close(){}})};
  const options={command:'rename-bindings',paths:[root],chunks:manifest,concurrency:2};
  assert.equal((await run(options,dependencies)).exitCode,0);
  assert.equal(new Set(events.filter(e=>e.status==='cpu-timing'&&e.threadId>0).map(e=>e.threadId)).size,2);
  assert.equal(calls,2);
  assert.equal((await run(options,dependencies)).skipped,4);
  assert.equal(calls,2);
});
