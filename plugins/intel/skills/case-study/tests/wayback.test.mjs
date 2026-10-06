// ABOUTME: Tests the batch archive fetcher (saving, the rate, redirects, the refused error), reading a count out
// ABOUTME: of a capture, and the dated curve. A local HTTP server plays the archive.
import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs'
import { spawn, spawnSync } from 'node:child_process'
import http from 'node:http'
import https from 'node:https'
import net from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, test } from 'node:test'
import { gzipSync } from 'node:zlib'

import * as wb from '../scripts/wayback.mjs'

const OLD = '<span class="yt-subscription-button-subscriber-count-branded-horizontal" title="10,490,968 subscribers">10,490,968</span>'
const LOCALIZED = '<span class="yt-subscription-button-subscriber-count-branded-horizontal yt-uix-tooltip" title="37 704 014" aria-label="37 704 014 подписчиков">37</span>'
const MODERN = '"gridChannelRenderer":{"subscriberCountText":{"simpleText":"97K"}},"header":{"c4TabbedHeaderRenderer":{"title":"X",' +
  '"subscriberCountText":{"runs":[{"text":"105M subscribers"}]}}}'
const MODERN_LOCALIZED = '"c4TabbedHeaderRenderer":{"title":"X","subscriberCountText":{"simpleText":"106 Mln di iscritti"}}'
const CAPTURES = { 2014: OLD, 2015: LOCALIZED, 2020: MODERN }

// /page/<n> is a capture; /replayed429 is a capture of a 429; /throttled is the archive refusing the caller;
// /web/...channel-flaky is refused once, then given; /moved sends the caller on to /page/0 the way the archive sends an id_ url to its nearest capture.
const refusedOnce = new Set() // the captures of channel-flaky are refused the first time each is asked for
const server = http.createServer((req, res) => {
  if (req.url.startsWith('/cdx/search/cdx')) {
    const rows = [['timestamp', 'statuscode'], ['20140115000000', '200'], ['20150715000000', '200'], ['20200915000000', '200']]
    if (req.url.includes('channel-throttled')) { res.writeHead(429); res.end('Too Many Requests'); return }
    if (req.url.includes('channel-offline') && !refusedOnce.has(req.url)) { refusedOnce.add(req.url); res.writeHead(503); res.end('<html>Temporarily Offline</html>'); return }
    if (req.url.includes('channel-gateway') && !refusedOnce.has(req.url)) { refusedOnce.add(req.url); res.writeHead(504); res.end('<html>Gateway Time-out</html>'); return }
    if (req.url.includes('channel-broken')) { res.writeHead(503); res.end('<html>Service Unavailable</html>'); return }
    res.writeHead(200)
    res.end(JSON.stringify(/channel-(a|flaky|offline|gateway)/.test(req.url) ? rows : []))
    return
  }
  if (req.url.startsWith('/web/')) {
    if (req.url.includes('channel-flaky') && !refusedOnce.has(req.url)) { refusedOnce.add(req.url); res.writeHead(429); res.end('Too Many Requests'); return }
    res.writeHead(200, { 'x-archive-orig-date': 'then' })
    res.end(CAPTURES[req.url.slice(5, 9)])
    return
  }
  if (req.url === '/moved') {
    res.writeHead(302, { location: '/page/0' })
    res.end()
    return
  }
  const code = { '/replayed429': 429, '/throttled': 429 }[req.url] || 200
  res.writeHead(code, req.url === '/throttled' ? {} : { 'x-archive-orig-date': 'then' })
  res.end(`body of ${req.url}`)
})
let base
let tmp
before(async () => {
  tmp = mkdtempSync(join(tmpdir(), 'wayback-'))
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  base = `http://127.0.0.1:${server.address().port}`
})
after(() => {
  server.close()
  rmSync(tmp, { recursive: true, force: true })
})

