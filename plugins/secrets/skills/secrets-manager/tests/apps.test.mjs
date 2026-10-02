import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadAdapters, validateAdapter, kit } from '../scripts/adapter.mjs';
import factory from './fixtures/adapters.mjs';
const fixture = fileURLToPath(new URL('./fixtures/adapters.mjs', import.meta.url));
test('validate all required fields and optional hooks', () => {
 const base = factory(kit)[0];
 for (const field of ['name','domain','startUrl','entryTexts','signIn','ready']) {
  const a = {...base}; delete a[field]; assert.throws(() => validateAdapter(a), new RegExp(field));
 }
 for (const name of ['', 'bad-name', [], 'google', 'x', 'tiktok']) assert.throws(() => validateAdapter({...base,name}));
 for (const hook of ['signIn','ready','signedInUrl','byEmail','verify']) assert.throws(() => validateAdapter({...base,[hook]:1}));
 assert.throws(() => validateAdapter({...base, exportEnv: {envVar:'BAD',token:1}}));
});
test('loader handles absence, config, env override, duplicates, and import errors', async () => {
 const dir = mkdtempSync(join(tmpdir(),'adapter-')), configPath = join(dir,'config.json');
 assert.deepEqual(await loadAdapters({configPath,env:{}}), []);
 writeFileSync(configPath,JSON.stringify({adapters:[fixture]}));
 assert.deepEqual((await loadAdapters({configPath,env:{}})).map(a=>a.name), ['alpha','beta']);
 assert.deepEqual(await loadAdapters({configPath,env:{SECRETS_MANAGER_ADAPTERS:''}}), []);
 await assert.rejects(loadAdapters({paths:[fixture,fixture]}), /duplicate adapter name/);
 const bad = join(dir,'bad.mjs'); writeFileSync(bad,'throw new Error("import failure")');
 await assert.rejects(loadAdapters({paths:[bad]}), e => e.message.includes(bad) && e.message.includes('import failure'));
 const invalid = join(dir,'invalid.mjs'); writeFileSync(invalid,'export default () => [{}]');
 await assert.rejects(loadAdapters({paths:[invalid]}), /name is required/);
});
test('fixture same-tab and popup entry flows use real page helpers', async () => {
 const steps=[]; const page={getByText:t=>({first:()=>({click:async()=>steps.push(t)})}),locator:t=>({first:()=>({waitFor:async()=>steps.push(t)})})};
 const [a,b]=factory(kit);
 await a.signIn(page); assert.deepEqual(steps,['Login','Continue with Google']);
 steps.length=0; await b.signIn(page); assert.deepEqual(steps,['Sign up','iframe[src*="accounts.google.com/gsi/button"]','Continue with Google']);
});

test('fixture adapters drive same-tab and popup OAuth and their readiness hooks', async () => {
 const {oauthSurface}=await import('../scripts/login.mjs');
 const adapters=factory(kit);
 for (const adapter of adapters) {
  let current=adapter.startUrl;
  const google={url:()=> 'https://accounts.google.com/o/oauth2/auth'};
  const pages=[];
  const page={url:()=>current,context:()=>({pages:()=>pages,cookies:async()=>[{name:'auth-access-token'}]}),
   evaluate:async (_fn,key)=>key==='session:token',
   getByText:text=>({first:()=>({click:async()=>{
    if(text==='Continue with Google') {
     if(adapter.name==='alpha') current=google.url(); else pages.push(google);
    }
   }})}),locator:()=>({first:()=>({waitFor:async()=>{}})})};
  pages.push(page);
  assert.equal(await adapter.ready(page),true);
  await adapter.signIn(page);
  assert.equal(await oauthSurface(page,adapter,'fixture@example.com'),adapter.name==='alpha'?page:google);
 }
 assert.throws(()=>kit.aliasFor('base@example.com'),/tag is required/);
 await assert.rejects(kit.mintAppPassword({},{}),/name is required/);
});

