// ABOUTME: Tests the machine-wide gate's limits: proxy exits, pacing, slots, the Exa credit window, the kept GDELT and Google News articles, the YouTube filter and the log stats.
// ABOUTME: Everything runs against a temporary state directory; no service is called.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, test } from 'node:test'
import { fileURLToPath } from 'node:url'

const root = mkdtempSync(join(tmpdir(), 'gate-'))
const state = join(root, 'limits')
mkdirSync(state, {recursive: true})
process.env.INTEL_STATE_DIR = root
process.env.ISP_PROXY_URL = '' // the machine's .env must not reach the tests
const gate = await import('../scripts/gate.mjs')
after(() => rmSync(root, { recursive: true, force: true }))

test('the proxy exits are the ten ports after the URL\'s own; no URL means no proxy', () => {
  assert.deepEqual(gate.proxies('http://u:p@isp.example:8001').slice(0, 3), ['http://u:p@isp.example:8002', 'http://u:p@isp.example:8003', 'http://u:p@isp.example:8004'])
  assert.equal(gate.proxies('http://u:p@isp.example:8001').length, 10)
  assert.deepEqual(gate.proxies(), [], 'no proxy set')
  assert.deepEqual(gate.readCommand('https://a.example/p', null).slice(-1), ['https://r.jina.ai/https://a.example/p'])
  assert.ok(gate.readCommand('https://a.example/p', 'http://x:1').includes('-x'))
})

test('paced waits out the gap since the last call with the same key, across processes', () => {
  const t0 = Date.now()
  gate.paced('t', 0.3, state)
  gate.paced('t', 0.3, state)
  assert.ok(Date.now() - t0 >= 280, 'the second call waited')
  const t1 = Date.now()
  gate.paced('other', 0.3, state)
  assert.ok(Date.now() - t1 < 200, 'another key does not wait')
})

test('a slot is held while its function runs, a dead owner\'s slot is taken over, and tryOnce gives up when all are taken', () => {
  let inner
  const result = gate.slot('s', 1, () => {
    assert.ok(existsSync(join(state, 's-0.slot')))
    inner = gate.slot('s', 1, () => 'second', { state, tryOnce: true })
    return 'first'
  }, { state })
  assert.equal(result, 'first')
  assert.equal(inner, undefined, 'the only slot was held')
  assert.ok(!existsSync(join(state, 's-0.slot')), 'released after the function')
  writeFileSync(join(state, 's-0.slot'), '999999999')
  assert.equal(gate.slot('s', 1, () => 'taken over', { state, tryOnce: true }), 'taken over')
})

test('freeRoute hands out an exit whose gap has passed and never the same one within the gap', () => {
  const a = gate.freeRoute(2, 0.5, state)
  const b = gate.freeRoute(2, 0.5, state)
  assert.notEqual(a, b)
})

test('exa retries on 429, stops asking for ten minutes after a 402, and otherwise returns the answer', () => {
  const answers = [{ stdout: 'error (429)', stderr: '' }, { stdout: 'result', stderr: '' }]
  assert.equal(gate.exa('web_search_exa', ['query=x'], { state, call: () => answers.shift() }), 'result')
  let calls = 0
  assert.equal(gate.exa('web_search_exa', ['query=x'], { state, call: () => { calls++; return { stdout: '', stderr: 'error (402)' } } }), '')
  assert.equal(gate.exa('web_search_exa', ['query=x'], { state, call: () => { calls++; return { stdout: 'late', stderr: '' } } }), '', 'not asked again inside the window')
  assert.equal(calls, 1)
})

test('an Exa answer that is an error is no result: a 5xx or a timeout is asked again, any other error gives nothing', () => {
  const fresh = mkdtempSync(join(tmpdir(), 'exa-'))
  let calls = 0
  const unauthorized = () => { calls++; return { status: 1, stdout: 'MCP error: error (401) Unauthorized: invalid API key', stderr: '' } }
  assert.equal(gate.exa('web_search_exa', ['query=x'], { state: fresh, call: unauthorized }), '', 'a refused key is not a result')
  assert.equal(calls, 1, 'and is not asked again')
  const flaky = [{ status: 1, stdout: 'error (503) Service Unavailable', stderr: '' }, { status: 1, stdout: '', stderr: 'Error: request timed out' }, { status: 0, stdout: 'result', stderr: '' }]
  assert.equal(gate.exa('web_search_exa', ['query=x'], { state: fresh, call: () => flaky.shift() }), 'result')
  assert.equal(gate.exa('web_search_exa', ['query=x'], { state: fresh, tries: 2, call: () => ({ status: 1, stdout: 'error (500)', stderr: '' }) }), '', 'one that keeps failing gives nothing')
  rmSync(fresh, { recursive: true, force: true })
})

