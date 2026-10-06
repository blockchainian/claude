#!/usr/bin/env node
// ABOUTME: Fetches archived pages in one parallel batch at a fixed rate, and builds a dated follower curve
// ABOUTME: from every monthly capture of a profile page.
//
// Usage: wayback.mjs fetch <out dir> <url>... [--from <file with one url per line>]
//        wayback.mjs curve <out dir> <profile url>...     (every address the profile has had)
// Captures go through the proxy in INTEL_RESIDENTIAL_PROXY_URL (from the .env file env.mjs finds), one URL that gives every
// connection another household address, which the archive counts apart; without it the run stops with an error. The
// capture lists go through INTEL_ISP_PROXY_URL, else direct. A request that fails, or that
// the archive answers with 429, is asked for again; when it keeps failing the run stops with an error. fetch prints one JSON line per url (url, status, file). curve prints one JSON line per capture (date,
// value, text, url, file): value is the count when it could be read, text is the page's own wording when it is
// rounded or in another language, and both are null when the page shows no count.
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import https from 'node:https'
import { join } from 'node:path'
import tls from 'node:tls'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { gunzipSync } from 'node:zlib'
import { loadEnv } from './env.mjs'

loadEnv()

// The capture lists, through the ISP proxy, for the whole batch.
export const LIST_PER_MINUTE = 100
// Through the residential proxy. Measured 2026-10 with exits worldwide: 0 of 40 captures answered 429 at 10 at once;
// the proxy itself refuses some connections at any rate tried (18 of 118 at 90 a minute), and those are asked again.
export const RESIDENTIAL_PER_MINUTE = 100
export const RETRIES = 8 // times a request that failed is asked again, each after a longer wait
const PER_WORKER = 10 // requests a minute one worker carries: a page takes seconds to arrive
const ARCHIVE = 'https://web.archive.org'
const TIMEOUT = 60_000
const REDIRECTS = 10
const EXACT = [ // patterns whose first group is the full count, digits with any locale's separators
  /subscriber-count[^>]*title="([\d][\d,.\s ]*)/,
  /"followers_count":(\d+)/,
  /"(?:stats|authorStats)":\{(?:"[^"]+":\{)?"followerCount":(\d+)/, // a TikTok profile's own stats, first on the page
  /"name":\s*"Follows",\s*"userInteractionCount":\s*(\d+)/, // an X profile's JSON-LD, before 2022
  /YouTubeUserTopLight">(?:Followers|Subscribers)<\/span><br>\s*<span[^>]*>([\d,]+)</, // Social Blade before 2025
  /title="([\d][\d,.\s ]*) Followers"/,
  /([\d][\d,]{4,}) subscribers/,
]
// The channel's own count, rounded or in words, where the page shows it; other channels listed on the page have counts
// too. In order: the script-rendered channel header (its accessibility label comes first from 2021), the page header
// YouTube uses from 2024, and Social Blade's statistics page from 2025 (YouTube and TikTok).
const HEADERS = [
  /"c4TabbedHeaderRenderer"[\s\S]*?"subscriberCountText":\{(?:"accessibility":\{"accessibilityData":\{"label":"[^"]*"\}\},)?(?:"simpleText":"|"runs":\[\{"text":")([^"]+)"/,
  /"pageHeaderRenderer"[\s\S]*?"content":"([^"]*\bsubscribers)"/,
  />(?:subscribers|followers)<\/p><p[^>]*>([\d.,]+[KMB]?)<\/p>/,
]
const FULL = /^\d{1,3}(?:[.,\s ]\d{3})+/ // a whole count with thousands separators
const ROUNDED = /^([\d.]+)([KMB])(?: subscribers)?$/

export class Refused extends Error {
  // The archive (or the proxy) turned the caller away before the batch finished; results holds, in input order, the
  // result of every url that was fetched (undefined for the others).
  constructor(remaining, total, cause, results = []) {
    super(`the archive refused the connection; ${remaining.length} of ${total} urls not fetched${cause ? ` (${cause})` : ''}`)
    this.remaining = remaining
    this.results = results
  }
}

export class Throttled extends Error {
  // The archive itself answered 429.
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

function proxyFor(proxy) {
  return proxy ? new URL(proxy) : null
}

const proxyAuth = proxy => (proxy.username ? { 'Proxy-Authorization': 'Basic ' + Buffer.from(`${decodeURIComponent(proxy.username)}:${decodeURIComponent(proxy.password)}`).toString('base64') } : {})

function tunnel(proxy, target) {
  // A TLS socket to the target through the proxy's CONNECT method, and the socket to the proxy under it.
  return new Promise((resolve, reject) => {
    const port = target.port || 443
    const req = http.request({ host: proxy.hostname, port: proxy.port || 80, method: 'CONNECT', path: `${target.hostname}:${port}`,
      headers: { Host: `${target.hostname}:${port}`, ...proxyAuth(proxy) }, timeout: TIMEOUT })
    req.on('connect', (res, socket) => {
      if (res.statusCode !== 200) {
        socket.destroy()
        reject(new Error(`proxy answered ${res.statusCode} to CONNECT`))
        return
      }
      const secure = tls.connect({ socket, servername: target.hostname })
      // A reset once the response is in (the proxy closing its side) is noise, not a failure of the request:
      // the request itself still hears the errors of its own socket.
      socket.on('error', () => {})
      secure.on('error', () => {})
      resolve({ secure, raw: socket })
    })
    req.on('timeout', () => req.destroy(new Error('proxy connect timed out')))
    req.on('error', reject)
    req.end()
  })
}

function once(url, via) {
  // {status, headers, body} of one GET of one url, through the proxy `via` when given, without following redirects.
  const target = new URL(url)
  const proxy = proxyFor(via, target)
  const headers = { 'User-Agent': 'Mozilla/5.0' }
  const collect = (req, resolve, reject) => {
    req.on('response', res => {
      const chunks = []
      res.on('data', chunk => chunks.push(chunk))
      res.on('end', () => {
        req.destroy() // closes the tunnel, if any
        resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) })
      })
      res.on('error', reject)
    })
    req.on('timeout', () => req.destroy(new Error(`timed out after ${TIMEOUT / 1000}s`)))
    req.on('error', reject)
    req.end()
  }
  if (!proxy) {
    const lib = target.protocol === 'https:' ? https : http
    return new Promise((resolve, reject) => collect(lib.request(target, { method: 'GET', headers, timeout: TIMEOUT, agent: false }), resolve, reject))
  }
  if (target.protocol !== 'https:') { // a plain request to the proxy, with the whole url as its path
    return new Promise((resolve, reject) => collect(http.request({ host: proxy.hostname, port: proxy.port || 80, method: 'GET', path: url,
      headers: { ...headers, Host: target.host, ...proxyAuth(proxy) }, timeout: TIMEOUT, agent: false }), resolve, reject))
  }
  // Both ends of the tunnel are closed with the answer: a proxy that keeps its side open would keep the process from ending.
  // No `agent: false` here: Node then makes an agent of its own, which ignores createConnection and goes straight to the host.
  return tunnel(proxy, target).then(({ secure, raw }) => new Promise((resolve, reject) => collect(
    https.request(target, { method: 'GET', headers: { ...headers, Host: target.host }, timeout: TIMEOUT, createConnection: () => secure }), resolve, reject))
    .finally(() => { secure.destroy(); raw.destroy() }))
}

