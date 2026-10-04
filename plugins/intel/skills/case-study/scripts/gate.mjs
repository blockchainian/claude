#!/usr/bin/env node
// ABOUTME: Machine-wide gate for the fetch commands of case-study runs: every agent of every workflow calls the
// ABOUTME: shared services (Jina, Exa, Wayback, Chrome, yt-dlp, GDELT, Google News, X) through it, so limits are kept across runs.
//
// Usage: gate.mjs read <url>              page text: Jina reader over the proxy exits, then Exa fetch, then Chrome
//        gate.mjs search "<query>" [n]    Exa web search, then DuckDuckGo and Yahoo when Exa is out of credits
//        gate.mjs wayback <args...>       this skill's wayback.mjs, one run at a time
//        gate.mjs chrome <args...>        opencli <args...>, at most CHROME_SLOTS at once; Google searches paced
//        gate.mjs yt <args...>            yt-dlp <args...> (Chrome cookies added for YouTube), at most YT_SLOTS at once
//        gate.mjs fetch-x-posts <args...> the fetch-x-posts script named by FETCH_X_POSTS: X search on an account pool, one JSON post per line
//        gate.mjs ytsearch "<query>" "<name>"...   YouTube search, only the videos whose title, channel or description has one of the names
//        gate.mjs ytuploads <channel url>          every upload of a YouTube channel with its exact date and plays, one JSON video per line, oldest first
//        gate.mjs gdelt "<name>"... [<from> <to>]  news articles GDELT found the names in (BigQuery), up to 100 names, dates YYYY-MM-DD
//        gate.mjs gnews "<name>" [<from> <to>]    Google News articles for the name, asked for week by week
//        gate.mjs stats                   calls and failures per command since the log began
// gdelt and gnews print one JSON article per line, oldest first, and keep what they fetched in
// ~/.local/share/case-study/<gdelt|gnews>/<name>/: articles.jsonl and, beside it, articles.out.json with the days held.
// State (pace files, slot locks, the log) lives in ~/.cache/case-study-limits, shared with fetch-x-posts. Settings come
// from the .env file env.mjs finds: ISP_PROXY_URL (one URL; the ten ports after its own are the exits; without it every
// request goes direct), RESIDENTIAL_PROXY_URL (Google News asked again through it when an exit is refused),
// FETCH_X_POSTS (the fetch-x-posts script) and GDELT_BQ_PROJECT (the Google Cloud project the BigQuery queries run in).
import { spawn, spawnSync } from 'node:child_process'
import { appendFileSync, closeSync, existsSync, mkdirSync, openSync, readFileSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadEnv } from './env.mjs'

loadEnv()

export const STATE = process.env.CASE_STUDY_LIMITS || join(homedir(), '.cache', 'case-study-limits')
export const DATA = process.env.CASE_STUDY_DATA || join(homedir(), '.local', 'share', 'case-study')
const HERE = dirname(fileURLToPath(import.meta.url))
const WAYBACK = join(HERE, 'wayback.mjs')
const JINA_GAP = 3.5 // seconds between requests on one exit: Jina allows 20 a minute per IP without a key
const CHROME_SLOTS = 6
const YT_SLOTS = 3
const YT_GAP = 2 // seconds between YouTube requests machine-wide: the logged-in session is shared by every run
const EXITS = 10
const GDELT_START = '2017-01-01' // the first day asked for when no range is given
const GDELT_NAMES = 100 // the most names one call may ask for
// The most one GDELT query (a year at most) may read. Measured 2026-10: 52 GB for one year of names.
const GDELT_MAX_BYTES = 120e9
const OUTPUT_MAX = 512e6 // bytes of one BigQuery answer; a Node string holds 537 MB
const GNEWS_GAP = 0.1 // seconds between two Google News requests machine-wide: ten a second, spread over the ISP exits
const GNEWS_WEEKS = EXITS // weeks one gnews call reads at once
const YT_RESULTS = 20 // videos asked of one YouTube search, before the name filter
const YT_BATCH = 8 // video pages one yt-dlp run reads
const YT_ROUTE_RUNS = 3 // yt-dlp runs at once on one route, when reading video pages without the login
const YT_TABS = ['videos', 'shorts', 'streams']

const sleep = ms => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, Math.max(0, Math.ceil(ms)))
const now = () => Date.now() / 1000

// The ten exits of the proxy: the ten ports after the URL's own (which is the rotating entry).
export function proxies(base = process.env.ISP_PROXY_URL) {
  if (!base) return []
  return Array.from({ length: EXITS }, (_, n) => base.replace(/:(\d+)$/, (_, port) => `:${Number(port) + n + 1}`))
}

