import test from 'node:test';
import assert from 'node:assert/strict';
import { parseArgs, validatePlan, schedule, toArgs, logRecord, confirmedAccount, preview } from '../scripts/manage-x-account.mjs';
test('dry run is default and sending requires the literal flag',()=>{
  assert.deepEqual(parseArgs(['post','Exact text']),{send:false,actions:[{verb:'post',text:'Exact text'}]});
  assert.equal(parseArgs(['like','https://x.com/placeholder/status/1','--send']).send,true);
  assert.deepEqual(parseArgs(['--plan','actions.json']),{send:false,plan:'actions.json'});
  for(const a of [[],['post'],['post','text','extra'],['--plan','a','post','b'],['post','x','--unknown']]) assert.throws(()=>parseArgs(a));
});
test('every verb validates with required fields and exact command arguments',()=>{
  for (const verb of ['reply','quote','post','like','unlike','follow','unfollow']) {
    const action={verb,text:'Exact text',url:'https://x.com/placeholder/status/1',user:'@placeholder'};
    assert.deepEqual(validatePlan([action]),[action]);
    assert.equal(toArgs(action)[0],verb);
    assert.ok(!toArgs(action).includes('@placeholder'));
  }
});
test('one invalid action refuses the whole batch, including malformed objects',()=>{
  for(const bad of [null,[],{verb:'delete'},{verb:'post',text:''},{verb:'post',text:3},{verb:'like',url:'bad'},{verb:'follow',user:'bad name'},{verb:'post',text:'x'.repeat(25001)}]) {
    assert.throws(()=>validatePlan([{verb:'post',text:'valid'},bad]),/REFUSED/);
  }
  assert.throws(()=>validatePlan({}),/list/);
  assert.equal(validatePlan([{verb:'post',text:'😀'.repeat(25000)}]).length,1);
});
test('follow schedule caps attempted follows at 15 and never queues excess',()=>{
  const actions=Array.from({length:17},()=>({verb:'follow',user:'placeholder'}));
  actions.splice(2,0,{verb:'post',text:'text'});
  const result=schedule(validatePlan(actions));
  assert.equal(result.filter(r=>r.skip).length,2);
  assert.equal(result.filter(r=>r.waitMs===60000).length,14);
  assert.equal(result[0].waitMs,0); assert.equal(result[2].waitMs,0);
  assert.equal(result.at(-1).waitMs,0);
});
test('account confirmation requires successful whoami and a valid username',()=>{
  assert.equal(confirmedAccount({code:0,body:[{username:'placeholder'}]}),'placeholder');
  for(const response of [{code:1,body:{username:'placeholder'}},{code:0,body:{}},{code:0,body:'logged out'},{code:0,body:{username:'?'}}]) assert.throws(()=>confirmedAccount(response));
});
test('log records include failed attempts and preserve exact action and result',()=>{
  const action={verb:'post',text:'Exact text'};
  assert.deepEqual(logRecord('placeholder',action,{code:1,body:{},stderr:'failed'},'2026-09-10T00:00:00Z'),{at:'2026-09-10T00:00:00Z',account:'placeholder',action,ok:false,result:'failed'});
  assert.equal(logRecord('placeholder',action,{code:0,body:{ok:true}}).ok,true);
  assert.match(preview([action]),/Exact text/);
  assert.match(preview([{verb:'post',text:'x'.repeat(281)}]),/verified/);
});