export async function httpGet(proxy, url) {
  // {status, body} for one url, through the proxy when given, redirects followed. Throws when the archive
  // itself answers 429, 503 or 504.
  let response
  for (let hop = 0; ; hop++) {
    response = await once(url, proxy)
    const location = response.headers.location
    if (![301, 302, 303, 307, 308].includes(response.status) || !location || hop >= REDIRECTS) break
    url = new URL(location, url).href
  }
  // A capture of a page that answered 429 at the time carries the archive's replay headers; a bare 429 is
  // the archive limiting the caller.
  const replayed = Object.keys(response.headers).some(name => name.toLowerCase().startsWith('x-archive-orig-'))
  if (response.status === 429 && !replayed) throw new Throttled(url)
  // 503 (the archive "temporarily offline") and 504 pass on the next asking, measured 2026-10: 19 of 20 at once.
  if ([503, 504].includes(response.status) && !replayed) throw new Error(`the archive answered ${response.status}`)
  return { status: response.status, body: response.body }
}

export async function fetchAll(urls, proxy, out, { perMinute = LIST_PER_MINUTE, get = httpGet, retryWait = 1000, rotating = false } = {}) {
  // Fetch every url, save each body under out, and return one result per url in input order. rotating: the proxy gives
  // every request another address, so a 429, which the archive answers to one network, is asked again at once.
  mkdirSync(out, { recursive: true })
  const todo = urls.map((url, position) => ({ position, url }))
  const results = new Map()
  let refused = false
  let cause = ''
  const failures = {} // how many requests failed, by reason
  let nextStart = 0
  let pausedUntil = 0 // a 429 from the archive pauses every worker on fixed addresses: it counts the caller, not the url
  const now = () => performance.now()
  const work = async () => {
    while (!refused && results.size < urls.length) {
      const item = todo.shift()
      if (!item) {
        await sleep(200)
        continue
      }
      // space the requests evenly, after any pause
      const start = Math.max(now(), nextStart, pausedUntil)
      nextStart = start + 60_000 / perMinute
      await sleep(Math.max(0, start - now()))
      while (pausedUntil > now()) await sleep(pausedUntil - now()) // a 429 came while this one waited
      let status
      let body
      try {
        ;({ status, body } = await get(proxy, item.url))
      } catch (error) { // refused connection, proxy failure, timeout, the archive's own 429
        item.failures = (item.failures || 0) + 1
        todo.push(item)
        const reason = error instanceof Throttled ? 'the archive answered 429' : String(error.code || error.message).replace(/\d+\.\d+\.\d+\.\d+(:\d+)?/g, '').trim()
        failures[reason] = (failures[reason] || 0) + 1
        if (item.failures > RETRIES) {
          refused = true
          cause = reason
          return
        }
        if (error instanceof Throttled) {
          if (!rotating) pausedUntil = Math.max(pausedUntil, now() + retryWait * 2 ** (item.failures - 1))
        } else await sleep(retryWait * item.failures)
        continue
      }
      const name = `${String(item.position).padStart(3, '0')}-` + item.url.replace(/[^A-Za-z0-9]+/g, '-').slice(-120).replace(/^-+|-+$/g, '') + '.html'
      const path = join(out, name)
      body = Buffer.from(body)
      if (body[0] === 0x1f && body[1] === 0x8b) body = gunzipSync(body) // the archive replays some captures as the compressed bytes it stored
      writeFileSync(path, body)
      results.set(item.position, { url: item.url, status, file: path })
    }
  }
  await Promise.all(Array.from({ length: Math.ceil(perMinute / PER_WORKER) }, work))
  if (Object.keys(failures).length) console.error(`asked again: ${Object.entries(failures).map(([reason, count]) => `${count} × ${reason}`).join(', ')}`)
  if (results.size < urls.length) {
    throw new Refused(urls.filter((_, position) => !results.has(position)), urls.length, cause, urls.map((_, position) => results.get(position)))
  }
  return urls.map((_, position) => results.get(position))
}

