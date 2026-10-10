import { appendFileSync, existsSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative, isAbsolute } from 'node:path';
import { loadEnvFile } from '../../fetch-x-mentions/scripts/env.mjs';
import { feedRoot } from './fetch-x-feed.mjs';
export function saveDigest({root=feedRoot(),archive,text}) {
  if(typeof text!=='string' || !text.trim()) throw new Error('nothing to save: pass the exact digest via --text or stdin');
  if(!archive && existsSync(root)) {
    archive=readdirSync(root).filter(n=>n.startsWith('x-tldr-') && statSync(join(root,n)).isDirectory()).map(n=>join(root,n)).sort((a,b)=>statSync(b).mtimeMs-statSync(a).mtimeMs)[0];
  }
  if(!archive || !existsSync(archive) || !statSync(archive).isDirectory()) throw new Error('no timeline archive found; run fetch-x-feed tldr first');
  const within=relative(realpathSync(root),realpathSync(archive));
  if(within==='..' || within.startsWith('../') || isAbsolute(within)) throw new Error('archive must be inside the feed state directory');
  const saved=join(archive,'tldr.md'), history=join(root,'tldr-history.jsonl');
  writeFileSync(saved,text);
  appendFileSync(history,JSON.stringify({saved_at:new Date().toISOString(),archive,digest:text})+'\n');
  return {saved,history,entries:readFileSync(history,'utf8').trim().split('\n').length};
}
export function parseArgs(argv) {
  const args={};
  for(let i=0;i<argv.length;i++) {
    if(!['--archive','--text'].includes(argv[i])) throw new Error(`unknown argument ${argv[i]}`);
    const key=argv[i].slice(2), value=argv[++i];
    if(value===undefined) throw new Error(`--${key} requires a value`);
    args[key]=value;
  }
  return args;
}
function main() {
  loadEnvFile(); const args=parseArgs(process.argv.slice(2));
  if(args.text===undefined) args.text=readFileSync(0,'utf8');
  console.log(JSON.stringify(saveDigest(args)));
}
if(process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {main();} catch(error) {console.error(`save-x-digest: ${error.message}`); process.exitCode=1;}
}