// A bq stand-in: answers every call with the rows of the year it was asked for.
function bigquery(rowsByYear, calls) {
  return args => {
    calls.push(args)
    const year = args.find(a => a.startsWith('--parameter=from:')).slice(-19, -15)
    const names = JSON.parse(args.find(a => a.startsWith('--parameter=names:')).split(':').slice(2).join(':'))
    return { status: 0, stderr: '', stdout: JSON.stringify((rowsByYear[year] || []).filter(r => names.includes(r.name))) }
  }
}
const parameter = (args, name) => args.find(a => a.startsWith(`--parameter=${name}:`)).split(':').slice(2).join(':')

test('missingDays and coverDays keep the days already held as merged ranges', () => {
  assert.deepEqual(gate.missingDays([], '2023-01-01', '2023-01-31'), [['2023-01-01', '2023-01-31']])
  assert.deepEqual(gate.missingDays([['2023-01-10', '2023-01-20']], '2023-01-01', '2023-01-31'), [['2023-01-01', '2023-01-09'], ['2023-01-21', '2023-01-31']])
  assert.deepEqual(gate.missingDays([['2022-01-01', '2023-12-31']], '2023-06-01', '2023-06-30'), [])
  assert.deepEqual(gate.coverDays([['2023-01-01', '2023-01-09']], '2023-01-10', '2023-01-20'), [['2023-01-01', '2023-01-20']], 'adjacent ranges join')
  assert.deepEqual(gate.coverDays([['2023-03-01', '2023-03-09']], '2023-01-01', '2023-01-20'), [['2023-01-01', '2023-01-20'], ['2023-03-01', '2023-03-09']])
})

test('gdelt reads from BigQuery only the days a name lacks, a year at a time, and keeps each name\'s articles in its own folder', () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  const calls = []
  const call = bigquery({
    2022: [{ name: 'kobeissi letter', url: 'https://b.example/early', seen: '20220105000000', domain: 'b.example', mentions: '1' }],
    2023: [{ name: 'kobeissi letter', url: 'https://a.example/late', seen: '20231129194500', domain: 'a.example', mentions: '3' },
      { name: 'kobeissi letter', url: 'https://a.example/late', seen: '20231130000000', domain: 'a.example', mentions: '3' },
      { name: 'adam kobeissi', url: 'https://c.example/adam', seen: '20230301000000', domain: 'c.example', mentions: '2' }],
    2024: [{ name: 'kobeissi letter', url: 'https://d.example/next', seen: '20240210000000', domain: 'd.example', mentions: '1' }],
  }, calls)
  const options = { data, state, call, project: 'p', now: new Date('2026-10-03T12:00:00Z') }
  assert.deepEqual(gate.gdelt(['Kobeissi Letter', 'Adam  KOBEISSI'], '2022-01-01', '2023-12-31', options), [
    { name: 'kobeissi letter', url: 'https://b.example/early', domain: 'b.example', date: '2022-01-05', mentions: 1 },
    { name: 'kobeissi letter', url: 'https://a.example/late', domain: 'a.example', date: '2023-11-29', mentions: 3 },
    { name: 'adam kobeissi', url: 'https://c.example/adam', domain: 'c.example', date: '2023-03-01', mentions: 2 },
  ], 'one article per url, the earliest day it was seen')
  assert.equal(calls.length, 2, 'one query per year')
  assert.ok(calls[0].includes('--project_id=p') && calls[0].some(a => a === '--maximum_bytes_billed=100000000000'))
  assert.equal(parameter(calls[0], 'names'), '["kobeissi letter","adam kobeissi"]', 'every name in one query, lowercased')
  assert.deepEqual([parameter(calls[0], 'from'), parameter(calls[0], 'to')], ['2022-01-01 00:00:00', '2023-01-01 00:00:00'])
  assert.match(calls[0].at(-1), /gkg_partitioned[\s\S]*UNNEST\(@names\)[\s\S]*AllNames[\s\S]*_PARTITIONTIME >= @from/)
  const folder = join(data, 'gdelt', 'kobeissi-letter')
  assert.deepEqual(JSON.parse(readFileSync(join(folder, 'articles.out.json'), 'utf8')), { name: 'kobeissi letter', covered: [['2022-01-01', '2023-12-31']], count: 2 })
  assert.deepEqual(readFileSync(join(folder, 'articles.jsonl'), 'utf8').trim().split('\n').map(l => JSON.parse(l).url), ['https://b.example/early', 'https://a.example/late'], 'oldest first')

  assert.deepEqual(gate.gdelt(['kobeissi letter'], '2023-06-01', '2023-12-31', options).map(a => a.url), ['https://a.example/late'])
  assert.equal(calls.length, 2, 'days already held are not read again')
  assert.deepEqual(gate.gdelt(['Kobeissi Letter', 'Someone Else'], '2023-06-01', '2024-03-31', options).map(a => a.url), ['https://a.example/late', 'https://d.example/next'])
  assert.equal(calls.length, 4)
  assert.equal(parameter(calls[2], 'names'), '["someone else"]', 'a held year is read for the new name only')
  assert.deepEqual([parameter(calls[2], 'from'), parameter(calls[2], 'to')], ['2023-06-01 00:00:00', '2024-01-01 00:00:00'])
  assert.equal(parameter(calls[3], 'names'), '["kobeissi letter","someone else"]')
  assert.deepEqual([parameter(calls[3], 'from'), parameter(calls[3], 'to')], ['2024-01-01 00:00:00', '2024-04-01 00:00:00'], 'only the days asked for')
  assert.deepEqual(JSON.parse(readFileSync(join(folder, 'articles.out.json'), 'utf8')).covered, [['2022-01-01', '2024-03-31']])
  rmSync(data, { recursive: true, force: true })
})

