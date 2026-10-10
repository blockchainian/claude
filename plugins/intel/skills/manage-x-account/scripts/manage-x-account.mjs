import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { appendFileSync, mkdirSync, readFileSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { loadEnvFile, stateDir } from '../../fetch-x-mentions/scripts/env.mjs';
import { parseJson } from '../../fetch-x-feed/scripts/fetch-x-feed.mjs';
const exec=promisify(execFile);
const fields={reply:['url','text'],quote:['url','text'],post:['text'],like:['url'],unlike:['url'],follow:['user'],unfollow:['user']};
export function parseArgs(argv) {
  let send=false, plan; const positional=[];
  for(let i=0;i<argv.length;i++) {
    if(argv[i]==='--send') send=true;
    else if(argv[i]==='--plan') {plan=argv[++i]; if(!plan || plan.startsWith('--')) throw new Error('--plan needs a file');}
    else if(argv[i].startsWith('--')) throw new Error(`unknown option ${argv[i]}`);
    else positional.push(argv[i]);
  }
  if(plan) {if(positional.length) throw new Error('use a plan or a single action'); return {send,plan};}
  const [verb,...rest]=positional;
  if(!Object.hasOwn(fields,verb) || rest.length!==fields[verb].length) throw new Error('give a supported verb and its required arguments, or --plan');
  return {send,actions:[Object.fromEntries([['verb',verb],...fields[verb].map((f,i)=>[f,rest[i]])])]};
}
export function validatePlan(actions) {
  if(!Array.isArray(actions)) throw new Error('plan must be a JSON list');
  const problems=[];
  for(const [index,a] of actions.entries()) {
    if(!a || typeof a!=='object' || Array.isArray(a) || !Object.hasOwn(fields,a.verb)) {problems.push(`${index+1}: unknown action`); continue;}
    for(const f of fields[a.verb]) if(typeof a[f]!=='string' || !a[f].trim()) problems.push(`${index+1}: missing or invalid ${f}`);
    if(fields[a.verb].includes('url') && typeof a.url==='string' && !/^https:\/\/x\.com\/[A-Za-z0-9_]+\/status\/\d+(?:[?#].*)?$/.test(a.url.trim())) problems.push(`${index+1}: invalid status URL`);
    if(fields[a.verb].includes('user') && typeof a.user==='string' && !/^@?[A-Za-z0-9_]{1,15}$/.test(a.user.trim())) problems.push(`${index+1}: invalid handle`);
    if(typeof a.text==='string' && [...a.text].length>25000) problems.push(`${index+1}: text exceeds 25000 characters`);
  }
  if(problems.length) throw new Error('REFUSED — fix the whole batch:\n'+problems.join('\n'));
  return actions;
}
export function toArgs(action) {
  return [action.verb,...fields[action.verb].map(f=>f==='user'?action[f].trim().replace(/^@/,''):f==='url'?action[f].trim():action[f])];
}
export function schedule(actions) {
  let follows=0;
  return actions.map(action=>{
    if(action.verb!=='follow') return {action,skip:false,waitMs:0};
    follows++;
    return {action,skip:follows>15,waitMs:follows>1 && follows<=15?60000:0};
  });
}
export function confirmedAccount({code,body}) {
  const who=Array.isArray(body)?body[0]:body;
  if(code!==0 || typeof who?.username!=='string' || !/^[A-Za-z0-9_]{1,15}$/.test(who.username)) throw new Error('could not confirm account; run opencli twitter whoami before sending');
  return who.username;
}
export function logRecord(account,action,response,at=new Date().toISOString()) {
  return {at,account,action,ok:response.code===0,result:response.code===0?response.body:(response.stderr || response.body)};
}
export function preview(actions) {
  return actions.map((a,i)=>`[${i+1}/${actions.length}] ${a.verb.toUpperCase()}\n`+fields[a.verb].map(f=>`${f}: ${a[f]}`).join('\n')+(a.text?`\n${[...a.text].length} characters${[...a.text].length>280?' — over 280; needs a verified account':''}`:'')).join('\n\n');
}
async function call(args) {
  let result;
  try {result={...await exec('opencli',['twitter',...args,'-f','json'],{timeout:180000,maxBuffer:8*1024*1024}),code:0};}
  catch(error) {result={code:error.code || 1,stdout:error.stdout || '',stderr:error.stderr || error.message};}
  let body=result.stdout.trim();
  try {body=parseJson(body);} catch { /* Write output can be plain text; the exit status determines success. */ }
  return {code:result.code,body,stderr:result.stderr.trim()};
}
async function main() {
  loadEnvFile();
  const args=parseArgs(process.argv.slice(2));
  const actions=validatePlan(args.plan?JSON.parse(readFileSync(args.plan,'utf8')):args.actions);
  let account='unconfirmed';
  try {account=confirmedAccount(await call(['whoami']));}
  catch(error) {if(args.send) throw error; console.error(error.message);}
  console.log(`account: ${account}; actions: ${actions.length}; mode: ${args.send?'SEND':'DRY RUN'}`);
  console.log(preview(actions));
  const steps=schedule(actions), skipped=steps.filter(s=>s.skip).length;
  if(skipped) console.log(`${skipped} follows will be skipped, not queued (15/run cap).`);
  if(!args.send) {console.log('Dry run. Nothing sent. Review this preview before using --send.'); return;}
  const path=join(stateDir(),'x','writes.jsonl'); mkdirSync(dirname(path),{recursive:true});
  // Open the log before the first write so an unwritable destination cannot go unnoticed.
  appendFileSync(path,'');
  let done=0, failed=0;
  for(const [index,step] of steps.entries()) {
    if(step.skip) {console.log(`[${index+1}] SKIPPED follow — run cap`); continue;}
    if(step.waitMs) await sleep(step.waitMs);
    const response=await call(toArgs(step.action));
    appendFileSync(path,JSON.stringify(logRecord(account,step.action,response))+'\n');
    if(response.code===0) {done++; console.log(`[${index+1}] sent ${step.action.verb}`);}
    else {failed++; console.log(`[${index+1}] FAILED ${step.action.verb}: ${String(response.stderr || JSON.stringify(response.body)).slice(0,200)}\nNot retried — check X before running this action again.`);}
    await sleep(3000);
  }
  console.log(`sent ${done}, failed ${failed}, skipped ${skipped}. Log: ${path}`);
  if(failed) process.exitCode=1;
}
if(process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {await main();} catch(error) {console.error(`manage-x-account: ${error.message}`); process.exitCode=1;}
}
