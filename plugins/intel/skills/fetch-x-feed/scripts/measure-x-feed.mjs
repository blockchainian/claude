import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { median } from './fetch-x-feed.mjs';
const cjk=/[一-鿿]/;
const contract=/\b[1-9A-HJ-NP-Za-km-z]{32,44}\b/;
const ticker=/\$([A-Za-z][A-Za-z0-9]{1,12})\b/g;
const personas=[
  ['trader / degen',/trader|trading|trench|degen|memecoin|gambl|打狗|战壕|土狗|韭菜/i],
  ['founder / builder',/founder|building|builder|co-?founder|\bceo\b|\bcto\b|建设/i],
  ['call channel / TG',/\btg\b|telegram|channel|频道|calls|signals|群/i],
  ['referral / promo',/\bref\b|referral|返佣|邀请码|use my|sign ?up|discount/i],
  ['content / media',/podcast|youtube|writer|content|educat|artist|host|streamer/i],
  ['investor / advisor',/angel|investor|advisor|\bvc\b|ventures|capital/i],
];
// Timeline and the sibling search skill each have a defined author field.
const authorOf=row=>Object.hasOwn(row,'author')?row.author:row.user;
const count=values=>Object.fromEntries([...new Set(values)].map(v=>[v,values.filter(x=>x===v).length]));
const number=(row,key)=>row[key] ?? 0;
const engagement=row=>number(row,'likes')+3*number(row,'retweets')+2*number(row,'replies');
function groups(rows) {
  const by=new Map();
  for(const row of rows) {
    const author=authorOf(row);
    if(typeof author!=='string' || !author) throw new Error('post is missing its author');
    if(!by.has(author)) by.set(author,[]);
    by.get(author).push(row);
  }
  return by;
}
export function measure(rows,top=25,terms=[]) {
  const by=groups(rows), reach=rows.some(r=>r.views), text=rows.map(r=>r.text ?? '').join('\n');
  const bios=[...by.values()].map(rs=>(rs[0].bio ?? '').replace(/\n/g,' | '));
  const volume=[...by].map(([author,rs])=>({author,posts:rs.length,bio:rs[0].bio ?? '',medianViews:reach?median(rs.map(r=>number(r,'views'))):null,medianReplies:reach?median(rs.map(r=>number(r,'replies'))):null})).sort((a,b)=>b.posts-a.posts);
  const tier=view=>view>=100000?'mega >100k':view>=20000?'large 20-100k':view>=5000?'mid 5-20k':view>=1000?'small 1-5k':'tiny <1k';
  const percentiles=reach?Object.fromEntries(['likes','replies','views'].map(field=>{
    const values=rows.map(r=>number(r,field)).sort((a,b)=>a-b);
    return [field,{median:values[Math.floor(values.length/2)],p90:values[Math.floor(.9*values.length)],max:values.at(-1)}];
  })):null;
  return {
    posts:rows.length,authors:by.size,chinese:rows.filter(r=>cjk.test(r.text ?? '')).length,
    media:rows.filter(r=>r.has_media).length,quotes:rows.filter(r=>r.quoted_tweet || r.quoted).length,
    contractAddresses:rows.filter(r=>contract.test(r.text ?? '')).length,
    percentiles,lowReplyPosts:reach?rows.filter(r=>number(r,'replies')<=2).length:null,
    reachTiers:reach?count(volume.map(v=>tier(v.medianViews))):null,
    personas:Object.fromEntries(personas.map(([label,pattern])=>[label,bios.filter(b=>pattern.test(b)).length])),
    volume:volume.slice(0,top),opportunities:reach?volume.filter(v=>v.posts>=3 && v.medianViews>=3000).map(v=>({...v,viewsPerReply:v.medianViews/(v.medianReplies+1)})).sort((a,b)=>b.viewsPerReply-a.viewsPerReply).slice(0,top):[],
    tickers:Object.fromEntries(Object.entries(count([...text.matchAll(ticker)].map(m=>m[1]))).sort((a,b)=>b[1]-a[1]).slice(0,30)),
    terms:Object.fromEntries(terms.map(term=>[term,text.toLowerCase().split(term.toLowerCase()).length-1])),
    topPosts:[...rows].sort((a,b)=>engagement(b)-engagement(a)).slice(0,top),
  };
}
export function replyShape(text) {
  const s=(text ?? '').replace(/@\w+/g,'').trim();
  if(contract.test(s)) return 'drops a contract address';
  if(/\$[A-Za-z][A-Za-z0-9]{1,12}\b/.test(s)) return 'shills a ticker';
  if(/https?:\/\//.test(s) && s.length<60) return 'bare link drop';
  if([...s].length<=15) return 'one word / emoji';
  if(s.includes('?')) return 'asks a question';
  if([...s].length>=120) return 'long take';
  return 'short opinion';
}
export function measureReplies(replies,feed,top=25) {
  const by=groups(replies), bios=[...by.values()].map(rs=>rs.at(-1).bio ?? '');
  const shapes={};
  for(const row of replies) {
    const shape=replyShape(row.text);
    shapes[shape] ??= {count:0,meanLikes:0};
    shapes[shape].count++; shapes[shape].meanLikes+=number(row,'likes');
  }
  for(const shape of Object.values(shapes)) shape.meanLikes/=shape.count;
  const feedAuthors=new Set(feed.map(authorOf));
  return {sampled:replies.length,authors:by.size,zeroLikes:replies.filter(r=>!r.likes).length,threeOrMoreLikes:replies.filter(r=>r.likes>=3).length,shapes,
    personas:Object.fromEntries([['blank bio',/^\s*$/],...personas].map(([label,pattern])=>[label,bios.filter(b=>pattern.test(b)).length])),
    overlap:[...by.keys()].filter(a=>feedAuthors.has(a)).sort(),topReplies:[...replies].sort((a,b)=>number(b,'likes')-number(a,'likes')).slice(0,top)};
}
export function parseArgs(argv) {
  const args={archive:null,top:25,terms:[]};
  for(let i=0;i<argv.length;i++) {
    if(argv[i]==='--top') {const n=argv[++i]; if(!/^[1-9]\d*$/.test(n) || !Number.isSafeInteger(Number(n))) throw new Error('--top requires a positive integer'); args.top=Number(n);}
    else if(argv[i]==='--terms') {const value=argv[++i]; if(value===undefined) throw new Error('--terms requires a value'); args.terms=value.split(',').map(t=>t.trim()).filter(Boolean);}
    else if(!argv[i].startsWith('--') && args.archive===null) args.archive=argv[i];
    else throw new Error(`unknown argument ${argv[i]}`);
  }
  if(!args.archive) throw new Error('give an archive directory');
  return args;
}
function main() {
  const {archive,top,terms}=parseArgs(process.argv.slice(2));
  const rows=JSON.parse(readFileSync(join(archive,'feed.json'),'utf8'));
  const report=measure(rows,top,terms), repliesPath=join(archive,'replies.json');
  if(existsSync(repliesPath)) report.replyAnalysis=measureReplies(JSON.parse(readFileSync(repliesPath,'utf8')),rows,top);
  console.log(JSON.stringify(report,null,2));
}
if(process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {main();} catch(error) {console.error(`measure-x-feed: ${error.message}`); process.exitCode=1;}
}