test('gdelt reads the running day again every time, keeps the years done before a failure, and refuses what it cannot run', () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  const calls = []
  const options = { data, state, call: bigquery({}, calls), project: 'p', now: new Date('2026-10-03T12:00:00Z') }
  gate.gdelt(['someone new'], '2026-09-01', undefined, options)
  assert.deepEqual([parameter(calls[0], 'from'), parameter(calls[0], 'to')], ['2026-09-01 00:00:00', '2026-10-04 00:00:00'], 'no end date means today')
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gdelt', 'someone-new', 'articles.out.json'), 'utf8')).covered, [['2026-09-01', '2026-10-02']], 'a day counts once it has ended')
  gate.gdelt(['someone new'], '2026-09-01', '2026-10-03', options)
  assert.deepEqual([parameter(calls[1], 'from'), parameter(calls[1], 'to')], ['2026-10-03 00:00:00', '2026-10-04 00:00:00'])
  gate.gdelt(['someone new'], '2026-09-01', '2026-10-02', options)
  assert.equal(calls.length, 2)

  let asked = 0
  // bq ends its message with the help page's address, so the quota is recognised anywhere in the output
  const heldRow = JSON.stringify([{ name: 'nobody', url: 'https://h.example/held', seen: '20240105000000', domain: 'h.example', mentions: '1' }])
  const failing = { ...options, call: args => asked++ ? { status: 1, stdout: 'BigQuery error in query operation: Quota exceeded: Your project exceeded quota for free query bytes scanned. For more information, see\nhttps://cloud.google.com/bigquery/docs/troubleshoot-quotas', stderr: '' } : { status: 0, stdout: heldRow, stderr: '' } }
  assert.throws(() => gate.gdelt(['nobody'], '2024-01-01', '2025-12-31', failing), error => error instanceof gate.QuotaUsedUp && /free BigQuery quota/.test(error.message)
    && error.held.map(a => a.url).join() === 'https://h.example/held')
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gdelt', 'nobody', 'articles.out.json'), 'utf8')).covered, [['2024-01-01', '2024-12-31']], 'the year read before the failure is kept')
  assert.throws(() => gate.gdelt(['nobody'], '2025-01-01', '2025-12-31', { ...options, project: '' }), /BIGQUERY_PROJECT_ID/)
  assert.throws(() => gate.gdelt(['https://api.gdeltproject.org/api/v2/doc/doc?query=x'], undefined, undefined, options), /name/)
  assert.throws(() => gate.gdelt([], '2025-01-01', '2025-12-31', options), /name/)
  assert.throws(() => gate.gdelt(Array.from({ length: 101 }, (_, i) => `name ${i}`), '2025-01-01', '2025-12-31', options), /100/)
  assert.throws(() => gate.gdelt(['nobody'], '2025', '2025-12-31', options), /YYYY-MM-DD/)
  rmSync(data, { recursive: true, force: true })
})