test('startup merges adapter traffic blocks and resets them for zero adapters', async () => {
 const {isBlockedHost,BLOCKED_WEBSOCKETS}=await import('../scripts/traffic.mjs');
 await loadAdapters({paths:[fixture]});
 assert.equal(isBlockedHost('app-actions-eu.alpha.example'),true);
 assert.ok(BLOCKED_WEBSOCKETS.includes('wss://data.alpha.example/**'));
 await loadAdapters({paths:[]});
 assert.equal(isBlockedHost('app-actions-eu.alpha.example'),false);
 assert.deepEqual(BLOCKED_WEBSOCKETS,[]);
});

test('adapter names must equal the final table name and exclude reserved tables', () => {
 const base = factory(kit)[0];
 for (const name of ['alpha__beta', '_google', 'Alpha', 'alpha_', 'sqlite_sessions', 'sqlite_sequence', 'google', 'x', 'tiktok']) {
  assert.throws(() => validateAdapter({...base, name}), /bad adapter name/);
 }
 assert.equal(validateAdapter({...base, name: 'alpha_beta'}).name, 'alpha_beta');
});

test('normalized table collisions cannot overwrite an existing in-memory session', async () => {
 const db = kit.store.openDb(':memory:');
 const base = factory(kit)[0];
 try {
  const first = validateAdapter({...base, name: 'alpha_beta'});
  kit.store.saveSession(db, first.name, 'fixture@example.com', [{name:'session', value:'first'}], []);
  assert.equal(kit.store.toAppSlug('alpha__beta'), first.name); // the reviewed collision
  assert.throws(() => {
   const second = validateAdapter({...base, name: 'alpha__beta'});
   kit.store.saveSession(db, second.name, 'fixture@example.com', [{name:'session', value:'second'}], []);
  }, /bad adapter name/);
  assert.equal(kit.store.getSession(db, first.name, 'fixture@example.com').cookies[0].value, 'first');
  const dir = mkdtempSync(join(tmpdir(), 'collision-')), path = join(dir, 'adapters.mjs');
  writeFileSync(path, `export default () => ['alpha_beta','alpha__beta'].map(name => ({name, domain:'example.com', startUrl:'https://example.com', entryTexts:['Login'], signIn:async()=>{}, ready:async()=>true}));`);
  await assert.rejects(loadAdapters({paths:[path]}), /bad adapter name: alpha__beta/);
 } finally { db.close(); }
});

test('fixture email hook uses the real kit reader and profile engine with stubbed mailbox and launcher', async () => {
 const {spawnSync} = await import('node:child_process');
 const result = spawnSync(process.execPath, ['--experimental-test-module-mocks', fileURLToPath(new URL('./fixtures/profile-probe.mjs', import.meta.url))], {encoding:'utf8'});
 assert.equal(result.status, 0, result.stderr);
 assert.match(result.stdout, /profile and mailbox contract verified/);
});

test('the kit exposes the app-flow engine helpers a byEmail or verify hook needs', async () => {
  for (const name of ['exportScoped', 'gotoPastCloudflare', 'waitReady', 'appAlreadySignedIn', 'withAppRetries']) {
    assert.equal(typeof kit[name], 'function', `kit.${name}`);
  }
  assert.equal(typeof kit.filterState, 'function');
  assert.deepEqual(kit.filterState({ cookies: [{ domain: '.b.test', name: 'k' }, { domain: 'other.test', name: 'k' }], origins: [] }, 'b.test').cookies.length, 1);
});

test('the kit exposes the credential-file writers so a hook can persist a minted app password', () => {
  for (const name of ['setAppPassword', 'setTotpSecret', 'loadCredentials']) {
    assert.equal(typeof kit.credentials[name], 'function', `kit.credentials.${name}`);
  }
  assert.equal(typeof kit.config.credentialsDir, 'function');
});
