// ABOUTME: Tests the machine-wide gate's limits: proxy exits, pacing, slots, the Exa credit window, the kept GDELT and Google News articles, the YouTube filter and the log stats.
// ABOUTME: Everything runs against a temporary state directory; no service is called.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, test } from 'node:test'
import { fileURLToPath } from 'node:url'

const state = mkdtempSync(join(tmpdir(), 'gate-'))
process.env.CASE_STUDY_LIMITS = state
process.env.ISP_PROXY_URL = '' // the machine's .env must not reach the tests
const gate = await import('../scripts/gate.mjs')
after(() => rmSync(state, { recursive: true, force: true }))

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
  assert.ok(calls[0].includes('--project_id=p') && calls[0].some(a => a.startsWith('--maximum_bytes_billed=')))
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
  const failing = { ...options, call: args => asked++ ? { status: 1, stdout: 'Quota exceeded: Your project exceeded quota for free query bytes scanned', stderr: '' } : { status: 0, stdout: '[]', stderr: '' } }
  assert.throws(() => gate.gdelt(['nobody'], '2024-01-01', '2025-12-31', failing), /Quota exceeded/)
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gdelt', 'nobody', 'articles.out.json'), 'utf8')).covered, [['2024-01-01', '2024-12-31']], 'the year read before the failure is kept')
  assert.throws(() => gate.gdelt(['nobody'], '2025-01-01', '2025-12-31', { ...options, project: '' }), /GDELT_BQ_PROJECT/)
  assert.throws(() => gate.gdelt(['https://api.gdeltproject.org/api/v2/doc/doc?query=x'], undefined, undefined, options), /name/)
  assert.throws(() => gate.gdelt([], '2025-01-01', '2025-12-31', options), /name/)
  assert.throws(() => gate.gdelt(Array.from({ length: 101 }, (_, i) => `name ${i}`), '2025-01-01', '2025-12-31', options), /100/)
  assert.throws(() => gate.gdelt(['nobody'], '2025', '2025-12-31', options), /YYYY-MM-DD/)
  rmSync(data, { recursive: true, force: true })
})