export function log(command, status, detail = '', state = STATE) {
  mkdirSync(state, { recursive: true })
  const time = new Date().toTimeString().slice(0, 8)
  appendFileSync(join(state, 'log.tsv'), `${time}\t${command}\t${status}\t${detail.split(/\s+/).join(' ').slice(0, 120)}\n`)
}

const alive = pid => { try { process.kill(pid, 0); return true } catch (error) { return error.code === 'EPERM' } }

// A lock file holding its owner's pid; a lock left by a process that is gone is taken over.
function tryLock(path) {
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const fd = openSync(path, 'wx')
      writeFileSync(fd, String(process.pid))
      closeSync(fd)
      return () => { try { rmSync(path, { force: true }) } catch {} }
    } catch (error) {
      if (error.code !== 'EEXIST') throw error
      let owner = NaN
      try { owner = Number(readFileSync(path, 'utf8')) } catch {}
      if (owner && alive(owner)) return null
      try { rmSync(path, { force: true }) } catch {}
    }
  }
  return null
}

function withLock(path, fn) {
  let release
  while (!(release = tryLock(path))) sleep(100)
  try { return fn() } finally { release() }
}

// Blocks until `gap` seconds have passed since the last call with this key, on this machine.
export function paced(key, gap, state = STATE) {
  mkdirSync(state, { recursive: true })
  const file = join(state, `${key}.pace`)
  withLock(`${file}.lock`, () => {
    let last = 0
    try { last = Number(readFileSync(file, 'utf8')) || 0 } catch {}
    sleep((last + gap - now()) * 1000)
    writeFileSync(file, String(now()))
  })
}

// The same lock held while an async fn runs, waiting for it without blocking the process.
async function withLockAsync(path, fn) {
  let release
  while (!(release = tryLock(path))) await new Promise(resolve => setTimeout(resolve, 100))
  try { return await fn() } finally { release() }
}

// paced without blocking the process: each call reserves the next free moment under the lock and waits for it outside.
export async function pacedAsync(key, gap, state = STATE) {
  mkdirSync(state, { recursive: true })
  const file = join(state, `${key}.pace`)
  const at = withLock(`${file}.lock`, () => {
    let last = 0
    try { last = Number(readFileSync(file, 'utf8')) || 0 } catch {}
    const next = Math.max(now(), last + gap)
    writeFileSync(file, String(next))
    return next
  })
  await new Promise(resolve => setTimeout(resolve, Math.max(0, (at - now()) * 1000)))
}

// The index of an exit whose pace gap has passed, waiting until one has.
export function freeRoute(count, gap, state = STATE) {
  mkdirSync(state, { recursive: true })
  for (;;) {
    const order = Array.from({ length: count }, (_, i) => i).sort(() => Math.random() - 0.5)
    for (const i of order) {
      const file = join(state, `jina-${i}.pace`)
      const taken = withLock(`${file}.lock`, () => {
        let last = 0
        try { last = Number(readFileSync(file, 'utf8')) || 0 } catch {}
        if (last + gap > now()) return false
        writeFileSync(file, String(now()))
        return true
      })
      if (taken) return i
    }
    sleep(300)
  }
}

// Holds one of `count` machine-wide slots while fn runs; tryOnce gives up instead of waiting.
export function slot(name, count, fn, { state = STATE, tryOnce = false } = {}) {
  mkdirSync(state, { recursive: true })
  for (;;) {
    for (let i = 0; i < count; i++) {
      const release = tryLock(join(state, `${name}-${i}.slot`))
      if (release) {
        try { return fn() } finally { release() }
      }
    }
    if (tryOnce) return undefined
    sleep(500)
  }
}

const run = (command, args, options = {}) => spawnSync(command, args, { encoding: 'utf8', maxBuffer: 1 << 28, ...options })

// run, without blocking the process: resolves with the status and the output once the command ends.
const runAsync = (command, args) => new Promise(resolve => {
  const child = spawn(command, args)
  let stdout = ''
  let stderr = ''
  child.stdout.setEncoding('utf8').on('data', chunk => { stdout += chunk })
  child.stderr.setEncoding('utf8').on('data', chunk => { stderr += chunk })
  child.on('error', () => resolve({ status: null, stdout, stderr }))
  child.on('close', status => resolve({ status, stdout, stderr }))
})