test('extract reads the count a capture shows', () => {
  assert.equal(wb.extract(OLD).value, 10490968, 'an exact count from the old channel page')
  assert.equal(wb.extract(LOCALIZED).value, 37704014, 'a count with another locale\'s separators')
  assert.deepEqual(wb.extract(MODERN), { value: 105000000, text: '105M subscribers' }, 'the channel\'s own rounded count, not a listed channel\'s')
  assert.deepEqual(wb.extract(MODERN_LOCALIZED), { value: null, text: '106 Mln di iscritti' }, 'a count in words of another language is kept as text')
  assert.equal(wb.extract('"c4TabbedHeaderRenderer":{"subscriberCountText":{"simpleText":"57.216.326 iscritti"}}').value, 57216326, 'a full count in the header, whatever the language')
  assert.equal(wb.extract('{"followers_count":342701,"friends_count":10}').value, 342701, 'a twitter capture gives the follower count')
  assert.deepEqual(wb.extract('<html>subscribe</html>'), { value: null, text: null }, 'a page with no count gives nothing')
  // Formats in the IShowSpeed captures of 2021-2026, each the channel's own count
  assert.deepEqual(wb.extract('"subscriberCountText":{"accessibility":{"accessibilityData":{"label":"61.1K subscribers"}},"simpleText":"61.1K subscribers"},"x":1,"c4TabbedHeaderRenderer":{"channelId":"UC","subscriberCountText":{"accessibility":{"accessibilityData":{"label":"1.6 million subscribers"}},"simpleText":"1.6M subscribers"},"tvBanner":{}}'),
    { value: 1600000, text: '1.6M subscribers' }, 'the header\'s count when its accessibility label comes before the text')
  assert.deepEqual(wb.extract('"metadataParts":[{"text":{"content":"9K subscribers"}}],"pageHeaderRenderer":{"pageTitle":"IShowSpeed","metadataRows":[{"metadataParts":[{"text":{"content":"@IShowSpeed"}}]},{"metadataParts":[{"text":{"content":"47M subscribers"}},{"text":{"content":"1.8K videos"}}]}]}'),
    { value: 47000000, text: '47M subscribers' }, 'the page header of 2024 on, not a count before it')
  assert.deepEqual(wb.extract('<div class="py-1"><p class="m-0 text-[0.75em] font-medium capitalize pr-[50px]">subscribers</p><p class="text-[1.25em] font-extralight pr-[50px]">36.8M</p></div>'),
    { value: 36800000, text: '36.8M' }, 'Social Blade\'s layout of 2025 on')
  // TikTok, X and Social Blade's TikTok pages, as in the khaby-lame captures
  assert.equal(wb.extract('"roomId":""},"stats":{"followerCount":64500000,"followingCount":47}').value, 64500000, 'a TikTok profile\'s own stats')
  assert.equal(wb.extract('"authorStats":{"followerCount":125800000,"heart":1}').value, 125800000, 'a TikTok profile\'s author stats')
  assert.equal(wb.extract('"stats":{"khaby.lame":{"followerCount":161900000}}').value, 161900000, 'TikTok stats keyed by the user')
  assert.equal(wb.extract('{"@type":"InteractionCounter","name": "Follows", "userInteractionCount": 342701}').value, 342701, 'an X page\'s JSON-LD before 2022')
  assert.equal(wb.extract('<span class="YouTubeUserTopLight">Followers</span><br>\n\t\t\t<span style="font-weight: bold;">78,200,000</span>').value, 78200000, 'Social Blade\'s layout before 2025')
  assert.deepEqual(wb.extract('<p class="m-0 capitalize pr-[50px]">followers</p><p class="text-[1.25em] pr-[50px]">162.1M</p>'),
    { value: 162100000, text: '162.1M' }, 'Social Blade\'s TikTok layout of 2025 on')
})

