import test from 'node:test';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { verifyX } from '../scripts/verify-x.mjs';
const io={log(){},error(){}};
function database() {
 const db=new DatabaseSync(':memory:');
 db.exec(`CREATE TABLE x(username TEXT PRIMARY KEY, auth_token TEXT, ct0 TEXT, cookies TEXT, status TEXT, updated_at TEXT);
 INSERT INTO x VALUES ('a','token-a',NULL,NULL,'active','t'),('b','token-b','old','{}','active','t'),('c','token-c',NULL,NULL,'active','t')`);
 return db;
}
test('verification selects pending rows and saves valid pairs or expiry', async () => {
 const db=database(), seen=[];
 try {
  assert.equal(await verifyX(db,{concurrency:2},{io,provision:async token=>{seen.push(token);return {ok:token==='token-a',ct0:'new'};}}),0);
  assert.deepEqual(seen.sort(),['token-a','token-c']);
  const a=db.prepare('SELECT * FROM x WHERE username=?').get('a');
  assert.equal(a.status,'active');assert.equal(a.ct0,'new');assert.deepEqual(JSON.parse(a.cookies),{auth_token:'token-a',ct0:'new'});
  assert.equal(db.prepare('SELECT status FROM x WHERE username=?').get('c').status,'expired');
  assert.equal(db.prepare('SELECT ct0 FROM x WHERE username=?').get('b').ct0,'old');
 } finally {db.close();}
});
test('only selects an already paired row; failures persist expiry and return nonzero', async () => {
 const db=database();
 try {
  assert.equal(await verifyX(db,{only:['b']},{io,provision:async()=>{throw new Error('network');}}),1);
  assert.equal(db.prepare('SELECT status FROM x WHERE username=?').get('b').status,'expired');
  await assert.rejects(verifyX(db,{concurrency:0},{io}),/positive integer/);
 } finally {db.close();}
});