// Exa's answer, or '' when the account is out of credits (asked again every 10 minutes).
export function exa(tool, params, { tries = 6, state = STATE, call = (args) => run('mcporter', args) } = {}) {
  const outFile = join(state, 'exa-out-of-credits')
  if (existsSync(outFile) && now() - statSync(outFile).mtimeMs / 1000 < 600) return ''
  let out = ''
  for (let attempt = 0; attempt < tries; attempt++) {
    const r = call(['call', `exa.${tool}`, ...params])
    out = (r.stdout || '') + (r.stderr || '')
    if (out.includes('error (402)')) {
      mkdirSync(state, { recursive: true })
      writeFileSync(outFile, '')
      return ''
    }
    if (!out.includes('error (429)')) return out
    sleep((1 + Math.random() * 2 * (attempt + 1)) * 1000)
  }
  return ''
}

// [engine, results] from the first engine that answers: Exa, then Yahoo, then DuckDuckGo.
export function search(query, count = 8) {
  const out = exa('web_search_exa', [`query=${query}`, `numResults=${count}`])
  if (out) return ['exa', out]
  for (const engine of ['yahoo', 'duckduckgo']) {
    paced(engine, 2)
    const r = run('opencli', [engine, 'search', query, '-f', 'yaml'])
    if (r.status === 0 && r.stdout.includes('url:')) return [engine, r.stdout]
  }
  return ['FAILED', 'search failed on every engine (Exa is out of credits; DuckDuckGo and Yahoo gave nothing)']
}

// The curl call that reads a page through the Jina reader, through one exit or direct.
export function readCommand(url, proxy) {
  return ['curl', '-s', '-m', '60', '-A', 'curl/8', '-w', '\n%{http_code}', ...(proxy ? ['-x', proxy] : []), `https://r.jina.ai/${url}`]
}

export function read(url) {
  const routes = [null, ...proxies()]
  let best = ''
  for (let attempt = 0; attempt < 4; attempt++) {
    const i = freeRoute(routes.length, JINA_GAP)
    const [command, ...args] = readCommand(url, routes[i])
    const r = run(command, args)
    if (r.status !== 0) continue
    const cut = r.stdout.lastIndexOf('\n')
    const text = r.stdout.slice(0, cut)
    const code = r.stdout.slice(cut + 1).trim()
    if (code === '429') continue // this exit is over its limit: take another
    if (code !== '200') break
    if (text.length > 1500 && !text.slice(0, 600).includes('requiring CAPTCHA')) {
      log('read', 'jina', url)
      return text
    }
    best = text
    break // the reader answered, the page itself is blocked or short: another exit will not change that
  }
  const fetched = exa('web_fetch_exa', [`urls=["${url}"]`, 'maxCharacters=60000'])
  if (fetched.length > 1500) {
    log('read', 'exa', url)
    return fetched
  }
  const r = slot('chrome', CHROME_SLOTS, () => run('opencli', ['web', 'read', '--url', url, '--stdout', 'true', '--download-images', 'false', '--window', 'background']))
  if (r.status === 0 && r.stdout.length > 1500) {
    log('read', 'chrome', url)
    return r.stdout
  }
  log('read', 'SHORT', url)
  return [best, fetched, r.stdout || ''].sort((a, b) => b.length - a.length)[0]
}

const day = date => date.toISOString().slice(0, 10)
const addDays = (date, n) => day(new Date(Date.parse(`${date}T00:00:00Z`) + n * 864e5))

// The parts of from..to (days, both included) that the ranges in `covered` do not hold.
export function missingDays(covered, from, to) {
  const gaps = []
  let next = from
  for (const [a, b] of covered) {
    if (b < next) continue
    if (a > to) break
    if (a > next) gaps.push([next, addDays(a, -1)])
    next = addDays(b, 1)
  }
  if (next <= to) gaps.push([next, to])
  return gaps
}

// `covered` with from..to added; ranges that touch are joined.
export function coverDays(covered, from, to) {
  const merged = []
  for (const [a, b] of [...covered, [from, to]].sort((x, y) => x[0] < y[0] ? -1 : 1)) {
    const last = merged.at(-1)
    if (last && a <= addDays(last[1], 1)) { if (b > last[1]) last[1] = b } else merged.push([a, b])
  }
  return merged
}

function writeWhole(path, text) {
  writeFileSync(`${path}.part`, text)
  renameSync(`${path}.part`, path)
}