test('fetchAll saves every page in input order at its rate; a replayed 429 is a result', async () => {
  const urls = [0, 1, 2].map(n => `${base}/page/${n}`).concat(`${base}/replayed429`)
  const start = performance.now()
  const results = await wb.fetchAll(urls, null, join(tmp, 'a'), { perMinute: 240 })
  const took = (performance.now() - start) / 1000
  assert.deepEqual(results.map(r => r.url), urls)
  assert.ok(results.slice(0, 3).every(r => readFileSync(r.file, 'utf8') === `body of ${r.url.slice(base.length)}`), 'a page is saved to the output folder')
  assert.ok(results[3].status === 429 && results[3].file, 'a capture of a 429 is a result, not a refusal')
  assert.ok(took >= 0.75, `the batch keeps to its rate: ${took.toFixed(2)}s for 4 requests at 240 a minute`)
})

test('a redirect is followed, as the archive sends an id_ url to its nearest capture', async () => {
  const [result] = await wb.fetchAll([`${base}/moved`], null, join(tmp, 'r'), { perMinute: 6000 })
  assert.equal(result.status, 200)
  assert.equal(readFileSync(result.file, 'utf8'), 'body of /page/0')
})

test('the proxy given is the one every request goes through', async () => {
  const seen = []
  const get = (proxy, url) => {
    seen.push(proxy)
    return wb.httpGet(null, url)
  }
  const results = await wb.fetchAll([0, 1, 2].map(n => `${base}/page/${n}`), 'http://isp.example:8080', join(tmp, 'b'), { perMinute: 6000, get })
  assert.ok(results.length === 3 && results.every(r => r.status === 200), JSON.stringify(results))
  assert.deepEqual(seen, Array(3).fill('http://isp.example:8080'))
})

test('a request whose connection fails is asked again; one that keeps failing stops the batch', async () => {
  const failures = new Map()
  const flaky = async (proxy, url) => {
    failures.set(url, (failures.get(url) || 0) + 1)
    if (failures.get(url) <= 2) throw new Error('connect ECONNREFUSED')
    return { status: 200, body: Buffer.from(url) }
  }
  const urls = [0, 1, 2].map(n => `${base}/page/${n}`)
  const results = await wb.fetchAll(urls, null, join(tmp, 'e'), { perMinute: 6000, get: flaky, retryWait: 1 })
  assert.deepEqual(results.map(r => r.status), [200, 200, 200])
  assert.deepEqual([...failures.values()], [3, 3, 3], 'each was asked again until it answered')
  let asked = 0
  const dead = async () => { asked++; throw new Error('connect ECONNREFUSED') }
  await assert.rejects(wb.fetchAll(urls.slice(0, 1), null, join(tmp, 'f'), { perMinute: 6000, get: dead, retryWait: 1 }), error => error instanceof wb.Refused)
  assert.equal(asked, 1 + wb.RETRIES)
})

test('a capture the archive replays compressed is saved as text', async () => {
  const get = async () => ({ status: 200, body: gzipSync(Buffer.from('1,234 subscribers')) })
  const results = await wb.fetchAll([`${base}/page/0`], ['only'], join(tmp, 'z'), { perMinute: 6000, get })
  assert.equal(readFileSync(results[0].file, 'utf8'), '1,234 subscribers')
})

test('a 429 the archive itself keeps answering is a refusal that stops the batch, and the error says what is left', async () => {
  const urls = [0, 1, 2].map(n => `${base}/page/${n}`).concat(`${base}/throttled`)
  await assert.rejects(wb.fetchAll(urls, null, join(tmp, 'c'), { perMinute: 6000, retryWait: 1 }), error => {
    assert.ok(error instanceof wb.Refused)
    assert.ok(error.remaining.includes(`${base}/throttled`), JSON.stringify(error.remaining))
    assert.ok(error.message.includes('1 of 4') && error.message.includes('the archive answered 429'), error.message)
    return true
  })
})

