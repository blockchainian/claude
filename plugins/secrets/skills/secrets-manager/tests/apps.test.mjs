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