test('gnews asks Google News for each month it lacks, keeps the articles under their own addresses, and stops at the first refusal', async () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  const item = (title, link, date, source) => `<item><title>${title}</title><link>${link}</link><pubDate>${date}</pubDate><source url="https://www.${source}">X</source></item>`
  const months = {
    '2025-06-01': item('Stocks &amp; bonds - A', 'https://news.google.com/rss/articles/one', 'Tue, 03 Jun 2025 07:00:00 GMT', 'a.example')
      + item('Second - B', 'https://news.google.com/rss/articles/two', 'Wed, 11 Jun 2025 07:00:00 GMT', 'b.example') + item('Again - A', 'https://news.google.com/rss/articles/one', 'Tue, 03 Jun 2025 07:00:00 GMT', 'a.example'),
  }
  const asked = []
  const searched = []
  const resolved = []
  // Google's page for a link carries a signature, and its batchexecute answers with the article's address for it
  const call = (url, form) => {
    const link = url.match(/rss\/articles\/(\w+)$/)
    if (link) return { status: 200, body: link[1] === 'bare' ? '<c-wiz></c-wiz>' : `<c-wiz><div jscontroller="x" data-n-a-sg="sig-${link[1]}" data-n-a-ts="17"></div></c-wiz>` }
    if (url.endsWith('/batchexecute')) {
      // the answers come back in any order, each tagged with its request's tag
      const answers = JSON.parse(form)[0].map(([, request, , tag]) => {
        const [, id, ts, sg] = request.match(/"(\w+)",(\d+),"sig-(\w+)"\]$/)
        assert.deepEqual([ts, sg], ['17', id])
        resolved.push(id)
        return ['wrb.fr', 'Fbv4je', JSON.stringify(['garturlres', `https://${id}.example/story?a=1&b=2`, 1]), null, null, null, tag]
      })
      return { status: 200, body: `)]}'\n\n${JSON.stringify([...answers.reverse(), ['di', 11]])}\n` }
    }
    const month = decodeURIComponent(url).match(/after:([\d-]+) before:([\d-]+)/); searched.push(new URL(url).searchParams.get('q')); asked.push(`${month[1]} ${month[2]}`); return { status: 200, body: `<rss>${months[month[1]] || ''}</rss>` }
  }
  const options = { data, state, call, now: new Date('2026-10-03T12:00:00Z') }
  assert.deepEqual((await gate.gnews('Kobeissi Letter', '2025-06-04', '2025-06-12', options)), [
    { url: 'https://two.example/story?a=1&b=2', domain: 'b.example', date: '2025-06-11', title: 'Second - B' },
  ], 'only the days asked for')
  assert.deepEqual(asked, ['2025-06-01 2025-07-01'], 'whole calendar months, first day to first day')
  assert.deepEqual((await gate.gnews('kobeissi letter', '2025-06-02', '2025-06-15', options)).map(a => a.title), ['Stocks & bonds - A', 'Second - B'], 'one article per link, oldest first')
  assert.ok(searched.every(q => q.startsWith('"Kobeissi Letter" after:') || q.startsWith('"kobeissi letter" after:')), 'the name is searched as a phrase, not as separate words')
  assert.equal(asked.length, 1, 'months already held are not asked for again')
  assert.deepEqual(resolved, ['one', 'two'], 'a link is resolved once')
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'kobeissi-letter', 'articles.out.json'), 'utf8')), { name: 'kobeissi letter', covered: [['2025-06-01', '2025-06-30']], count: 2 })
  let n = 0
  const refusing = { ...options, call: url => { asked.push('x'); return n++ ? { status: 429, body: '' } : { status: 200, body: '<rss></rss>' } } }
  await assert.rejects(() => gate.gnews('Kobeissi Letter', '2025-06-02', '2025-08-31', refusing), error => /429/.test(error.message)
    && error.held.map(a => a.title).join() === 'Stocks & bonds - A,Second - B')
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'kobeissi-letter', 'articles.out.json'), 'utf8')).covered, [['2025-06-01', '2025-07-31']], 'the month read before the refusal is kept')
  months['2025-08-01'] = item('No signature - C', 'https://news.google.com/rss/articles/bare?oc=5', 'Tue, 05 Aug 2025 07:00:00 GMT', 'c.example')
  assert.deepEqual((await gate.gnews('Kobeissi Letter', '2025-08-01', '2025-08-31', options)).map(a => a.url), ['https://news.google.com/rss/articles/bare?oc=5'], 'a link Google gives no address for stays as it is')
  months['2025-09-01'] = item('Third - D', 'https://news.google.com/rss/articles/three', 'Tue, 02 Sep 2025 07:00:00 GMT', 'd.example')
  const unresolved = { ...options, call: (url, form) => url.includes('/rss/articles/') ? { status: 429, body: '' } : call(url, form) }
  await assert.rejects(() => gate.gnews('Kobeissi Letter', '2025-09-01', '2025-09-30', unresolved), /429/)
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'kobeissi-letter', 'articles.out.json'), 'utf8')).covered, [['2025-06-01', '2025-08-31']], 'a month whose links were not resolved is asked for again')
  rmSync(data, { recursive: true, force: true })
})