test('curve has one dated row per capture of every address, each naming its capture and its saved file', async () => {
  const { rows, missing } = await wb.curve(['example.com/channel-a', 'example.com/channel-b'], null, join(tmp, 'd'), { perMinute: 6000, archive: base })
  assert.deepEqual(missing, [])
  assert.deepEqual(rows.map(r => [r.date, r.value]), [['2014-01-15', 10490968], ['2015-07-15', 37704014], ['2020-09-15', 105000000]])
  assert.equal(rows[0].url, `${base}/web/20140115000000id_/example.com/channel-a`)
  assert.ok(statSync(rows[0].file).isFile())
})

test('the capture lists go through the ISP proxy at its own rate, the captures through the residential proxy at the residential rate', () => {
  assert.deepEqual(wb.routes('http://home.example:1', 'http://isp.example:2'),
    { proxy: 'http://home.example:1', perMinute: wb.RESIDENTIAL_PER_MINUTE, listProxy: 'http://isp.example:2', listPerMinute: wb.LIST_PER_MINUTE })
  assert.equal(wb.LIST_PER_MINUTE, 100)
})

test('without the residential proxy there is no route for the captures', () => {
  assert.throws(() => wb.routes('', 'http://isp.example:2'), /INTEL_RESIDENTIAL_PROXY_URL/)
  assert.throws(() => wb.routes('', null), /INTEL_RESIDENTIAL_PROXY_URL/)
})

test('a list the archive refuses or cannot give does not cost the rows of the others: they are kept, and what is missing is named', async () => {
  const { rows, missing } = await wb.curve(['example.com/channel-a', 'example.com/channel-throttled', 'example.com/channel-broken'], null, join(tmp, 'p'),
    { perMinute: 6000, archive: base, retryWait: 1 })
  assert.deepEqual(rows.map(r => r.date), ['2014-01-15', '2015-07-15', '2020-09-15'])
  assert.deepEqual(missing.map(m => m.replace(base, '')).sort(), [
    '/cdx/search/cdx?url=example.com/channel-broken&output=json&fl=timestamp,statuscode&filter=statuscode:200&collapse=timestamp:6',
    '/cdx/search/cdx?url=example.com/channel-throttled&output=json&fl=timestamp,statuscode&filter=statuscode:200&collapse=timestamp:6',
  ])
})

test('a 429 from the archive pauses every request of the batch, not only the one refused', async () => {
  const starts = []
  let throttledAt = null
  const get = async (proxy, url) => {
    starts.push(performance.now())
    if (throttledAt === null) { throttledAt = performance.now(); throw new wb.Throttled(url) }
    return { status: 200, body: Buffer.from(url) }
  }
  const urls = Array.from({ length: 6 }, (_, n) => `${base}/page/${n}`)
  const results = await wb.fetchAll(urls, null, join(tmp, 'g'), { perMinute: 6000, get, retryWait: 300 })
  assert.equal(results.length, 6)
  const during = starts.filter(t => t > throttledAt && t < throttledAt + 280)
  assert.equal(during.length, 0, `requests started during the pause: ${during.map(t => Math.round(t - throttledAt)).join(', ')} ms after the 429`)
})

test('through a proxy that gives every request a new address, a 429 is asked again at once and pauses nothing', async () => {
  const starts = []
  let throttledAt = null
  const get = async (proxy, url) => {
    starts.push(performance.now())
    if (throttledAt === null) { throttledAt = performance.now(); throw new wb.Throttled(url) }
    return { status: 200, body: Buffer.from(url) }
  }
  const urls = Array.from({ length: 6 }, (_, n) => `${base}/page/${n}`)
  const results = await wb.fetchAll(urls, null, join(tmp, 'r'), { perMinute: 6000, get, retryWait: 5000, rotating: true })
  assert.deepEqual(results.map(r => r.url), urls, 'the refused one is read on its second asking')
  assert.ok(starts.at(-1) - throttledAt < 1000, `the last request started ${Math.round(starts.at(-1) - throttledAt)} ms after the 429`)
})