// The articles kept for one name of one source: <data>/<source>/<name>/articles.jsonl, one per line, oldest first, one
// per url, and articles.out.json beside it with the days already fetched. The .out.json is what decides what to fetch.
// A name as it is kept: lower case, single spaces; and the folder it is kept under.
const nameKey = name => String(name || '').toLowerCase().split(/\s+/).filter(Boolean).join(' ')
const nameFolder = key => key.replace(/[\s/\\]+/g, '-')

function archive(source, name, data) {
  const key = nameKey(name)
  if (!key || key.includes('://')) throw Error('give the name to look for, not a URL')
  const folder = join(data, source, nameFolder(key))
  const file = join(folder, 'articles.jsonl')
  const progress = join(folder, 'articles.out.json')
  let covered = []
  let articles = []
  if (existsSync(progress)) {
    const held = JSON.parse(readFileSync(progress, 'utf8'))
    if (held.name !== key) throw Error(`${folder} holds another name: ${held.name}`)
    covered = held.covered
    articles = readFileSync(file, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line))
  }
  return {
    key,
    get covered() { return covered },
    between: (from, to) => articles.filter(a => a.date >= from && a.date <= to),
    // adds the articles (the first one kept per url) and the days from..to, and writes both files
    add(found, from, to) {
      const byUrl = new Map(articles.map(a => [a.url, a]))
      for (const a of found) if (!byUrl.has(a.url) || a.date < byUrl.get(a.url).date) byUrl.set(a.url, a)
      articles = [...byUrl.values()].sort((x, y) => x.date < y.date ? -1 : x.date > y.date ? 1 : 0)
      if (from <= to) covered = coverDays(covered, from, to)
      mkdirSync(folder, { recursive: true })
      writeWhole(file, articles.map(a => JSON.stringify(a) + '\n').join(''))
      writeWhole(progress, JSON.stringify({ name: key, covered, count: articles.length }))
    },
  }
}

// from..to as given or by default, the end never after today; throws on anything that is not a day.
function dayRange(from, to, start, now) {
  const today = day(now)
  from ||= start
  to = !to || to > today ? today : to
  if (![from, to].every(d => /^\d{4}-\d{2}-\d{2}$/.test(d)) || from > to) throw Error('dates are YYYY-MM-DD, <from> before <to>')
  return [from, to, addDays(today, -1)]
}

// The bq call that lists, for each of `names`, the articles of the days from..to in whose names GDELT found it.
export function gdeltCommand(names, from, to, project) {
  const sql = `SELECT name, DocumentIdentifier AS url, CAST(DATE AS STRING) AS seen, SourceCommonName AS domain,
  DIV(LENGTH(LOWER(AllNames)) - LENGTH(REPLACE(LOWER(AllNames), name, '')), LENGTH(name)) AS mentions
FROM \`gdelt-bq.gdeltv2.gkg_partitioned\` JOIN UNNEST(@names) AS name ON STRPOS(LOWER(AllNames), name) > 0
WHERE _PARTITIONTIME >= @from AND _PARTITIONTIME < @to`
  return ['--quiet', '--headless', `--project_id=${project}`, '--format=json', 'query', '--use_legacy_sql=false', '--max_rows=10000000',
    `--maximum_bytes_billed=${GDELT_MAX_BYTES}`, `--parameter=names:ARRAY<STRING>:${JSON.stringify(names)}`,
    `--parameter=from:TIMESTAMP:${from} 00:00:00`, `--parameter=to:TIMESTAMP:${addDays(to, 1)} 00:00:00`, sql]
}

