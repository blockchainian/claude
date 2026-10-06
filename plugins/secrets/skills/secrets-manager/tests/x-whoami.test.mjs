import {test} from 'node:test';
import assert from 'node:assert/strict';
import {whoamiX} from '../scripts/x-verify.mjs';
const credential='a'.repeat(40);
const options={queryId:'QID',bearer:'public-bearer'};
const viewer=(username)=>new Response(JSON.stringify({data:{viewer:{user_results:{result:{core:{screen_name:username}}}}}}));
test('X whoami provisions ct0 from an auth token and returns the server username',async()=>{
 const calls=[];
 const identity=await whoamiX({credential},{...options,fetchImpl:async(url,init)=>{
  calls.push({url,init});
  return calls.length===1?new Response('{}',{headers:{'set-cookie':'ct0=csrf; Path=/'}}):viewer('alice');
 }});
 assert.deepEqual(identity,{username:'alice'});
 assert.equal(calls.length,2);
 assert.equal(calls[0].init.headers.cookie,`auth_token=${credential}`);
 assert.equal(calls[1].init.headers.cookie,`auth_token=${credential}; ct0=csrf`);
 assert.equal(calls[1].init.headers['x-csrf-token'],'csrf');
 assert.ok(calls[0].init.signal instanceof AbortSignal);
});
test('X whoami validates the credential before requests',async()=>{
 for(const credential of ['','bad','a'.repeat(39),'a'.repeat(40)+'; injected=1'])
  await assert.rejects(()=>whoamiX({credential},options),/auth_token/);
});
test('X whoami reports stale queries, HTTP failures and missing ct0',async()=>{
 for(const [status,message] of [[404,/SECRETS_X_VIEWER_QUERY_ID/],[429,/HTTP 429/],[500,/HTTP 500/],[200,/ct0/]])
  await assert.rejects(()=>whoamiX({credential},{...options,fetchImpl:async()=>new Response('{}',{status})}),message);
});
test('X whoami rejects expired, locked and suspended credentials even when a user is included',async()=>{
 for(const code of [32,89,215,64,326]){
  let n=0;
  await assert.rejects(()=>whoamiX({credential},{...options,fetchImpl:async()=>++n===1?
   new Response('{}',{headers:{'set-cookie':'ct0=csrf'}}):new Response(JSON.stringify({errors:[{code}],data:{viewer:{user_results:{result:{core:{screen_name:'alice'}}}}}}))}),/X whoami/);
 }
});
test('X whoami rejects malformed responses and usernames',async()=>{
 for(const body of [{},{data:{viewer:{user_results:{result:{core:{screen_name:'bad name'}}}}}}]){
  let n=0;
  await assert.rejects(()=>whoamiX({credential},{...options,fetchImpl:async()=>++n===1?
   new Response('{}',{headers:{'set-cookie':'ct0=csrf'}}):new Response(JSON.stringify(body))}),/identity/);
 }
});

test('X whoami reports network failures',async()=>{
 await assert.rejects(()=>whoamiX({credential},{...options,fetchImpl:async()=>{throw new Error('network down');}}),/network down/);
});
