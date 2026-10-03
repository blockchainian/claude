#!/usr/bin/env node
// ABOUTME: Fetches archived pages in one parallel batch at a fixed rate, and builds a dated follower curve
// ABOUTME: from every monthly capture of a profile page.
//
// Usage: wayback.mjs fetch <out dir> <url>... [--from <file with one url per line>]
//        wayback.mjs curve <out dir> <profile url>...     (every address the profile has had)
// Requests go through the proxy in ISP_PROXY_URL (from the .env file env.mjs finds), one URL; the proxy rotates its exit
// addresses itself. Without it they go direct, through the HTTPS_PROXY / HTTP_PROXY of the environment when one
// is set (NO_PROXY is honoured). When the archive or the proxy refuses the connection the run stops with an
// error. fetch prints one JSON line per url (url, status, file). curve prints one JSON line per capture (date,
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

// For the whole batch, not for each exit: the archive counts the proxy's exits together. Measured 2026-10: 60 a
// minute spread evenly over the ten exits had 19 of 120 requests answered 429, and one exit alone was refused after 20
// in a minute.
export const PER_MINUTE = 30
export const WORKERS = 3 // a page takes seconds to arrive, so one worker alone cannot reach the rate
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

export class Refused extends Error {
  // The archive (or the proxy) turned the caller away before the batch finished.
  constructor(remaining, total) {
    super(`the archive refused the connection; ${remaining.length} of ${total} urls not fetched`)
    this.remaining = remaining
  }
}

class Throttled extends Error {
  // The archive itself answered 429.
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

function bypassed(hostname) {
  // True when the environment's NO_PROXY names this host (a name, a parent domain, or `*`).
  const list = (process.env.NO_PROXY || process.env.no_proxy || '').split(',').map(s => s.trim()).filter(Boolean)
  return list.some(entry => entry === '*' || hostname === entry.replace(/^\./, '') || hostname.endsWith('.' + entry.replace(/^\./, '')))
}

function proxyFor(proxy, target) {
  // The proxy URL a request goes through: the one given, or the environment's for a direct request.
  if (proxy) return new URL(proxy)
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

function once(url, via) {
  // {status, headers, body} of one GET of one url, through the proxy `via` when given, without following redirects.
  const target = new URL(url)
  const proxy = proxyFor(via, target)
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

export async function httpGet(proxy, url) {
  // {status, body} for one url, through the proxy when given, redirects followed. Throws when the archive
  // itself answers 429.
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
  return { status: response.status, body: response.body }
}

export async function fetchAll(urls, proxy, out, { perMinute = PER_MINUTE, get = httpGet } = {}) {
  // Fetch every url, save each body under out, and return one result per url in input order.
  mkdirSync(out, { recursive: true })
  const todo = urls.map((url, position) => ({ position, url }))
  const results = new Map()
  let refused = false
  let nextStart = 0
  const now = () => performance.now()
  const work = async () => {
    while (!refused && results.size < urls.length) {
      const item = todo.shift()
      if (!item) {
        await sleep(200)
        continue
      }
      // space the requests evenly
      const start = Math.max(now(), nextStart)
      nextStart = start + 60_000 / perMinute
      await sleep(Math.max(0, start - now()))
      let status
      let body
      try {
        ;({ status, body } = await get(proxy, item.url))
      } catch { // refused connection, proxy failure, timeout, the archive's own 429
        refused = true
        todo.push(item)
        return
      }
      const name = `${String(item.position).padStart(3, '0')}-` + item.url.replace(/[^A-Za-z0-9]+/g, '-').slice(-120).replace(/^-+|-+$/g, '') + '.html'
      const path = join(out, name)
      body = Buffer.from(body)
      if (body[0] === 0x1f && body[1] === 0x8b) body = gunzipSync(body) // the archive replays some captures as the compressed bytes it stored
      writeFileSync(path, body)
      results.set(item.position, { url: item.url, status, file: path })
    }
  }
  await Promise.all(Array.from({ length: WORKERS }, work))
  if (results.size < urls.length) throw new Refused(urls.filter((_, position) => !results.has(position)), urls.length)
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

export async function curve(addresses, proxy, out, { perMinute = PER_MINUTE, archive = ARCHIVE } = {}) {
  // One row per monthly capture of every address, oldest first, with the count each capture shows.
  const listings = addresses.map(address => `${archive}/cdx/search/cdx?url=${address}&output=json&fl=timestamp,statuscode&filter=statuscode:200&collapse=timestamp:6`)
  const captures = []
  const listed = await fetchAll(listings, proxy, join(out, 'lists'), { perMinute })
  addresses.forEach((address, n) => {
    const rows = JSON.parse(readFileSync(listed[n].file, 'utf8') || '[]')
    captures.push(...rows.slice(1).map(([stamp]) => `${archive}/web/${stamp}id_/${address}`))
  })
  const rows = []
  for (const page of await fetchAll(captures, proxy, out, { perMinute })) {
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
  const proxy = (process.env.ISP_PROXY_URL || '').trim() || null
  try {
    let results
    if (cmd === 'curve') {
      results = await curve(items, proxy, out)
    } else {
      const urls = [...items, ...(parsed.values.from ? readFileSync(parsed.values.from, 'utf8').split(/\r?\n/).map(l => l.trim()).filter(Boolean) : [])]
      results = await fetchAll(urls, proxy, out)
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