// The articles GDELT found each of `names` in between two days, per name oldest first: { name, url, domain, date,
// mentions }. GDELT lists the names it recognised in an article's text (not the text itself); the date is the day it
// collected the article. A query costs what the days it reads cost, whatever the number of names, so only the days a
// name lacks are read, a year at a time, with every name that lacks them in the same query. A day is held once it has
// ended: today is read again each time.
export function gdelt(names, from, to, { data = DATA, state = STATE, project = process.env.GDELT_BQ_PROJECT, now = new Date(), call = args => run('bq', args, { maxBuffer: OUTPUT_MAX }) } = {}) {
  if (!names.length) throw Error('give the name to look for: gate.mjs gdelt "<name>"... [<from> <to>]')
  if (names.length > GDELT_NAMES) throw Error(`at most ${GDELT_NAMES} names in one call`)
  const [start, end, yesterday] = dayRange(from, to, GDELT_START, now)
  if (!project) throw Error('GDELT_BQ_PROJECT (the Google Cloud project the BigQuery queries run in) is not set in the .env file')
  mkdirSync(state, { recursive: true })
  return withLock(join(state, 'gdelt.lock'), () => {
    const kept = [...new Map(names.map(name => archive('gdelt', name, data)).map(a => [a.key, a])).values()]
    for (let year = Number(start.slice(0, 4)); year <= Number(end.slice(0, 4)); year++) {
      const inYear = ([a, b]) => [a < `${year}-01-01` ? `${year}-01-01` : a, b > `${year}-12-31` ? `${year}-12-31` : b]
      const lacking = kept.map(a => ({ a, gaps: missingDays(a.covered, start, end).map(inYear).filter(([x, y]) => x <= y) })).filter(l => l.gaps.length)
      if (!lacking.length) continue
      const first = lacking.map(l => l.gaps[0][0]).sort()[0]
      const last = lacking.map(l => l.gaps.at(-1)[1]).sort().at(-1)
      const r = call(gdeltCommand(lacking.map(l => l.a.key), first, last, project))
      let rows
      try { rows = r.status === 0 ? JSON.parse(r.stdout.trim() || '[]') : null } catch { rows = null }
      if (r.error?.code === 'ENOBUFS') throw Error(`the answer for ${year} is over ${OUTPUT_MAX / 1e6} MB: ask for fewer names or fewer days (${lacking.map(l => l.a.key).join(', ')})`)
      if (!rows) throw Error(((r.stdout || '') + (r.stderr || '') || r.error?.message || 'bq failed').trim().split('\n').at(-1).slice(0, 300))
      for (const { a } of lacking) {
        const found = rows.filter(row => row.name === a.key).map(row => ({ url: row.url, domain: row.domain, date: `${row.seen.slice(0, 4)}-${row.seen.slice(4, 6)}-${row.seen.slice(6, 8)}`, mentions: Number(row.mentions) }))
        a.add(found, first, last > yesterday ? yesterday : last)
      }
    }
    return kept.flatMap(a => a.between(start, end).map(article => ({ name: a.key, ...article })))
  })
}

// One Google News request, compressed, through a random ISP exit; when Google refuses that exit (429, 503) or it fails,
// asked again through the residential proxy. Google refuses an address it has seen too often, so neither goes direct
// unless no proxy is set.
export async function gnewsRequest(url, form, { exits = proxies(), residential = (process.env.RESIDENTIAL_PROXY_URL || '').trim(), curl = args => runAsync('curl', args) } = {}) {
  const routes = [exits.length ? exits[Math.floor(Math.random() * exits.length)] : null, ...(residential ? [residential] : [])]
  let answer
  for (const proxy of routes) {
    const r = await curl(['-sL', '--compressed', '-m', '30', '-w', '\n%{http_code}', ...(proxy ? ['-x', proxy] : []), ...(form ? ['--data-urlencode', `f.req=${form}`] : []), url])
    const out = r.stdout || ''
    const cut = out.lastIndexOf('\n')
    answer = { status: Number(out.slice(cut + 1)), body: out.slice(0, cut) }
    if (![0, 429, 503].includes(answer.status)) break
  }
  return answer
}

const xmlText = s => (s || '').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;|&apos;/g, "'").replace(/&amp;/g, '&')

// The article's own address behind one of Google News's links: Google's page for the link carries a signature, and its
// batchexecute answers with the address for that signature. A link Google gives neither for is returned as it is.
async function articleAddress(link, call) {
  const id = (link.match(/^https:\/\/news\.google\.com\/(?:rss\/)?(?:articles|read)\/([\w-]+)/) || [])[1]
  if (!id) return link
  const page = await call(`https://news.google.com/rss/articles/${id}`)
  if (page.status !== 200) throw Error(`Google News answered ${page.status} for the address of an article`)
  const [signature, time] = ['sg', 'ts'].map(k => (page.body.match(new RegExp(`data-n-a-${k}="([^"]+)"`)) || [])[1])
  if (!signature || !time) return link
  const answer = await call('https://news.google.com/_/DotsSplashUi/data/batchexecute', JSON.stringify([[['Fbv4je',
    `["garturlreq",[["X","X",["X","X"],null,null,1,1,"US:en",null,1,null,null,null,null,null,0,1],"X","X",1,[1,1,1],1,1,null,0,0,null,0],"${id}",${time},"${signature}"]`]]]))
  if (answer.status !== 200) throw Error(`Google News answered ${answer.status} for the address of an article`)
  try {
    return JSON.parse(JSON.parse(answer.body.split('\n').find(line => line.startsWith('[['))).find(part => part[1] === 'Fbv4je')[2])[1] || link
  } catch { return link }
}

