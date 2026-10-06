#!/usr/bin/env node
// ABOUTME: Finds the most downloaded EPUB on one Anna's Archive search page and its download links, and with --out downloads it.
// ABOUTME: All site requests go through a headed Chrome session that passes the DDoS-Guard check.

import { createWriteStream, realpathSync } from 'node:fs';
import { open, readFile, rename, rm } from 'node:fs/promises';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { parseEnv } from 'node:util';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { isChallenge, openSession } from './site-session.mjs';

const DEFAULT_BASE = 'https://annas-archive.pk';
const MD5_LINK = /^\/md5\/([0-9a-f]{32})\/?$/;
const SLOW_LINK = /^\/slow_download\/([0-9a-f]{32})\/\d+\/\d+\/?$/;
const VOID_TAGS = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'source', 'wbr']);

function decodeHtml(value) {
  return value.replace(/&(#(?:x[\da-f]+|\d+)|amp|lt|gt|quot|apos|nbsp);/gi, (entity, name) => {
    if (name.startsWith('#')) {
      const point = name[1]?.toLowerCase() === 'x' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return point <= 0x10ffff ? String.fromCodePoint(point) : entity;
    }
    return { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' }[name.toLowerCase()];
  });
}

export function parseLinks(html) {
  const anchors = [];
  const copyUrls = [];
  const stack = [];
  let listCount = 0;
  const cleaned = html.replace(/<(script|style)\b[^>]*>[\s\S]*?<\/\1\s*>/gi, '');
  for (const token of cleaned.match(/<!--[\s\S]*?-->|<![^>]*>|<[^>]*>|[^<]+/g) ?? []) {
    if (token.startsWith('<!--') || token.startsWith('<!')) continue;
    const end = /^<\/([a-z][\w:-]*)\s*>$/i.exec(token);
    if (end) {
      const index = stack.findLastIndex(frame => frame.tag === end[1].toLowerCase());
      if (index < 0) continue;
      for (const frame of stack.splice(index).reverse()) {
        if (frame.tag === 'a') anchors.push(frame.anchor);
        if (frame.tag === 'span' && frame.copy && httpUrl(frame.text.trim())) copyUrls.push(frame.text.trim());
        if (frame.tag === 'span' && frame.downloads && frame.row) {
          const count = Number(frame.text.trim().replaceAll(',', ''));
          if (Number.isSafeInteger(count) && count >= 0) frame.row.downloadsTotal = count;
        }
      }
      continue;
    }
    const start = /^<([a-z][\w:-]*)(?:\s|>|\/)/i.exec(token);
    if (start) {
      const tag = start[1].toLowerCase();
      const attrs = {};
      const body = token.slice(start[0].length - 1, -1);
      for (const match of body.matchAll(/([\w:-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g)) {
        attrs[match[1].toLowerCase()] = decodeHtml(match[2] ?? match[3] ?? match[4] ?? '');
      }
      const classes = (attrs.class ?? '').split(/\s+/);
      if (tag === 'div' && classes.includes('js-aarecord-list-outer')) listCount++;
      const li = [...stack].reverse().find(frame => frame.tag === 'li');
      const list = [...stack].reverse().find(frame => frame.listIndex);
      const listPosition = stack.findLastIndex(parent => parent.listIndex);
      const row = listPosition >= 0 ? stack[listPosition + 1] : null;
      const frame = { tag, classes, text: '', row, listIndex: tag === 'div' && classes.includes('js-aarecord-list-outer') ? listCount : undefined };
      if (tag === 'a') {
        frame.anchor = { href: attrs.href ?? '', text: '', parents: stack.map(parent => ({ tag: parent.tag, classes: parent.classes })), listIndex: list?.listIndex ?? null, li, row };
      }
      if (tag === 'span') frame.copy = classes.includes('bg-gray-200');
      if (tag === 'span') frame.downloads = attrs.title === 'Downloads' && classes.includes('whitespace-nowrap');
      if (!VOID_TAGS.has(tag) && !token.endsWith('/>')) stack.push(frame);
      continue;
    }
    const data = decodeHtml(token);
    for (const frame of stack) {
      frame.text += data;
      if (frame.anchor) frame.anchor.text += data;
    }
  }
  for (const anchor of anchors) anchor.text = anchor.text.replace(/\s+/g, ' ').trim();
  return { anchors, copyUrls };
}

function httpUrl(value) {
  try { return ['http:', 'https:'].includes(new URL(value).protocol); }
  catch { return false; }
}

let session = null;

async function get(url, follow = true) {
  session ??= openSession(new URL(url).origin);
  return (await session).get(url, follow);
}

function checkedHtml({ status, body }, label) {
  if (isChallenge(body)) throw new Error(`${label}未通过浏览器验证（可能是验证码）；可用浏览器保存页面后传入对应的 --*-html 文件`);
  if (status !== 200) throw new Error(`${label}返回 HTTP ${status}`);
  return body;
}

function searchResults(body) {
  const records = new Map();
  for (const anchor of parseLinks(body).anchors) {
    const match = MD5_LINK.exec(anchor.href);
    if (match && anchor.listIndex === 1 && anchor.text && !records.has(match[1])) {
      records.set(match[1], { title: anchor.text, downloadsTotal: anchor.row?.downloadsTotal });
    }
  }
  return records;
}

async function metric(base, md5) {
  const { status, body } = await get(`${base}/dyn/md5/inline_info/${md5}`);
  if (status !== 200) throw new Error(`指标接口返回 HTTP ${status}`);
  let data;
  try { data = JSON.parse(body); }
  catch { throw new Error('指标接口未返回 downloads_total'); }
  if (!Object.hasOwn(data ?? {}, 'downloads_total')) throw new Error('指标接口未返回 downloads_total');
  if (!Number.isSafeInteger(data.downloads_total) || data.downloads_total < 0) throw new Error('downloads_total 无效');
  return data.downloads_total;
}

export async function readMemberKey(configPath = join(homedir(), '.config', 'intel', '.env')) {
  const config = parseEnv(await readFile(configPath, 'utf8'));
  const key = config.ANNA_ARCHIVE_SECRET_KEY;
  if (!key?.trim()) throw new Error(`请在 ${configPath} 配置 ANNA_ARCHIVE_SECRET_KEY`);
  return key;
}

async function fastUrl(base, md5) {
  const query = new URLSearchParams({ md5, key: await readMemberKey() });
  const { status, body } = await get(`${base}/dyn/api/fast_download.json?${query}`, false);
  let data;
  try { data = JSON.parse(body); }
  catch { return { status, error: '接口没有返回 JSON' }; }
  const url = data?.download_url;
  if ([200, 204].includes(status) && typeof url === 'string' && httpUrl(url)) return { status, url };
  return { status, error: typeof data?.error === 'string' ? data.error : '没有返回下载链接' };
}

function directUrl(html) {
  const links = parseLinks(html);
  for (const anchor of links.anchors) {
    const paragraph = anchor.parents.some(parent => parent.tag === 'p' && parent.classes.includes('text-xl') && parent.classes.includes('font-bold'));
    if (paragraph && httpUrl(anchor.href)) return anchor.href;
  }
  return links.copyUrls[0] ?? null;
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

const isNoWaitlist = anchor => (anchor.li?.text ?? '').toLowerCase().includes('no waitlist');

// The waitlist servers ("slightly faster but with waitlist") download at megabytes per second after a
// ~50s server-side queue; the "no waitlist" servers are immediate but throttled to tens of KB/s. Lists the
// waitlist servers first, then the no-waitlist ones, each in page order.
export function orderSlowPaths(anchors, md5) {
  const paths = anchors.filter(anchor => SLOW_LINK.exec(anchor.href)?.[1] === md5);
  return [...paths.filter(anchor => !isNoWaitlist(anchor)), ...paths.filter(isNoWaitlist)];
}

// Prefers the first waitlist server, keeping the first no-waitlist one as a fallback that always yields a link.
export function selectSlowPaths(anchors, md5) {
  const paths = orderSlowPaths(anchors, md5);
  const primary = paths[0] ?? null;
  const fallback = paths.find(anchor => isNoWaitlist(anchor) && anchor !== primary) ?? null;
  return { primary, fallback };
}

const SLOW_POLL_MS = 8_000;
const SLOW_WAIT_MS = 130_000;

// The entry first redirects to itself with a bare "?" before serving the waitlist page; a redirect anywhere else is refused.
export function leftEntry(entry, landed) {
  const from = new URL(entry);
  const to = new URL(landed);
  return from.origin !== to.origin || from.pathname !== to.pathname;
}

// Polls an entry until its direct link appears (the waitlist page reveals it only after the queue),
// or the deadline passes. Returns the link, a redirect error, or the last response for the caller to judge.
async function resolveSlow(base, entryHref, deadline) {
  const entry = new URL(entryHref, base).href;
  let last = { status: 0, body: '' };
  do {
    const { status, url: landed, body } = await get(entry);
    if (leftEntry(entry, landed)) return { entry, error: '入口跳转到别处，未跟随' };
    last = { status, body };
    const url = status === 200 ? directUrl(body) : null;
    if (url) return { entry, status, source: 'live', url };
    if (Date.now() < deadline) await sleep(SLOW_POLL_MS);
  } while (Date.now() < deadline);
  return { entry, status: last.status, body: last.body };
}

async function slowUrl(base, md5, detail, savedSlowHtml) {
  const { primary, fallback } = selectSlowPaths(parseLinks(detail).anchors, md5);
  if (!primary) return { error: '详情页没有慢速下载入口' };
  // A saved slow page means the live entry is blocked, so do not sit through the queue: try live once.
  const wait = savedSlowHtml !== null ? 0 : SLOW_WAIT_MS;
  let result = await resolveSlow(base, primary.href, Date.now() + wait);
  if (result.url) return result;
  if (result.error) return { entry: result.entry, error: result.error };
  if (fallback) {
    const alt = await resolveSlow(base, fallback.href, Date.now());
    if (alt.url) return alt;
    if (alt.status) result = alt;
  }
  if (savedSlowHtml !== null) {
    if (!savedSlowHtml.includes(`/md5/${md5}`)) return { entry: result.entry, status: result.status, error: '保存的慢速页面与获选记录不符' };
    const savedUrl = directUrl(savedSlowHtml);
    if (savedUrl) return { entry: result.entry, status: result.status, source: 'saved_html', url: savedUrl };
  }
  const error = isChallenge(result.body ?? '') ? '未通过浏览器验证' : result.status !== 200 ? '慢速入口不可用' : '仍在等待或页面未提供直链';
  return { entry: result.entry, status: result.status, error };
}

async function isEpub(path) {
  const file = await open(path);
  try {
    const { buffer, bytesRead } = await file.read(Buffer.alloc(4), 0, 4, 0);
    return bytesRead === 4 && buffer.equals(Buffer.from('PK\x03\x04'));
  } finally { await file.close(); }
}

// Downloads from the first link not refused with 429 (too many downloads at once from this IP). Each resolver
// runs only after the link before it was refused, because resolving a slow entry means sitting through its queue.
export async function downloadFirst(resolvers, out, fetchFile = fetch) {
  const refused = [];
  for (const resolve of resolvers) {
    const url = await resolve();
    if (!url) continue;
    const response = await fetchFile(url);
    if (response.status === 429) {
      refused.push(url);
      await response.body?.cancel();
      continue;
    }
    if (!response.ok) throw new Error(`下载返回 HTTP ${response.status}`);
    const part = `${out}.part`;
    try {
      await pipeline(Readable.fromWeb(response.body), createWriteStream(part));
      if (!(await isEpub(part))) throw new Error('下载的文件不是 EPUB（可能是错误页）');
      await rename(part, out);
    } finally { await rm(part, { force: true }); }
    return { path: out, url, refused };
  }
  throw new Error(refused.length ? `${refused.length} 个链接都返回 429（这个 IP 同时下载太多）` : '没有可用的下载链接');
}

// Download order: the fast link, the slow link already resolved, then every other slow entry on the detail page.
function downloadResolvers(base, detail, md5, fast, slow) {
  const others = orderSlowPaths(parseLinks(detail).anchors, md5).map(anchor => new URL(anchor.href, base).href).filter(entry => entry !== slow.entry);
  return [() => fast.url, () => slow.url, ...others.map(entry => async () => (await resolveSlow(base, entry, Date.now() + SLOW_WAIT_MS)).url)];
}

function args(argv) {
  if (argv.includes('--help') || argv.includes('-h')) {
    console.log('用法：node <anna-archive-links.mjs 路径> <书名> [--search-html 文件] [--detail-html 文件] [--slow-html 文件] [--out 文件]\n给出 --out 时下载到该文件：某个链接返回 429 就换下一个链接。\n会员密钥从 ~/.config/intel/.env 的 ANNA_ARCHIVE_SECRET_KEY 读取，不读取环境变量。');
    process.exit(0);
  }
  const options = { baseUrl: DEFAULT_BASE };
  const keys = { '--search-html': 'searchHtml', '--detail-html': 'detailHtml', '--slow-html': 'slowHtml', '--base-url': 'baseUrl', '--out': 'out' };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] in keys) {
      if (!argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error(`${argv[i]} 缺少值`);
      options[keys[argv[i]]] = argv[++i];
    } else if (!argv[i].startsWith('-') && !options.title) options.title = argv[i];
    else throw new Error(`未知参数：${argv[i]}`);
  }
  if (!options.title) throw new Error('请提供书名或搜索词');
  return options;
}

async function run(options) {
  const base = options.baseUrl.replace(/\/$/, '');
  if (!httpUrl(base)) throw new Error('base-url 必须是 HTTP(S) URL');
  const search = options.searchHtml
    ? await readFile(options.searchHtml, 'utf8')
    : checkedHtml(await get(`${base}/search?${new URLSearchParams({ q: options.title, ext: 'epub' })}`), '搜索页');
  const records = searchResults(search);
  if (!records.size) throw new Error('未找到 EPUB 结果；请检查搜索词和页面内容');
  const ids = [...records.keys()];
  const counts = new Map(ids.filter(md5 => records.get(md5).downloadsTotal !== undefined).map(md5 => [md5, records.get(md5).downloadsTotal]));
  const missing = ids.filter(md5 => !counts.has(md5));
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(4, missing.length) }, async () => {
    while (next < missing.length) {
      const md5 = missing[next++];
      try { counts.set(md5, await metric(base, md5)); }
      catch (error) { throw new Error(`无法比较全部结果：${md5}: ${error.message}`); }
    }
  }));
  const winner = ids.reduce((best, md5) => counts.get(md5) > counts.get(best) || (counts.get(md5) === counts.get(best) && md5 > best) ? md5 : best);
  // A detail page that fails to load costs only the slow links; the fast link does not need it.
  const detail = options.detailHtml
    ? await readFile(options.detailHtml, 'utf8')
    : await get(`${base}/md5/${winner}`).then(page => checkedHtml(page, '详情页'), error => error);
  const savedSlowHtml = options.slowHtml ? await readFile(options.slowHtml, 'utf8') : null;
  const [fast, slow] = await Promise.all([
    fastUrl(base, winner).catch(error => ({ error: error.message })),
    detail instanceof Error ? { error: detail.message } : slowUrl(base, winner, detail, savedSlowHtml).catch(error => ({ error: error.message })),
  ]);
  const download = options.out
    ? await downloadFirst(downloadResolvers(base, detail instanceof Error ? '' : detail, winner, fast, slow), options.out).catch(error => ({ error: error.message }))
    : undefined;
  console.log(JSON.stringify({ title: records.get(winner).title, md5: winner, downloads_total: counts.get(winner), compared: records.size, detail_url: `${base}/md5/${winner}`, fast, slow, download }, null, 2));
}

async function main() {
  try { await run(args(process.argv.slice(2))); }
  finally { await (await session)?.close(); }
}

// Node resolves the main module through symlinks, so argv[1] must be resolved the same way.
if (pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url) {
  main().catch(error => { console.error(`错误：${error.message}`); process.exitCode = 1; });
}