test('a Google News request goes compressed through an ISP exit, and through the residential proxy when Google refuses that exit', async () => {
  const exits = gate.proxies('http://u:p@isp.example:8001')
  const residential = 'http://r:p@resi.example:9000'
  const asked = []
  const curl = answers => args => { asked.push(args); return { status: 0, stdout: `body\n${answers.shift()}` } }
  assert.deepEqual((await gate.gnewsRequest('https://news.google.com/rss/search?q=x', undefined, { exits, residential, curl: curl([200]) })), { status: 200, body: 'body' })
  assert.equal(asked.length, 1)
  assert.ok(asked[0].includes('--compressed'), 'asked for a compressed answer')
  assert.ok(exits.includes(asked[0][asked[0].indexOf('-x') + 1]), 'through one of the ISP exits')
  assert.equal(asked[0].at(-1), 'https://news.google.com/rss/search?q=x')
  asked.length = 0
  for (const refusal of [503, 429, 302]) {
    assert.deepEqual((await gate.gnewsRequest('https://news.google.com/x', 'form', { exits, residential, curl: curl([refusal, 200]) })).status, 200)
    assert.equal(asked.at(-1)[asked.at(-1).indexOf('-x') + 1], residential, `a ${refusal} is asked again through the residential proxy`)
    assert.ok(asked.at(-1).includes('f.req=form'), 'with the same form')
  }
  asked.length = 0
  assert.equal((await gate.gnewsRequest('https://news.google.com/x', undefined, { exits, residential, curl: curl([404]) })).status, 404)
  assert.equal(asked.length, 1, 'an answer that is not a refusal is not asked again')
  asked.length = 0
  assert.equal((await gate.gnewsRequest('https://news.google.com/x', undefined, { exits: [], residential: '', curl: curl([503]) })).status, 503)
  assert.deepEqual([asked.length, asked[0].includes('-x')], [1, false], 'without a proxy, one direct request')
})

