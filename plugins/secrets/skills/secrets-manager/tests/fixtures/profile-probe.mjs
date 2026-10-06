import assert from 'node:assert/strict';
import { mock } from 'node:test';
import { createRequire } from 'node:module';
import { mkdtempSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
const require = createRequire(new URL('../../scripts/package.json', import.meta.url));
const lifecycle = [], launches = [], contexts = [], filled = [];
const page = {fill: async (...args) => filled.push(args), title: async () => 'fixture page'};
mock.module(require.resolve('imapflow'), {namedExports: {ImapFlow: class {
 constructor(options) {assert.deepEqual(options.auth,{user:'base@example.com',pass:'abcdefghijklmnop'});}
 async connect() {lifecycle.push('mail-connect');}
 async getMailboxLock(folder) {assert.equal(folder,'INBOX');return {release(){lifecycle.push('mail-release');}};}
 async search() {return [1];}
 async fetchOne() {return {source: Buffer.from('From: Delta <signup@delta.example>\r\nTo: base+delta@example.com\r\nSubject: Verification code\r\nContent-Type: text/plain\r\n\r\nYour verification code is 481920.\r\n')};}
 async logout() {lifecycle.push('mail-logout');}
}}});
const exitChecks = [];
mock.module(new URL('../../scripts/proxy-check.mjs', import.meta.url).href, {namedExports: {
 assertExitUp: async (url) => {exitChecks.push([url, launches.length]);},
 dispatcherFor: () => null, ProxyExitDown: class extends Error {},
}});
mock.module(require.resolve('camoufox-js'), {namedExports: {Camoufox: async options => {
 launches.push(options);
 const context = {closed:false, routes:[], sockets:[], pages:()=>[page],
  route:async (...args)=>context.routes.push(args), routeWebSocket:async (...args)=>context.sockets.push(args),
  close:async()=>{context.closed=true;}};
 contexts.push(context);return context;
}}});
process.env.SECRETS_STATE_DIR = mkdtempSync(join(tmpdir(),'profile-probe-'));
process.env.SECRETS_RESIDENTIAL_PROXY_URL = 'http://user:pass@proxy.example:8080';
const {kit, loadAdapters} = await import('../../scripts/adapter.mjs');
const [adapter] = await loadAdapters({paths:[fileURLToPath(new URL('./email-adapters.mjs',import.meta.url))]});
assert.equal(typeof kit.emailOtp.readSignupOtp, 'function');
assert.equal(kit.emailOtp.waitForCode, undefined);
const result = await adapter.byEmail({cred:{email:'base@example.com',app_password:'abcd efgh ijkl mnop'}});
assert.equal(result.status,'ok');assert.equal(result.alias,'base+delta@example.com');
assert.equal(result.detail,'fixture page');assert.equal(result.proxyUrl,process.env.SECRETS_RESIDENTIAL_PROXY_URL);
assert.deepEqual(result.message,{otp:'481920',from:'"Delta" <signup@delta.example>',subject:'Verification code',to:'base+delta@example.com',folder:'INBOX'});
assert.deepEqual(filled,[['input[name=code]','481920']]);
assert.deepEqual(lifecycle,['mail-connect','mail-release','mail-logout']);
assert.equal(contexts[0].closed,true);
assert.equal(launches[0].user_data_dir,kit.config.profileDirFor(result.alias));
// The exit is checked once per browser, before that browser launches.
assert.deepEqual(exitChecks[0],[result.proxyUrl,0]);
assert.equal(launches[0].headless,true);assert.equal(launches[0].geoip,true);
assert.deepEqual(launches[0].proxy,{server:'http://proxy.example:8080',username:'user',password:'pass'});
assert.ok(launches[0].fingerprint);assert.ok(existsSync(join(launches[0].user_data_dir,'fingerprint.json')));
assert.equal(contexts[0].routes.length,1);assert.equal(contexts[0].sockets[0][0],'wss://data.delta.example/**');
await assert.rejects(kit.withProfile(result.alias, {}, async (context, received) => {
 assert.equal(received,page);assert.equal(context,contexts[1]);
 throw new Error('hook failure');
}), /hook failure/);
assert.equal(contexts[1].closed,true);assert.deepEqual(launches[1].fingerprint,launches[0].fingerprint);
// A headless run asked to record saves the page video under the account's debug dir; no other run does.
assert.equal(launches[0].recordVideo,undefined);
await kit.withProfile(result.alias, {record:true}, async () => {});
assert.ok(launches[2].recordVideo.dir.startsWith(join(kit.config.debugDir(),result.alias,'rec-')));
console.log('profile and mailbox contract verified');
