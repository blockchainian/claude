import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseArgs, parseWindow, filterWindow, denseBand, pullSize, parseJson, topicFromHtml, distinctive } from '../scripts/fetch-x-feed.mjs';
import { measure, replyShape, measureReplies, parseArgs as measureArgs } from '../scripts/measure-x-feed.mjs';
import { saveDigest, parseArgs as saveArgs } from '../scripts/save-x-digest.mjs';
const now = Date.parse('2026-09-10T12:00:00Z');
const fixture = JSON.parse(readFileSync(new URL('./fixtures/feed.json', import.meta.url)));
test('only timeline and trending inputs, positive limits and known flags', () => {
  assert.deepEqual(parseArgs(['tldr', '--limit', '8']), { what: 'tldr', window: '24h', limit: 8 });
  for (const args of [['keyword'], ['@placeholder'], ['tldr','--feed','for-you'], ['tldr','--limit','0'], ['tldr','--threads','1'], ['tldr','--window']]) assert.throws(() => parseArgs(args));
});
test('relative and inclusive calendar windows are UTC and validated', () => {
  assert.equal(parseWindow('90m',now).start,now-90*60000);
  assert.equal(parseWindow('2w',now).start,now-14*86400000);
  assert.equal(parseWindow('2026-09-01..2026-09-07',now).end,Date.parse('2026-09-08T00:00:00Z')-1);
  for (const value of ['0h','bad','2026-02-30..2026-03-01','2026-09-12..2026-09-13']) assert.throws(() => parseWindow(value,now));
});
test('real archive shape survives filtering, sorting and id deduplication', () => {
  const rows = filterWindow([...fixture,fixture[0],{...fixture[0],id:'outside',created_at:'2020-01-01'}], {start:Date.parse('2026-09-01'),end:now+86400000});
  assert.equal(rows.length,fixture.length);
  assert.deepEqual(Object.keys(rows[0]),Object.keys(fixture[0]));
  assert.ok(Date.parse(rows[0].created_at)>=Date.parse(rows.at(-1).created_at));
  assert.throws(() => filterWindow([{id:'invalid',created_at:'bad'}],{start:0,end:now}));
});
test('dense band stops after two thin hours, including missing hours', () => {
  const rows = [0,1,2,5].flatMap((h)=>Array.from({length:h===5?1:12},()=>({created_at:new Date(now-h*3600000).toISOString()})));
  assert.deepEqual(denseBand(rows),{from:new Date(now-2*3600000).toISOString(),posts:36,share:36/37});
  assert.equal(denseBand(fixture),null);
});
test('probe sizing ignores stale tail, caps scrolling and respects explicit small pulls', () => {
  const rows=Array.from({length:150},(_,i)=>({created_at:new Date(now-(i===149?100000:i)*60000).toISOString()}));
  assert.equal(pullSize(rows,24).limit,850);
  assert.ok(pullSize(rows,1).perHour>50);
});
test('JSON banners are stripped without accepting broken payloads', () => {
  assert.deepEqual(parseJson('notice\n[{"a":1}]\nUpdate available'),[{a:1}]);
  assert.deepEqual(parseJson('{"text":"brace } and escaped \\\""} trailing'),{text:'brace } and escaped "'});
  assert.throws(()=>parseJson('banner [broken]'));
});
test('topic metadata handles attribute order and entities and requires a title', () => {
  assert.equal(topicFromHtml('<meta content="A &amp; B" property="og:title">','topic').title,'A & B');
  assert.throws(()=>topicFromHtml('<html></html>','topic'));
  assert.equal(distinctive('The project expands community research'),'project community research');
});
test('stats measure actual fields and omit unmeasured reach', () => {
  const rows=Array.from({length:3},(_,i)=>({...fixture[0],id:String(i),author:'placeholder',bio:'builder',text:'中文 $TEST topic',views:6000,likes:i,replies:2,retweets:1}));
  const result=measure(rows,2,['topic']);
  assert.equal(result.posts,3); assert.equal(result.authors,1); assert.equal(result.chinese,3);
  assert.equal(result.personas['founder / builder'],1); assert.equal(result.tickers.TEST,3);
  assert.equal(result.terms.topic,3); assert.equal(result.opportunities[0].viewsPerReply,2000);
  assert.equal(result.percentiles.likes.p90,2); assert.equal(result.topPosts[0].likes,2);
  assert.equal(measure(rows.map(r=>({...r,views:null}))).percentiles,null);
});
test('reply measurements retain shape and feed overlap',()=>{
  assert.equal(replyShape('@placeholder why is this happening?'),'asks a question');
  const result=measureReplies([{author:'placeholder',text:'ok',likes:0,bio:''}], [{author:'placeholder'}]);
  assert.equal(result.zeroLikes,1); assert.equal(result.overlap.length,1); assert.equal(result.shapes['one word / emoji'].count,1);
});
test('digest save preserves exact text and appends history, never regenerates', () => {
  const root=mkdtempSync(join(tmpdir(),'feed-test-'));
  try {
    mkdirSync(join(root,'x-tldr-2026-09-10')); const digest='  Exact digest\nwith spacing  ';
    const result=saveDigest({root,text:digest});
    assert.equal(readFileSync(result.saved,'utf8'),digest);
    assert.equal(JSON.parse(readFileSync(result.history,'utf8')).digest,digest);
    assert.throws(()=>saveDigest({root,text:' '}));
    saveDigest({root,text:'second'}); assert.equal(readFileSync(result.history,'utf8').trim().split('\n').length,2);
  } finally {rmSync(root,{recursive:true,force:true});}
});

test('measurement and save parsers reject missing values and preserve supplied text',()=>{
  assert.deepEqual(measureArgs(['archive','--top','3','--terms',' a, b ']),{archive:'archive',top:3,terms:['a','b']});
  assert.deepEqual(saveArgs(['--text','  exact  ','--archive','archive']),{text:'  exact  ',archive:'archive'});
  for(const args of [[],['archive','--top','0'],['archive','--terms'],['archive','--unknown']]) assert.throws(()=>measureArgs(args));
  for(const args of [['--text'],['--unknown','value']]) assert.throws(()=>saveArgs(args));
});