test('gnews asks for several months and link pages at once, and resolves many links in one request', async () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  let open = 0
  let most = 0
  let batches = 0
  const call = async (url, form) => {
    open++; most = Math.max(most, open)
    await new Promise(resolve => setTimeout(resolve, 20))
    open--
    const link = url.match(/rss\/articles\/(\w+)$/)
    if (link) return { status: 200, body: `<div data-n-a-sg="s" data-n-a-ts="1"></div>` }
    if (url.endsWith('/batchexecute')) {
      batches++
      return { status: 200, body: JSON.stringify(JSON.parse(form)[0].map(([, request, , tag]) => ['wrb.fr', 'Fbv4je', JSON.stringify(['garturlres', `https://${request.match(/"(m\d+)"/)[1]}.example/`, 1]), null, null, null, tag])) }
    }
    const first = decodeURIComponent(url).match(/after:([\d-]+)/)[1]
    return { status: 200, body: `<rss><item><title>T</title><link>https://news.google.com/rss/articles/m${first.replace(/-/g, '')}</link><pubDate>${first}</pubDate></item></rss>` }
  }
  const found = await gate.gnews('Many Months', '2025-01-01', '2025-12-31', { data, state, call, now: new Date('2026-10-03T12:00:00Z'), workers: 4, batch: 5 })
  assert.deepEqual(found.map(a => a.url), Array.from({ length: 12 }, (_, i) => `https://m2025${String(i + 1).padStart(2, '0')}01.example/`), 'each month\'s article under its own address')
  assert.equal(most, 4, 'four requests in flight at once')
  assert.equal(batches, 3, 'twelve links resolved five to a request')
  rmSync(data, { recursive: true, force: true })
})

test('pacedAsync spaces the calls with one key by the gap without blocking the process', async () => {
  let ticked = false
  setTimeout(() => { ticked = true }, 50)
  const t0 = Date.now()
  await Promise.all([gate.pacedAsync('ta', 0.3, state), gate.pacedAsync('ta', 0.3, state)])
  assert.ok(Date.now() - t0 >= 280, 'the second call waited out the gap')
  assert.ok(ticked, 'a timer ran while it waited')
})

test('ytsearch keeps the videos whose title, channel or description names the subject', () => {
  const video = (id, channel, title, description) => JSON.stringify({ id, channel, title, description })
  const stdout = [
    video('aaaaaaaaaaa', 'Fox Business', 'Recession is the only way down', 'Adam KOBEISSI joins the show'),
    video('bbbbbbbbbbb', 'Real Vision', 'Five Common Mistakes Investors Make', 'An expert view'),
    video('ccccccccccc', 'The Kobeissi Letter', 'Weekly outlook', null),
    video('ddddddddddd', 'Thoughtful Money', 'Adam Kobeissi: what comes next', ''),
  ].join('\n')
  let args
  const found = gate.ytsearch('Adam Kobeissi interview', ['Adam Kobeissi', 'Kobeissi Letter'], { count: 15, call: a => { args = a; return { status: 0, stdout, stderr: '' } } })
  assert.deepEqual(found, [
    { id: 'aaaaaaaaaaa', url: 'https://www.youtube.com/watch?v=aaaaaaaaaaa', channel: 'Fox Business', title: 'Recession is the only way down' },
    { id: 'ccccccccccc', url: 'https://www.youtube.com/watch?v=ccccccccccc', channel: 'The Kobeissi Letter', title: 'Weekly outlook' },
    { id: 'ddddddddddd', url: 'https://www.youtube.com/watch?v=ddddddddddd', channel: 'Thoughtful Money', title: 'Adam Kobeissi: what comes next' },
  ])
  assert.equal(args.at(-1), 'ytsearch15:Adam Kobeissi interview')
  assert.throws(() => gate.ytsearch('Adam Kobeissi interview', [], {}), /name/)
  assert.throws(() => gate.ytsearch('q', ['n'], { call: () => ({ status: 1, stdout: '', stderr: 'ERROR: Sign in to confirm' }) }), /Sign in/)
})

test('ytvideos shares the videos out over the routes, drops a route YouTube refuses and names the videos no route could read', async () => {
  const asked = []
  const call = async (route, ids) => {
    asked.push([route, ids])
    if (route === 'http://refused') return []
    return ids.filter(id => id !== 'private').map(id => ({ id, upload_date: '20150827', view_count: 7, title: `video ${id}` }))
  }
  const ids = ['a', 'b', 'private', 'c', 'd', 'e', 'f']
  const { videos, missing } = await gate.ytvideos(ids, { routes: [null, 'http://refused'], batch: 2, workers: 1, call })
  assert.deepEqual(videos.map(v => v.id), ['a', 'b', 'c', 'd', 'e', 'f'], 'in the order asked')
  assert.deepEqual(missing, ['private'])
  assert.equal(asked.filter(([route]) => route === 'http://refused').length, 1, 'a route that read nothing of a batch is not used again')
  assert.equal(asked.filter(([route, batch]) => route === null && batch.includes('private')).length, 2, 'a video one route could not read is asked for once more')
  assert.ok(asked.every(([, batch]) => batch.length <= 2))
  const none = await gate.ytvideos(ids, { routes: ['http://refused'], batch: 2, workers: 2, call })
  assert.deepEqual([none.videos, none.missing], [[], ids], 'refused on every route, every video is missing')
})