export function extract(page) {
  // The follower or subscriber count a capture shows: {value: int or null, text: the page's wording or null}.
  for (const pattern of EXACT) {
    const match = pattern.exec(page)
    if (match) return { value: Number(match[1].replace(/\D/g, '')), text: match[1].trim() }
  }
  const match = HEADERS.map(pattern => pattern.exec(page)).find(Boolean)
  if (!match) return { value: null, text: null }
  const text = match[1]
  const full = FULL.exec(text)
  if (full) return { value: Number(full[0].replace(/\D/g, '')), text }
  const rounded = ROUNDED.exec(text)
  const value = rounded ? Math.round(Number(rounded[1]) * { K: 1e3, M: 1e6, B: 1e9 }[rounded[2]]) : null
  return { value, text }
}

// A batch's results, the fetched ones only when the archive refused part of it, and the urls it did not give.
async function fetchWhatItGives(urls, proxy, out, options) {
  try {
    return { results: await fetchAll(urls, proxy, out, options), missing: [] }
  } catch (error) {
    if (!(error instanceof Refused)) throw error
    return { results: error.results, missing: error.remaining }
  }
}

export async function curve(addresses, proxy, out, { perMinute = LIST_PER_MINUTE, archive = ARCHIVE, listProxy = proxy, listPerMinute = perMinute, retryWait } = {}) {
  // { rows, missing }: one row per monthly capture of every address, oldest first, with the count each capture shows,
  // and the capture lists and captures the archive did not give, which cost none of the others. The capture lists are
  // asked through listProxy: the archive answers 429 to a list asked from a residential address.
  const listings = addresses.map(address => `${archive}/cdx/search/cdx?url=${address}&output=json&fl=timestamp,statuscode&filter=statuscode:200&collapse=timestamp:6`)
  const captures = []
  const lists = await fetchWhatItGives(listings, listProxy, join(out, 'lists'), { perMinute: listPerMinute, retryWait })
  const missing = [...lists.missing]
  addresses.forEach((address, n) => {
    const list = lists.results[n]
    if (!list) return
    let rows
    try { rows = list.status === 200 ? JSON.parse(readFileSync(list.file, 'utf8') || '[]') : null } catch { rows = null }
    if (!rows) { missing.push(list.url); return } // an error page instead of a list
    captures.push(...rows.slice(1).map(([stamp]) => `${archive}/web/${stamp}id_/${address}`))
  })
  const pages = await fetchWhatItGives(captures, proxy, out, { perMinute, retryWait, rotating: true })
  missing.push(...pages.missing)
  const rows = []
  for (const page of pages.results.filter(Boolean)) {
    const stamp = /\/web\/(\d{8})/.exec(page.url)[1]
    const found = page.status === 200 ? extract(readFileSync(page.file, 'utf8')) : { value: null, text: null }
    rows.push({ date: `${stamp.slice(0, 4)}-${stamp.slice(4, 6)}-${stamp.slice(6)}`, ...found, url: page.url, file: page.file })
  }
  return { rows: rows.sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0)), missing }
}