test('gnews asks Google News for each week it lacks, keeps the articles under their own addresses, and stops at the first refusal', async () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  const item = (title, link, date, source) => `<item><title>${title}</title><link>${link}</link><pubDate>${date}</pubDate><source url="https://www.${source}">X</source></item>`
  const weeks = {
    '2025-06-02': item('Stocks &amp; bonds - A', 'https://news.google.com/rss/articles/one', 'Tue, 03 Jun 2025 07:00:00 GMT', 'a.example'),
    '2025-06-09': item('Second - B', 'https://news.google.com/rss/articles/two', 'Wed, 11 Jun 2025 07:00:00 GMT', 'b.example') + item('Again - A', 'https://news.google.com/rss/articles/one', 'Tue, 03 Jun 2025 07:00:00 GMT', 'a.example'),
  }
  const asked = []
  const resolved = []
  // Google's page for a link carries a signature, and its batchexecute answers with the article's address for it
  const call = (url, form) => {
    const link = url.match(/rss\/articles\/(\w+)$/)
    if (link) return { status: 200, body: link[1] === 'bare' ? '<c-wiz></c-wiz>' : `<c-wiz><div jscontroller="x" data-n-a-sg="sig-${link[1]}" data-n-a-ts="17"></div></c-wiz>` }
    if (url.endsWith('/batchexecute')) {
      const [, id, ts, sg] = JSON.parse(form)[0][0][1].match(/"(\w+)",(\d+),"sig-(\w+)"\]$/)
      assert.deepEqual([ts, sg], ['17', id])
      resolved.push(id)
      return { status: 200, body: `)]}'\n\n${JSON.stringify([['wrb.fr', 'Fbv4je', JSON.stringify(['garturlres', `https://${id}.example/story?a=1&b=2`, 1])], ['di', 11]])}\n` }
    }
    const week = decodeURIComponent(url).match(/after:([\d-]+) before:([\d-]+)/); asked.push(`${week[1]} ${week[2]}`); return { status: 200, body: `<rss>${weeks[week[1]] || ''}</rss>` }
  }
  const options = { data, state, call, now: new Date('2026-10-03T12:00:00Z'), gap: 0 }
  assert.deepEqual((await gate.gnews('Kobeissi Letter', '2025-06-04', '2025-06-12', options)), [
    { url: 'https://two.example/story?a=1&b=2', domain: 'b.example', date: '2025-06-11', title: 'Second - B' },
  ], 'only the days asked for')
  assert.deepEqual(asked, ['2025-06-02 2025-06-09', '2025-06-09 2025-06-16'], 'whole weeks, Monday to Monday')
  assert.deepEqual((await gate.gnews('kobeissi letter', '2025-06-02', '2025-06-15', options)).map(a => a.title), ['Stocks & bonds - A', 'Second - B'], 'one article per link, oldest first')
  assert.equal(asked.length, 2, 'weeks already held are not asked for again')
  assert.deepEqual(resolved, ['one', 'two'], 'a link is resolved once')
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'kobeissi-letter', 'articles.out.json'), 'utf8')), { name: 'kobeissi letter', covered: [['2025-06-02', '2025-06-15']], count: 2 })
  let n = 0
  const refusing = { ...options, call: url => { asked.push('x'); return n++ ? { status: 429, body: '' } : { status: 200, body: '<rss></rss>' } } }
  await assert.rejects(() => gate.gnews('Kobeissi Letter', '2025-06-02', '2025-07-06', refusing), /429/)
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'kobeissi-letter', 'articles.out.json'), 'utf8')).covered, [['2025-06-02', '2025-06-22']], 'the week read before the refusal is kept')
  weeks['2025-06-23'] = item('No signature - C', 'https://news.google.com/rss/articles/bare?oc=5', 'Tue, 24 Jun 2025 07:00:00 GMT', 'c.example')
  assert.deepEqual((await gate.gnews('Kobeissi Letter', '2025-06-23', '2025-06-29', options)).map(a => a.url), ['https://news.google.com/rss/articles/bare?oc=5'], 'a link Google gives no address for stays as it is')
  weeks['2025-06-30'] = item('Third - D', 'https://news.google.com/rss/articles/three', 'Tue, 01 Jul 2025 07:00:00 GMT', 'd.example')
  const unresolved = { ...options, call: (url, form) => url.includes('/rss/articles/') ? { status: 429, body: '' } : call(url, form) }
  await assert.rejects(() => gate.gnews('Kobeissi Letter', '2025-06-30', '2025-07-06', unresolved), /429/)
  assert.deepEqual(JSON.parse(readFileSync(join(data, 'gnews', 'kobeissi-letter', 'articles.out.json'), 'utf8')).covered, [['2025-06-02', '2025-06-29']], 'a week whose links were not resolved is asked for again')
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
  for (const refusal of [503, 429]) {
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

test('gnews asks for several weeks at once and resolves their links while other weeks are asked for', async () => {
  const data = mkdtempSync(join(tmpdir(), 'data-'))
  let open = 0
  let most = 0
  const call = async url => {
    open++; most = Math.max(most, open)
    await new Promise(resolve => setTimeout(resolve, 20))
    open--
    if (url.includes('/rss/articles/')) return { status: 200, body: '<c-wiz></c-wiz>' }
    const monday = decodeURIComponent(url).match(/after:([\d-]+)/)[1]
    return { status: 200, body: `<rss><item><title>T</title><link>https://news.google.com/rss/articles/w${monday.replace(/-/g, '')}</link><pubDate>${monday}</pubDate></item></rss>` }
  }
  const found = await gate.gnews('Many Weeks', '2025-01-06', '2025-03-30', { data, state, call, now: new Date('2026-10-03T12:00:00Z'), gap: 0, workers: 4 })
  assert.equal(found.length, 12, 'one article for each of the twelve weeks')
  assert.equal(most, 4, 'four requests in flight at once')
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
  const stats = spawnSync(process.execPath, [script, 'stats'], { encoding: 'utf8', env: { ...process.env, CASE_STUDY_LIMITS: state } })
  assert.equal(stats.status, 0)
  assert.equal(JSON.parse(stats.stdout)['read:jina'], 2)
})
