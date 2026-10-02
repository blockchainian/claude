#!/usr/bin/env node
// ABOUTME: Fetches archived pages in one parallel batch at a fixed rate per route (a proxy or the direct
// ABOUTME: connection), and builds a dated follower curve from every monthly capture of a profile page.
//
// Usage: wayback.mjs fetch <out dir> <url>... [--from <file with one url per line>]
//        wayback.mjs curve <out dir> <profile url>...     (every address the profile has had)
// Routes come from the WAYBACK_PROXIES environment variable (comma-separated proxy URLs, one per exit IP).
// Without it there is one route: the direct connection, through the HTTPS_PROXY / HTTP_PROXY of the environment
// when one is set (NO_PROXY is honoured). A refused route is dropped; when every route is refused the run stops
// with an error. fetch prints one JSON line per url (url, status, file, route). curve prints one JSON line per
// capture (date, value, text, url, file): value is the count when it could be read, text is the page's own wording
// when it is rounded or in another language, and both are null when the page shows no count.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import https from 'node:https'
import { join } from 'node:path'
import tls from 'node:tls'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { gunzipSync } from 'node:zlib'

export const PER_MINUTE = 30 // requests per route; the archive refuses an IP well above this
export const WORKERS_PER_ROUTE = 3 // a page takes seconds to arrive, so one worker alone cannot reach the rate
const ARCHIVE = 'https://web.archive.org'
const TIMEOUT = 60_000
const REDIRECTS = 10
const EXACT = [ // patterns whose first group is the full count, digits with any locale's separators
  /subscriber-count[^>]*title="([\d][\d,.\s ]*)/,
  /"followers_count":(\d+)/,
  /title="([\d][\d,.\s ]*) Followers"/,
  /([\d][\d,]{4,}) subscribers/,
]
// The channel's own header on the script-rendered page; other channels listed on the page have counts too.
const HEADER = /"c4TabbedHeaderRenderer"[\s\S]*?"subscriberCountText":\{(?:"simpleText":"|"runs":\[\{"text":")([^"]+)"/
const FULL = /^\d{1,3}(?:[.,\s ]\d{3})+/ // a whole count with thousands separators
const ROUNDED = /^([\d.]+)([KMB]) subscribers$/

export class AllRoutesRefused extends Error {
  // Every route was refused before the batch finished.
  constructor(remaining, total) {
    super(`every route was refused by the archive; ${remaining.length} of ${total} urls not fetched`)
    this.remaining = remaining
  }
}

export class Refused extends Error {
  // The archive (or the proxy) turned this route away.
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

function bypassed(hostname) {
  // True when the environment's NO_PROXY names this host (a name, a parent domain, or `*`).
  const list = (process.env.NO_PROXY || process.env.no_proxy || '').split(',').map(s => s.trim()).filter(Boolean)
  return list.some(entry => entry === '*' || hostname === entry.replace(/^\./, '') || hostname.endsWith('.' + entry.replace(/^\./, '')))
}

function proxyFor(route, target) {
  // The proxy URL a request goes through: the route itself, or the environment's for the direct route.
  if (route) return new URL(route)
  if (bypassed(target.hostname)) return null
  const env = target.protocol === 'https:'
    ? process.env.HTTPS_PROXY || process.env.https_proxy
    : process.env.HTTP_PROXY || process.env.http_proxy
  return env ? new URL(env) : null
}

const proxyAuth = proxy => (proxy.username ? { 'Proxy-Authorization': 'Basic ' + Buffer.from(`${decodeURIComponent(proxy.username)}:${decodeURIComponent(proxy.password)}`).toString('base64') } : {})

function tunnel(proxy, target) {
  // A TLS socket to the target through the proxy's CONNECT method.
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
      resolve(secure)
    })
    req.on('timeout', () => req.destroy(new Error('proxy connect timed out')))
    req.on('error', reject)
    req.end()
  })
}

function once(url, route) {
  // {status, headers, body} of one GET of one url through one route, without following redirects.
  const target = new URL(url)
  const proxy = proxyFor(route, target)
  const headers = { 'User-Agent': 'Mozilla/5.0' }
  const collect = (req, resolve, reject) => {
    req.on('response', res => {
      const chunks = []
      res.on('data', chunk => chunks.push(chunk))
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }))
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
  return tunnel(proxy, target).then(socket => new Promise((resolve, reject) => collect(
    https.request(target, { method: 'GET', headers, timeout: TIMEOUT, agent: false, createConnection: () => socket }), resolve, reject)))
}

export async function httpGet(route, url) {
  // {status, body} for one url through one route, redirects followed. Throws Refused when the archive itself
  // answers 429.
  let response
  for (let hop = 0; ; hop++) {
    response = await once(url, route)
    const location = response.headers.location
    if (![301, 302, 303, 307, 308].includes(response.status) || !location || hop >= REDIRECTS) break
    url = new URL(location, url).href
  }
  // A capture of a page that answered 429 at the time carries the archive's replay headers; a bare 429 is
  // the archive limiting the caller.
  const replayed = Object.keys(response.headers).some(name => name.toLowerCase().startsWith('x-archive-orig-'))
  if (response.status === 429 && !replayed) throw new Refused(url)
  return { status: response.status, body: response.body }
}