// The proxy and rate for the captures, and for the capture lists, which go through the ISP proxy at its own rate: the
// lists asked at the residential rate through the ISP exits were answered 429 until the run stopped (2026-10). The
// captures go only through the residential proxy: from one address the archive answers 429 to nearly every one.
export function routes(residential, isp) {
  if (!residential) throw new Error('INTEL_RESIDENTIAL_PROXY_URL is not set: the archive captures are read only through the residential proxy')
  return { proxy: residential, perMinute: RESIDENTIAL_PER_MINUTE, listProxy: isp, listPerMinute: LIST_PER_MINUTE }
}

const USAGE = 'usage: wayback.mjs {fetch,curve} <out dir> ...'

async function main(argv) {
  const [cmd, ...rest] = argv
  if (!['fetch', 'curve'].includes(cmd)) {
    console.error(USAGE)
    process.exit(2)
  }
  let parsed
  try {
    parsed = parseArgs({ args: rest, options: cmd === 'fetch' ? { from: { type: 'string' } } : {}, allowPositionals: true, strict: true })
  } catch (error) {
    console.error(`${USAGE}\nwayback.mjs ${cmd}: ${error.message}`)
    process.exit(2)
  }
  const [out, ...items] = parsed.positionals
  if (!out || (cmd === 'curve' && !items.length)) {
    console.error(USAGE)
    process.exit(2)
  }
  const residential = (process.env.INTEL_RESIDENTIAL_PROXY_URL || '').trim()
  const isp = (process.env.INTEL_ISP_PROXY_URL || '').trim() || null
  let route
  try {
    route = routes(residential, isp)
  } catch (error) {
    console.error(`error: ${error.message}`)
    process.exit(2)
  }
  const { proxy, perMinute, listProxy, listPerMinute } = route
  try {
    let results
    if (cmd === 'curve') {
      const { rows, missing } = await curve(items, proxy, out, { perMinute, listProxy, listPerMinute })
      for (const row of rows) console.log(JSON.stringify(row))
      if (missing.length) {
        console.error(`error: the archive did not give ${missing.length} of the lists and captures; the rows above are what it gave`)
        console.error(missing.join('\n'))
        process.exit(1)
      }
      return
    } else {
      const urls = [...items, ...(parsed.values.from ? readFileSync(parsed.values.from, 'utf8').split(/\r?\n/).map(l => l.trim()).filter(Boolean) : [])]
      results = await fetchAll(urls, proxy, out, { perMinute, rotating: true })
    }
    for (const result of results) console.log(JSON.stringify(result))
  } catch (error) {
    if (!(error instanceof Refused)) throw error
    console.error(`error: ${error.message}`)
    console.error(error.remaining.join('\n'))
    process.exit(1)
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) main(process.argv.slice(2))
