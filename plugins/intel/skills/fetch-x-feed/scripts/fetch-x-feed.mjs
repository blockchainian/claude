import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { loadEnvFile, stateDir } from '../../fetch-x-mentions/scripts/env.mjs';
const exec = promisify(execFile);
export const feedRoot = () => join(stateDir(), 'x', 'feed');
export function parseJson(text) {
  const start = text.search(/[\[{]/);
  if (start < 0) throw new Error('command returned no JSON');
  let depth = 0, quoted = false, escaped = false;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (escaped) escaped = false;
      else if (c === '\\') escaped = true;
      else if (c === '"') quoted = false;
    } else if (c === '"') quoted = true;
    else if (c === '[' || c === '{') depth++;
    else if ((c === ']' || c === '}') && --depth === 0) return JSON.parse(text.slice(start, i + 1));
  }
  throw new Error('command returned incomplete JSON');
}
export async function opencli(args, timeout = 900000) {
  const result = await exec('opencli', ['twitter', ...args, '-f', 'json'], { timeout, maxBuffer: 32 * 1024 * 1024 });
  return parseJson(result.stdout);
}
export function parseArgs(argv) {
  const args = { what: null, window: '24h', limit: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (['--window', '--limit'].includes(arg)) {
      const value = argv[++i];
      if (!value || value.startsWith('--')) throw new Error(`${arg} needs a value`);
      args[arg.slice(2)] = value;
    } else if (!arg.startsWith('--') && args.what === null) args.what = arg;
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (args.what !== 'tldr') {
    let url;
    try { url = new URL(args.what); } catch { throw new Error('use tldr or a trending topic URL; use fetch-x-posts or fetch-x-user-posts for other inputs'); }
    if (url.protocol !== 'https:' || url.hostname !== 'x.com' || !/^\/i\/trending\/\d+\/?$/.test(url.pathname)) throw new Error('expected a trending topic URL');
  }
  if (args.limit !== null) {
    if (!/^[1-9]\d*$/.test(args.limit) || !Number.isSafeInteger(Number(args.limit))) throw new Error('--limit must be a positive integer');
    args.limit = Number(args.limit);
  }
  return args;
}
export function parseWindow(spec = '24h', now = Date.now()) {
  const label = spec.trim().toLowerCase();
  let start, end = now;
  if (label.includes('..')) {
    const dates = label.split('..');
    if (dates.length !== 2 || dates.some(d => !/^\d{4}-\d{2}-\d{2}$/.test(d) || !Number.isFinite(Date.parse(d)) || new Date(d).toISOString().slice(0,10) !== d)) throw new Error('invalid calendar window');
    start = Date.parse(dates[0]); end = Math.min(now, Date.parse(dates[1]) + 86400000 - 1);
  } else {
    const m = /^([1-9]\d*)\s*([hdwm])$/.exec(label);
    if (!m) throw new Error('window must be a duration or a calendar range');
    start = now - Number(m[1]) * {h:3600000,d:86400000,w:604800000,m:60000}[m[2]];
  }
  if (!Number.isFinite(start) || start > end) throw new Error('window starts after its end');
  return {start, end, label};
}
export function filterWindow(rows, { start, end }) {
  if (!Array.isArray(rows)) throw new Error('expected an array of posts');
  const seen = new Map();
  for (const row of rows) {
    if (!row || typeof row.id !== 'string' || !Number.isFinite(Date.parse(row.created_at))) throw new Error('post is missing a valid id or created_at');
    const at = Date.parse(row.created_at);
    if (start <= at && at <= end) seen.set(row.id, row);
  }
  return [...seen.values()].sort((a,b) => Date.parse(b.created_at)-Date.parse(a.created_at));
}
export function median(values) {
  const sorted = [...values].sort((a,b)=>a-b), n = sorted.length;
  return n ? (sorted[Math.floor((n-1)/2)] + sorted[Math.floor(n/2)]) / 2 : 0;
}
export function denseBand(rows) {
  if (rows.length < 24) return null;
  const hours = new Map();
  for (const row of rows) {
    const hour = Math.floor(Date.parse(row.created_at)/3600000)*3600000;
    hours.set(hour,(hours.get(hour) ?? 0)+1);
  }
  if (hours.size < 4) return null;
  const floor = Math.max(1,median([...hours.values()])*.25);
  let edge = Math.max(...hours.keys()), thin = 0;
  for (let hour=edge; hour>=Math.min(...hours.keys()); hour-=3600000) {
    if ((hours.get(hour) ?? 0) < floor) { if (++thin >= 2) break; }
    else {thin=0; edge=hour;}
  }
  const posts=rows.filter(row=>Date.parse(row.created_at)>=edge).length;
  return {from:new Date(edge).toISOString(),posts,share:posts/rows.length};
}
export function pullSize(rows, hours) {
  const stamped=rows.map(r=>Date.parse(r.created_at)).filter(Number.isFinite).sort((a,b)=>b-a);
  const core=stamped.slice(0,Math.max(2,Math.floor(stamped.length*.9)));
  const perHour=stamped.length>=4 ? core.length/Math.max(.25,(core[0]-core.at(-1))/3600000) : rows.length;
  return {perHour,limit:Math.max(150,Math.min(850,Math.floor(perHour*hours*1.35)+50))};
}
const stop = new Set('the a an and or of for to in on at by with from over after amid as is are was were his her its their this that new says said calls more than into out up'.split(' '));
export function distinctive(title) {
  const words=(title.match(/[A-Za-z0-9']+/g) ?? []).filter(w=>!stop.has(w.toLowerCase()));
  const keep=[...words].sort((a,b)=>b.length-a.length).slice(0,3);
  return words.filter(w=>keep.includes(w)).join(' ');
}
function decode(text) {
  return text.replace(/&(?:amp|quot|apos|lt|gt|#\d+|#x[\da-f]+);/gi,entity=>{
    const named={'&amp;':'&','&quot;':'"','&apos;':"'",'&lt;':'<','&gt;':'>'};
    if (named[entity.toLowerCase()]) return named[entity.toLowerCase()];
    return String.fromCodePoint(entity.toLowerCase().startsWith('&#x') ? parseInt(entity.slice(3),16) : parseInt(entity.slice(2),10));
  });
}
export function topicFromHtml(html,url) {
  const meta={url};
  for (const tag of html.match(/<meta\b[^>]*>/gi) ?? []) {
    const attrs=Object.fromEntries([...tag.matchAll(/([\w:-]+)\s*=\s*(["'])(.*?)\2/gs)].map(m=>[m[1].toLowerCase(),decode(m[3])]));
    if (attrs.property==='og:title') meta.title=attrs.content;
    if (attrs.property==='og:description') meta.summary=attrs.content;
  }
  if (!meta.title?.trim()) throw new Error('could not resolve trending topic title; page may be stale');
  return meta;
}
async function searchTopic(query,limit) {
  const script=fileURLToPath(new URL('../../fetch-x-posts/scripts/fetch-x-posts.mjs',import.meta.url));
  const {stdout}=await exec(process.execPath,[script,query,'--limit',String(limit),'--latest'],{timeout:900000,maxBuffer:32*1024*1024});
  return stdout.trim() ? stdout.trim().split('\n').map(line=>JSON.parse(line)) : [];
}
async function main() {
  loadEnvFile();
  const args=parseArgs(process.argv.slice(2)), window=parseWindow(args.window);
  const who=await opencli(['whoami'],30000);
  const account=Array.isArray(who)?who[0]:who;
  if (!account?.username || account.username==='?') throw new Error('could not confirm account; run opencli twitter login');
  const raws=[], notes=[];
  let rows, topic;
  if (args.what==='tldr') {
    const probe=await opencli(['timeline','--type','following','--limit',String(args.limit === null ? 150 : Math.min(args.limit,850))]);
    if (!Array.isArray(probe)) throw new Error('timeline returned no post array');
    raws.push(['raw-probe.json',probe]); rows=probe;
    if (args.limit === null) {
      const size=pullSize(probe,Math.max(1,(window.end-window.start)/3600000));
      notes.push(`Measured ${size.perHour.toFixed(0)} posts/hour; requested ${size.limit} rows. Timeline has no cursor and bottoms out near 850 items.`);
      if (size.limit>150) {
        try {
          const deep=await opencli(['timeline','--type','following','--limit',String(size.limit)]);
          if (!Array.isArray(deep)) throw new Error('timeline returned no post array');
          raws.push(['raw-deep.json',deep]); rows=[...probe,...deep];
        } catch (error) {notes.push(`Deep pull failed; probe data only: ${error.message.split('\n')[0]}`);}
      }
    } else notes.push('Explicit small pull; coverage may be incomplete.');
  } else {
    const response=await fetch(args.what,{signal:AbortSignal.timeout(25000)});
    if (!response.ok) throw new Error(`topic page returned ${response.status}`);
    topic=topicFromHtml(await response.text(),args.what); raws.push(['raw-topic.json',topic]);
    rows=await searchTopic(topic.title,args.limit ?? 60); raws.push(['raw-search.json',rows]);
    notes.push(`Headline query: ${topic.title} (${rows.length} posts).`);
    if (rows.length<15) {
      const alt=distinctive(topic.title);
      if (alt && alt.toLowerCase()!==topic.title.toLowerCase()) {
        const more=await searchTopic(alt,args.limit ?? 60); raws.push(['raw-search-alt.json',more]);
        notes.push(`Distinctive-word query: ${alt} (${more.length} posts).`); rows=[...rows,...more];
      }
    }
  }
  const mode=topic?'topic':'tldr';
  const slug=topic ? '-'+topic.title.toLowerCase().replace(/[^a-z0-9]+/g,'-').replace(/^-|-$/g,'').slice(0,40) : '';
  const archive=join(feedRoot(),`x-${mode}${slug}-${new Date().toISOString().slice(0,10)}`);
  mkdirSync(archive,{recursive:true});
  for (const [name,payload] of raws) writeFileSync(join(archive,name),JSON.stringify(payload,null,2)+'\n');
  const kept=filterWindow(rows,window), band=mode==='tldr'?denseBand(kept):null;
  writeFileSync(join(archive,'feed.json'),JSON.stringify(kept,null,2)+'\n');
  const summary={archive,mode,posts:kept.length,authors:new Set(kept.map(r=>mode==='tldr'?r.author:r.user)).size,window:window.label,oldest:kept.at(-1)?.created_at ?? null,newest:kept[0]?.created_at ?? null,dense_from:band?.from ?? null,dense_share:band?.share ?? null,notes};
  if (!kept.length) notes.push('No posts inside the requested window.');
  writeFileSync(join(archive,'README.md'),`# X feed capture\n\n${JSON.stringify(summary,null,2)}\n\nQuote the dense band. Older sparse hours can reflect the scroll limit, not a quiet feed.\n`);
  console.log(JSON.stringify(summary));
}
if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {await main();} catch(error) {console.error(`fetch-x-feed: ${error.message}`); process.exitCode=1;}
}
