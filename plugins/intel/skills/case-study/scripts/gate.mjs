#!/usr/bin/env node
// ABOUTME: Machine-wide gate for the fetch commands of case-study runs: every agent of every workflow calls the
// ABOUTME: shared services (Jina, Exa, Wayback, Chrome, yt-dlp, GDELT, X) through it, so limits are kept across runs.
//
// Usage: gate.mjs read <url>              page text: Jina reader over the proxy exits, then Exa fetch, then Chrome
//        gate.mjs search "<query>" [n]    Exa web search, then DuckDuckGo and Yahoo when Exa is out of credits
//        gate.mjs wayback <args...>       this skill's wayback.mjs, one run at a time
//        gate.mjs chrome <args...>        opencli <args...>, at most CHROME_SLOTS at once; Google searches paced
//        gate.mjs yt <args...>            yt-dlp <args...> (Chrome cookies added for YouTube), at most YT_SLOTS at once
//        gate.mjs fetch-x-posts <args...> the fetch-x-posts script named by FETCH_X_POSTS: X search on an account pool, one JSON post per line
//        gate.mjs gdelt "<url>"           GDELT, one request every 6 seconds machine-wide
//        gate.mjs stats                   calls and failures per command since the log began
// State (pace files, slot locks, the log) lives in ~/.cache/case-study-limits, shared with fetch-x-posts. Settings come
// from the .env file env.mjs finds: ISP_PROXY_URL (one URL; the ten ports after its own are the exits; without it every
// request goes direct) and FETCH_X_POSTS (the fetch-x-posts script).
import { spawnSync } from 'node:child_process'
import { appendFileSync, closeSync, existsSync, mkdirSync, openSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadEnv } from './env.mjs'

loadEnv()

export const STATE = process.env.CASE_STUDY_LIMITS || join(homedir(), '.cache', 'case-study-limits')
const HERE = dirname(fileURLToPath(import.meta.url))
const WAYBACK = join(HERE, 'wayback.mjs')
const JINA_GAP = 3.5 // seconds between requests on one exit: Jina allows 20 a minute per IP without a key
const CHROME_SLOTS = 6
const YT_SLOTS = 3
const YT_GAP = 2 // seconds between YouTube requests machine-wide: the logged-in session is shared by every run
const EXITS = 10

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
    const youtube = args.some(a => a.includes('youtube.com') || a.includes('youtu.be') || a.startsWith('ytsearch'))
    const r = slot('yt', YT_SLOTS, () => {
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
  } else if (command === 'gdelt') {
    for (let attempt = 0; attempt < 4; attempt++) {
      paced('gdelt', 6)
      const r = run('curl', ['-s', '-m', '40', '-w', '\n%{http_code}', args[0]])
      const cut = r.stdout.lastIndexOf('\n')
      const code = r.stdout.slice(cut + 1).trim()
      if (code === '200') {
        log('gdelt', 'ok', args[0])
        console.log(r.stdout.slice(0, cut))
        return
      }
      if (code !== '429') { console.error(`gdelt: HTTP ${code}`); process.exit(1) }
    }
    log('gdelt', 'FAILED', args[0])
    console.error('gdelt: still 429 after 4 paced tries')
    process.exit(1)
  } else if (command === 'stats') {
    console.log(JSON.stringify(stats()))
  } else {
    console.error(USAGE)
    process.exit(2)
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) main(process.argv.slice(2))