test('ytuploads lists the channel\'s tabs and gives every upload its exact date, oldest first', async () => {
  const listed = []
  const list = args => {
    listed.push(args.at(-1))
    return { status: 0, stdout: { videos: 'new\nold\n', shorts: 'short\ngone\n', streams: '' }[args.at(-1).split('/').at(-1)] }
  }
  const read = async ids => ({ missing: ['gone'], videos: ids.filter(id => id !== 'gone').map(id => ({ id, upload_date: { new: '20260919', old: '20120220', short: '20200101' }[id], timestamp: { new: 3, old: 1, short: 2 }[id], view_count: 5, duration: 60, title: id })) })
  const { uploads, missing } = await gate.ytuploads('https://www.youtube.com/@x/', { list, read })
  assert.deepEqual(listed, ['videos', 'shorts', 'streams'].map(tab => `https://www.youtube.com/@x/${tab}`))
  assert.deepEqual(uploads.map(u => [u.id, u.kind, u.date]), [['old', 'videos', '2012-02-20'], ['short', 'shorts', '2020-01-01'], ['new', 'videos', '2026-09-19']])
  assert.deepEqual(uploads[0], { id: 'old', kind: 'videos', date: '2012-02-20', timestamp: 1, views: 5, duration: 60, title: 'old', url: 'https://www.youtube.com/watch?v=old' })
  assert.deepEqual(missing, ['gone'])
  await assert.rejects(gate.ytuploads('https://www.youtube.com/@nobody', { list: () => ({ status: 1, stdout: '' }), read }), /no uploads listed/)
})

test('stats counts the log by command and status', () => {
  gate.log('read', 'jina', 'https://a.example', state)
  gate.log('read', 'jina', 'https://b.example', state)
  gate.log('chrome', 'FAILED', 'google search x', state)
  assert.deepEqual(gate.stats(state), { 'chrome:FAILED': 1, 'read:jina': 2 })
})

test('the command prints its usage without arguments and runs through a symlink', () => {
  const script = fileURLToPath(new URL('../scripts/gate.mjs', import.meta.url))
  const r = spawnSync(process.execPath, [script], { encoding: 'utf8' })
  assert.equal(r.status, 2)
  assert.match(r.stderr, /Usage: gate.mjs read/)
  const stats = spawnSync(process.execPath, [script, 'stats'], { encoding: 'utf8', env: { ...process.env, INTEL_STATE_DIR: root } })
  assert.equal(stats.status, 0)
  assert.equal(JSON.parse(stats.stdout)['read:jina'], 2)
})

test('a month searched before Google refused the next one has its links resolved and is kept', async () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  const july = '<item><title>July - A</title><link>https://news.google.com/rss/articles/jul</link><pubDate>Tue, 08 Jul 2025 07:00:00 GMT</pubDate><source url="https://www.a.example">A</source></item>'
  const call = (url, form) => {
    if (url.includes('/rss/articles/')) return { status: 200, body: '<c-wiz><div data-n-a-sg="s" data-n-a-ts="1"></div></c-wiz>' }
    if (url.endsWith('/batchexecute')) return { status: 200, body: `)]}'\n\n${JSON.stringify([['wrb.fr', 'Fbv4je', JSON.stringify(['garturlres', 'https://a.example/july', 1]), null, null, null, '0']])}\n` }
    return decodeURIComponent(url).includes('after:2025-07-01') ? { status: 200, body: `<rss>${july}</rss>` } : { status: 429, body: '' }
  }
  await assert.rejects(() => gate.gnews('Someone', '2025-07-01', '2025-08-31', { data, state, call, now: new Date('2026-10-03T12:00:00Z'), workers: 1 }), /429/)
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'someone', 'articles.out.json'), 'utf8')).covered, [['2025-07-01', '2025-07-31']])
  rmSync(data, { recursive: true, force: true })
})
