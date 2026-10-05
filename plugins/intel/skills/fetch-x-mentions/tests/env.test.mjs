import test from 'node:test';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {loadEnvFile,requireEnv,envPath} from '../scripts/env.mjs';
test('default configuration comes from the user directory, independent of cwd', () => {
 const home=mkdtempSync(join(tmpdir(),'intel-config-home-'));
 mkdirSync(join(home,'.config/intel'),{recursive:true});
 writeFileSync(join(home,'.config/intel/.env'),'INTEL_SHARED_CONFIG="shared value"\n');
 writeFileSync(join(home,'.env'),'INTEL_SHARED_CONFIG=wrong-location\n');
 const moduleUrl=new URL('../scripts/env.mjs',import.meta.url).href;
 const code=`const {loadEnvFile,requireEnv,envPath}=await import(${JSON.stringify(moduleUrl)}); loadEnvFile(); if(requireEnv('INTEL_SHARED_CONFIG')!=='shared value') throw Error('wrong configuration source'); console.log(envPath);`;
 const env={...process.env,HOME:home};delete env.INTEL_SHARED_CONFIG;
 const result=spawnSync(process.execPath,['--input-type=module','-e',code],{cwd:home,env,encoding:'utf8'});
 assert.equal(result.status,0,result.stderr);
 assert.equal(result.stdout.trim(),join(home,'.config/intel/.env'));
});
test('optional env file preserves exported variables and identifies missing keys',()=>{
 const dir=mkdtempSync(join(tmpdir(),'intel-env-')),path=join(dir,'.env');
 writeFileSync(path,'# comment\nINTEL_TEST_KEY=file\nINTEL_SECOND_KEY=loaded\n');
 process.env.INTEL_TEST_KEY='exported';
 try {
  loadEnvFile(path); loadEnvFile(join(dir,'missing'));
  assert.equal(process.env.INTEL_TEST_KEY,'exported');assert.equal(requireEnv('INTEL_SECOND_KEY'),'loaded');
  assert.throws(()=>requireEnv('INTEL_MISSING_KEY'),e=>e.message.includes('INTEL_MISSING_KEY')&&e.message.includes(envPath));
 } finally {delete process.env.INTEL_TEST_KEY;delete process.env.INTEL_SECOND_KEY;}
});
test('a missing config file reports that exact path for a required key', () => {
 const path=join(mkdtempSync(join(tmpdir(),'intel-local-env-')),'.env');
 delete process.env.INTEL_MISSING_LOCAL_KEY;
 loadEnvFile(path);
 assert.throws(()=>requireEnv('INTEL_MISSING_LOCAL_KEY'),e=>e.message.includes(path));
});

test('Node env-file parsing handles quotes, inline comments and exported precedence', () => {
 const dir=mkdtempSync(join(tmpdir(),'intel-dotenv-')), path=join(dir,'.env');
 const keys=['INTEL_QUOTED','INTEL_SINGLE','INTEL_COMMENT','INTEL_HASH','INTEL_PRECEDENCE'];
 const saved=Object.fromEntries(keys.map(key=>[key,process.env[key]]));
 for (const key of keys) delete process.env[key];
 writeFileSync(path, `# whole-line comment
INTEL_QUOTED="http://user:pass@proxy.example:8080" # trailing comment
INTEL_SINGLE='quoted words'
INTEL_COMMENT=bare # trailing comment
INTEL_HASH="keep # inside quotes"
INTEL_PRECEDENCE="file value"
`);
 process.env.INTEL_PRECEDENCE='exported';
 try {
  loadEnvFile(path);
  assert.equal(process.env.INTEL_QUOTED,'http://user:pass@proxy.example:8080');
  assert.equal(process.env.INTEL_SINGLE,'quoted words');
  assert.equal(process.env.INTEL_COMMENT,'bare');
  assert.equal(process.env.INTEL_HASH,'keep # inside quotes');
  assert.equal(process.env.INTEL_PRECEDENCE,'exported');
 } finally {
  for (const key of keys) {
   if (saved[key] === undefined) delete process.env[key]; else process.env[key]=saved[key];
  }
 }
});

test('importing a shared client never loads its sibling dotenv file', () => {
 const client=new URL('../../fetch-x-user-posts/scripts/fetch-x-user-posts.mjs', import.meta.url).href;
 const app=new URL('../../fetch-app-reviews/scripts/fetch-app-reviews.mjs', import.meta.url).href;
 const code=`process.loadEnvFile = () => { throw new Error('unexpected dotenv load during library import'); }; await import(${JSON.stringify(client)}); await import(${JSON.stringify(app)});`;
 const result=spawnSync(process.execPath,[...process.execArgv,'--input-type=module','-e',code],{encoding:'utf8'});
 assert.equal(result.status,0,result.stderr);
});