// The Google News articles for `name` between two days, oldest first: { url, domain, date, title }; url is the
// article's own address, not Google's link to it. One search answers with a part of what it has, and a narrower one with more of it, so every
// week (Monday to Sunday) is asked for on its own; a week is held once it was asked for and its links resolved, the
// running one once it ended. `workers` weeks are read at once, each one request at a time; the first refusal stops the
// weeks not yet started, and the weeks read are kept. The lock is per name: runs asking for other names go on.
export async function gnews(name, from, to, { data = DATA, state = STATE, now = new Date(), gap = GNEWS_GAP,
  call = gnewsRequest, workers = GNEWS_WEEKS } = {}) {
  const [start, end, yesterday] = dayRange(from, to, GDELT_START, now)
  mkdirSync(state, { recursive: true })
  return withLockAsync(join(state, `gnews-${nameFolder(nameKey(name))}.lock`), async () => {
    const a = archive('gnews', name, data)
    const mondays = new Set()
    const ask = async (...request) => { if (gap) await pacedAsync('gnews', gap, state); return call(...request) }
    const addresses = new Map()
    const address = link => addresses.get(link) || addresses.set(link, articleAddress(link, ask)).get(link)
    for (const [x, y] of missingDays(a.covered, start, end)) {
      for (let d = addDays(x, -((new Date(`${x}T00:00:00Z`).getUTCDay() + 6) % 7)); d <= y; d = addDays(d, 7)) mondays.add(d)
    }
    const readWeek = async monday => {
      const query = `${String(name).trim()} after:${monday} before:${addDays(monday, 7)}`
      const r = await ask(`https://news.google.com/rss/search?q=${encodeURIComponent(query)}&hl=en-US&gl=US&ceid=US:en`)
      if (r.status !== 200) throw Error(`Google News answered ${r.status} for the week of ${monday}; the weeks read are kept`)
      const found = []
      for (const [, item] of r.body.matchAll(/<item>([\s\S]*?)<\/item>/g)) {
        const field = tag => xmlText((item.match(new RegExp(`<${tag}[^>]*>([\\s\\S]*?)</${tag}>`)) || [])[1])
        const published = new Date(field('pubDate'))
        const link = field('link')
        const url = link && await address(link)
        if (url) found.push({ url, domain: ((item.match(/<source url="([^"]*)"/) || [])[1] || '').replace(/^https?:\/\/(www\.)?/, '').replace(/\/.*$/, ''), date: isNaN(published) ? monday : day(published), title: field('title') })
      }
      const sunday = addDays(monday, 6)
      a.add(found, monday, sunday > yesterday ? yesterday : sunday)
    }
    const queue = [...mondays].sort()
    let refusal
    const work = async () => {
      while (queue.length && !refusal) {
        try { await readWeek(queue.shift()) } catch (error) { refusal ??= error }
      }
    }
    await Promise.all(Array.from({ length: workers }, work))
    if (refusal) throw refusal
    return a.between(start, end)
  })
}

// yt-dlp through the YouTube limits: one of YT_SLOTS, paced, logged in; refused, the same request through an exit.
function yt(args) {
  const youtube = args.some(a => a.includes('youtube.com') || a.includes('youtu.be') || a.startsWith('ytsearch'))
  return slot('yt', YT_SLOTS, () => {
    if (youtube) paced('youtube', YT_GAP)
    let r = run('yt-dlp', [...(youtube ? ['--cookies-from-browser', 'chrome'] : []), ...args])
    let status = r.status === 0 ? 'ok' : 'FAILED'
    if (youtube && (r.stderr || '').includes('Sign in to confirm') && proxies().length) {
      // the logged-in session is being refused: the same request without login, through an exit
      r = run('yt-dlp', ['--proxy', proxies()[Math.floor(Math.random() * EXITS)], ...args])
      status = r.status === 0 ? 'ok-proxy' : 'BLOCKED'
    }
    return { ...r, gateStatus: status }
  })
}

// The videos of a YouTube search that name the subject: { id, url, channel, title }. YouTube fills a search up with
// videos that have nothing to do with it, so a video is kept only when its title, its channel or the piece of its
// description the search shows holds one of `names`.
export function ytsearch(query, names, { count = YT_RESULTS, call = yt } = {}) {
  if (!query || !names.length) throw Error('give the search and at least one name the videos must hold: gate.mjs ytsearch "<query>" "<name>"...')
  const r = call(['--flat-playlist', '--print', '%(.{id,channel,title,description})j', `ytsearch${count}:${query}`])
  if (r.status !== 0) throw Error((r.stderr || 'yt-dlp failed').trim().split('\n').at(-1).slice(0, 300))
  const wanted = names.map(name => name.toLowerCase())
  return (r.stdout || '').split('\n').filter(Boolean).map(line => JSON.parse(line))
    .filter(v => wanted.some(name => `${v.title}\n${v.channel}\n${v.description || ''}`.toLowerCase().includes(name)))
    .map(v => ({ id: v.id, url: `https://www.youtube.com/watch?v=${v.id}`, channel: v.channel, title: v.title }))
}

// What the pages of the videos `ids` show, read by one yt-dlp run without the login, through `route` (an exit, or
// null for direct): { id, upload_date, timestamp, view_count, duration, title } for each video it could read.
function ytPages(route, ids) {
  return new Promise(resolve => {
    const child = spawn('yt-dlp', [...(route ? ['--proxy', route] : []), '--skip-download', '--no-warnings', '--ignore-errors',
      '--print', '%(.{id,upload_date,timestamp,view_count,duration,title})j', ...ids.map(id => `https://www.youtube.com/watch?v=${id}`)], { stdio: ['ignore', 'pipe', 'ignore'] })
    let out = ''
    child.stdout.on('data', chunk => { out += chunk })
    child.on('error', () => resolve([]))
    child.on('close', () => resolve(out.split('\n').filter(Boolean).flatMap(line => { try { return [JSON.parse(line)] } catch { return [] } })))
  })
}

// The exact upload date and the plays of every video in `ids`, from the videos' own pages: { videos (in the order
// asked), missing (the ids no route could read) }. The pages are read without the login, so the shared session is not
// spent on them: the ids are shared out over the routes (the exits; direct only when there is no proxy, since that
// is the address the login is used from), `workers` yt-dlp runs at once on each. YouTube asks some exits to sign in: a route that reads nothing of a batch is not used again and its videos go
// to the others. A video one route could not read among others it could is asked for once more, then it is missing.
export async function ytvideos(ids, { routes = proxies().length ? proxies() : [null], batch = YT_BATCH, workers = YT_ROUTE_RUNS, call = ytPages } = {}) {
  const todo = [...ids]
  const found = new Map()
  const failed = new Map()
  const refused = new Set()
  let reading = 0
  const work = async route => {
    while (!refused.has(route)) {
      if (!todo.length) {
        if (!reading) return
        await new Promise(resolve => setTimeout(resolve, 100)) // a batch another run is reading may come back
        continue
      }
      const asked = todo.splice(0, batch)
      reading++
      const videos = await call(route, asked)
      reading--
      for (const video of videos) found.set(video.id, video)
      const unread = asked.filter(id => !found.has(id))
      if (!videos.length) {
        refused.add(route)
        todo.push(...unread)
      } else {
        for (const id of unread) {
          failed.set(id, (failed.get(id) || 0) + 1)
          if (failed.get(id) < 2) todo.push(id)
        }
      }
    }
  }
  await Promise.all(routes.flatMap(route => Array.from({ length: workers }, () => work(route))))
  return { videos: ids.filter(id => found.has(id)).map(id => found.get(id)), missing: ids.filter(id => !found.has(id)) }
}

// Every upload of a YouTube channel, oldest first: { id, kind (videos, shorts or streams), date (YYYY-MM-DD, exact),
// timestamp, views, duration, title, url }, and the ids whose pages could not be read. The channel's tabs list the
// ids (their dates are approximate); the exact dates come from the videos' own pages.
export async function ytuploads(channel, { list = yt, read = ytvideos } = {}) {
  const kinds = new Map()
  for (const tab of YT_TABS) {
    const r = list(['--flat-playlist', '--print', '%(id)s', `${channel.replace(/\/+$/, '')}/${tab}`])
    for (const id of (r.stdout || '').split('\n').filter(Boolean)) if (!kinds.has(id)) kinds.set(id, tab) // a channel without the tab lists nothing
  }
  if (!kinds.size) throw Error(`no uploads listed for ${channel}: give the channel's address, https://www.youtube.com/@<channel>`)
  const { videos, missing } = await read([...kinds.keys()])
  const uploads = videos.map(v => ({ id: v.id, kind: kinds.get(v.id), date: v.upload_date ? v.upload_date.replace(/^(\d{4})(\d{2})(\d{2})$/, '$1-$2-$3') : null,
    timestamp: v.timestamp ?? null, views: v.view_count ?? null, duration: v.duration ?? null, title: v.title, url: `https://www.youtube.com/watch?v=${v.id}` }))
  return { uploads: uploads.sort((a, b) => (a.timestamp || 0) - (b.timestamp || 0)), missing }
}

export function stats(state = STATE) {
  const counts = {}
  let lines = []
  try { lines = readFileSync(join(state, 'log.tsv'), 'utf8').split('\n') } catch {}
  for (const line of lines) {
    const parts = line.split('\t')
    if (parts.length < 4) continue
    const key = `${parts[1]}:${parts[2]}`
    counts[key] = (counts[key] || 0) + 1
  }
  return Object.fromEntries(Object.entries(counts).sort())
}

const USAGE = readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').filter(l => l.startsWith('// Usage:') || l.startsWith('//        ')).map(l => l.slice(3)).join('\n')

function passthrough(command, args, options) {
  const r = spawnSync(command, args, { stdio: 'inherit', ...options })
  return r.status ?? 1
}

function main(argv) {
  const [command, ...args] = argv
  if (!command || command === '-h' || command === '--help') { console.error(USAGE); process.exit(2) }
  if (command === 'read') {
    process.stdout.write(read(args[0]))
  } else if (command === 'search') {
    const [engine, out] = search(args[0], args[1] ? Number(args[1]) : 8)
    log('search', engine, args[0])
    console.log(out)
    if (engine === 'FAILED') process.exit(1)
  } else if (command === 'wayback') {
    const code = slot('wayback', 1, () => passthrough('node', [WAYBACK, ...args]))
    log('wayback', code === 0 ? 'ok' : 'FAILED', args.slice(0, 2).join(' '))
    process.exit(code)
  } else if (command === 'chrome') {
    if (args[0] === 'google') paced('google', 10) // Google answers a busy browser with a CAPTCHA page; waits outside the slots
    const code = slot('chrome', CHROME_SLOTS, () => passthrough('opencli', args))
    log('chrome', code === 0 ? 'ok' : 'FAILED', args.slice(0, 3).join(' '))
    process.exit(code)
  } else if (command === 'yt') {
    const r = yt(args)
    process.stdout.write(r.stdout || '')
    process.stderr.write(r.stderr || '')
    log('yt', r.gateStatus, args.at(-1) || '')
    process.exit(r.status ?? 1)
  } else if (command === 'fetch-x-posts') {
    const script = process.env.FETCH_X_POSTS
    if (!script) { console.error('gate.mjs fetch-x-posts: FETCH_X_POSTS (the fetch-x-posts.mjs script) is not set in the .env file'); process.exit(2) }
    const code = passthrough('node', [script, ...args])
    log('fetch-x-posts', code === 0 ? 'ok' : 'FAILED', args[0] || '')
    process.exit(code)
  } else if (['gdelt', 'gnews', 'ytsearch'].includes(command)) {
    const isDay = arg => /^\d{4}-\d{2}(-\d{2})?$/.test(arg)
    const names = args.filter(arg => !isDay(arg))
    const [from, to] = args.filter(isDay)
    ;(async () => command === 'gdelt' ? gdelt(names, from, to)
      : command === 'gnews' ? gnews(names[0], from, to)
        : ytsearch(names[0], names.slice(1)))().then(found => {
      for (const line of found) console.log(JSON.stringify(line))
      log(command, 'ok', args.join(' '))
    }, error => {
      log(command, 'FAILED', args.join(' '))
      console.error(`${command}: ${error.message}`)
      process.exit(1)
    })
  } else if (command === 'ytuploads') {
    ytuploads(args[0] || '').then(({ uploads, missing }) => {
      for (const upload of uploads) console.log(JSON.stringify(upload))
      if (!uploads.length) { console.error('ytuploads: YouTube refused every route (it asks to sign in): no video page was read'); log('ytuploads', 'BLOCKED', args[0]); process.exit(1) }
      if (missing.length) console.error(`ytuploads: ${missing.length} of ${uploads.length + missing.length} video pages could not be read: ${missing.join(' ')}`)
      log('ytuploads', missing.length ? 'PARTIAL' : 'ok', args[0])
    }, error => {
      log('ytuploads', 'FAILED', args[0] || '')
      console.error(`ytuploads: ${error.message}`)
      process.exit(1)
    })
  } else if (command === 'stats') {
    console.log(JSON.stringify(stats()))
  } else {
    console.error(USAGE)
    process.exit(2)
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) main(process.argv.slice(2))
