import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {loadEnvFile,requireEnv,envPath} from '../scripts/env.mjs';
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
test('a fresh X invocation fails on its first missing key with the config path',()=>{
 const env={...process.env,HOME:mkdtempSync(join(tmpdir(),'intel-home-'))};
 for(const key of Object.keys(env)) if(key.startsWith('X_')) delete env[key];
 const result=spawnSync(process.execPath,[fileURLToPath(new URL('../scripts/fetch-x-mentions.mjs',import.meta.url)),'example','query'],{env,encoding:'utf8'});
 assert.equal(result.status,1);assert.match(result.stderr,/X_BEARER is required in .*\.config\/intel\/\.env/);
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