test('curve asks again at once for a capture the archive refused, without pausing the others', async () => {
  const started = performance.now()
  const { rows, missing } = await wb.curve(['example.com/channel-flaky'], null, join(tmp, 'rc'), { perMinute: 6000, archive: base, retryWait: 5000 })
  assert.deepEqual(missing, [])
  assert.equal(rows.length, 3)
  assert.ok(performance.now() - started < 3000, `took ${Math.round(performance.now() - started)} ms`)
})

test('a list the archive answers 503 or 504 is asked for again, not taken for an empty one', async () => {
  const { rows, missing } = await wb.curve(['example.com/channel-offline', 'example.com/channel-gateway'], null, join(tmp, 'off'), { perMinute: 6000, archive: base, retryWait: 1 })
  assert.deepEqual(missing, [])
  assert.equal(rows.length, 6)
})

test('the partial results of a refused batch come with the refusal', async () => {
  const get = async (proxy, url) => { if (url.endsWith('/1')) throw new Error('connect ECONNREFUSED'); return { status: 200, body: Buffer.from(url) } }
  await assert.rejects(wb.fetchAll([0, 1].map(n => `${base}/page/${n}`), null, join(tmp, 'h'), { perMinute: 6000, get, retryWait: 1 }), error => {
    assert.ok(error instanceof wb.Refused)
    assert.deepEqual(error.results.map(r => r && r.url), [`${base}/page/0`, undefined])
    return true
  })
})

test('an https request through the proxy goes through its tunnel, not straight to the host, and names the host as it is', async () => {
  // A local TLS server stands for the archive under a name only the proxy can resolve: a request that skipped the
  // tunnel would not find it. The archive builds its redirects from the Host it is sent, so a wrong port there breaks them. The check runs in a child, which trusts the server's certificate from its start.
  const dir = mkdtempSync(join(tmpdir(), 'tunnel-'))
  const made = spawnSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=archive.invalid',
    '-addext', 'subjectAltName=DNS:archive.invalid', '-keyout', join(dir, 'key.pem'), '-out', join(dir, 'cert.pem')], { stdio: 'ignore' })
  assert.equal(made.status, 0, 'openssl could not make a certificate')
  const target = https.createServer({ key: readFileSync(join(dir, 'key.pem')), cert: readFileSync(join(dir, 'cert.pem')) }, (req, res) => res.end(`seen ${req.headers.host} ${req.url}`))
  await new Promise(resolve => target.listen(0, '127.0.0.1', resolve))
  const port = target.address().port
  const tunnels = []
  const proxy = http.createServer()
  proxy.on('connect', (req, client) => {
    tunnels.push(req.url)
    const upstream = net.connect(port, '127.0.0.1', () => {
      client.write('HTTP/1.1 200 Connection Established\r\n\r\n')
      upstream.pipe(client)
      client.pipe(upstream)
    })
    upstream.on('error', () => client.destroy())
    client.on('error', () => upstream.destroy())
  })
  await new Promise(resolve => proxy.listen(0, '127.0.0.1', resolve))
  const script = `import { httpGet } from ${JSON.stringify(new URL('../scripts/wayback.mjs', import.meta.url).href)}
const r = await httpGet('http://127.0.0.1:${proxy.address().port}', 'https://archive.invalid/web/x')
console.log(r.status, Buffer.from(r.body).toString())`
  const child = await new Promise(resolve => {
    const p = spawn(process.execPath, ['--input-type=module', '-e', script], { env: { ...process.env, NODE_EXTRA_CA_CERTS: join(dir, 'cert.pem') } })
    let out = ''
    let err = ''
    p.stdout.on('data', d => { out += d })
    p.stderr.on('data', d => { err += d })
    p.on('close', code => resolve({ code, out, err }))
  })
  proxy.close()
  target.close()
  rmSync(dir, { recursive: true, force: true })
  assert.equal(child.out.trim(), '200 seen archive.invalid /web/x', child.err)
  assert.deepEqual(tunnels, ['archive.invalid:443'])
})