export async function fetchAll(urls, routes, out, { perMinute = PER_MINUTE, get = httpGet } = {}) {
  // Fetch every url, save each body under out, and return one result per url in input order.
  mkdirSync(out, { recursive: true })
  const todo = urls.map((url, position) => ({ position, url }))
  const results = new Map()
  const refused = new Set()
  const nextStart = routes.map(() => 0)
  const now = () => performance.now()
  const work = async index => {
    while (!refused.has(index) && results.size < urls.length) {
      const item = todo.shift()
      if (!item) {
        await sleep(200)
        continue
      }
      // space this route's requests evenly
      const start = Math.max(now(), nextStart[index])
      nextStart[index] = start + 60_000 / perMinute
      await sleep(Math.max(0, start - now()))
      let status
      let body
      try {
        ;({ status, body } = await get(routes[index], item.url))
      } catch { // refused connection, proxy failure, timeout
        refused.add(index)
        todo.push(item)
        return
      }
      const name = `${String(item.position).padStart(3, '0')}-` + item.url.replace(/[^A-Za-z0-9]+/g, '-').slice(-120).replace(/^-+|-+$/g, '') + '.html'
      const path = join(out, name)
      body = Buffer.from(body)
      if (body[0] === 0x1f && body[1] === 0x8b) body = gunzipSync(body) // the archive replays some captures as the compressed bytes it stored
      writeFileSync(path, body)
      results.set(item.position, { url: item.url, status, file: path, route: index })
    }
  }
  await Promise.all(routes.flatMap((_, index) => Array.from({ length: WORKERS_PER_ROUTE }, () => work(index))))
  if (results.size < urls.length) throw new AllRoutesRefused(urls.filter((_, position) => !results.has(position)), urls.length)
  return urls.map((_, position) => results.get(position))
}

export function extract(page) {
  // The follower or subscriber count a capture shows: {value: int or null, text: the page's wording or null}.
  for (const pattern of EXACT) {
    const match = pattern.exec(page)
    if (match) return { value: Number(match[1].replace(/\D/g, '')), text: match[1].trim() }
  }
  const match = HEADER.exec(page)
  if (!match) return { value: null, text: null }
  const text = match[1]
  const full = FULL.exec(text)
  if (full) return { value: Number(full[0].replace(/\D/g, '')), text }
  const rounded = ROUNDED.exec(text)
  const value = rounded ? Math.round(Number(rounded[1]) * { K: 1e3, M: 1e6, B: 1e9 }[rounded[2]]) : null
  return { value, text }
}

export async function curve(addresses, routes, out, { perMinute = PER_MINUTE, archive = ARCHIVE } = {}) {
  // One row per monthly capture of every address, oldest first, with the count each capture shows.
  const listings = addresses.map(address => `${archive}/cdx/search/cdx?url=${address}&output=json&fl=timestamp,statuscode&filter=statuscode:200&collapse=timestamp:6`)
  const captures = []
  const listed = await fetchAll(listings, routes, join(out, 'lists'), { perMinute })
  addresses.forEach((address, n) => {
    const rows = JSON.parse(readFileSync(listed[n].file, 'utf8') || '[]')
    captures.push(...rows.slice(1).map(([stamp]) => `${archive}/web/${stamp}id_/${address}`))
  })
  const rows = []
  for (const page of await fetchAll(captures, routes, out, { perMinute })) {
    const stamp = /\/web\/(\d{8})/.exec(page.url)[1]
    const found = page.status === 200 ? extract(readFileSync(page.file, 'utf8')) : { value: null, text: null }
    rows.push({ date: `${stamp.slice(0, 4)}-${stamp.slice(4, 6)}-${stamp.slice(6)}`, ...found, url: page.url, file: page.file })
  }
  return rows.sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0))
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
  const routes = (process.env.WAYBACK_PROXIES || '').split(',').map(p => p.trim()).filter(Boolean)
  if (!routes.length) routes.push(null)
  try {
    let results
    if (cmd === 'curve') {
      results = await curve(items, routes, out)
    } else {
      const urls = [...items, ...(parsed.values.from ? readFileSync(parsed.values.from, 'utf8').split(/\r?\n/).map(l => l.trim()).filter(Boolean) : [])]
      results = await fetchAll(urls, routes, out)
    }
    for (const result of results) console.log(JSON.stringify(result))
  } catch (error) {
    if (!(error instanceof AllRoutesRefused)) throw error
    console.error(`error: ${error.message}`)
    console.error(error.remaining.join('\n'))
    process.exit(1)
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2))
