// ABOUTME: Tests the machine-wide gate's limits: proxy exits, pacing, slots, the Exa credit window, the kept GDELT years and the log stats.
// ABOUTME: Everything runs against a temporary state directory; no service is called.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
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

test('gdelt asks BigQuery once for the years it lacks, keeps them, and answers a date range from what it kept', () => {
  const calls = []
  const call = args => {
    calls.push(args)
    return { status: 0, stderr: '', stdout: JSON.stringify([
      { url: 'https://a.example/late', seen: '20231129194500', domain: 'a.example', mentions: '3' },
      { url: 'https://b.example/early', seen: '20220105000000', domain: 'b.example', mentions: '1' },
    ]) }
  }
  const options = { state, call, project: 'p', now: new Date('2026-10-03T12:00:00Z') }
  assert.deepEqual(gate.gdelt('Kobeissi Letter', '2022-01-01', '2023-12-31', options), [
    { url: 'https://b.example/early', domain: 'b.example', date: '2022-01-05', mentions: 1 },
    { url: 'https://a.example/late', domain: 'a.example', date: '2023-11-29', mentions: 3 },
  ])
  const args = calls[0]
  assert.ok(args.includes('--project_id=p') && args.includes('--parameter=name:STRING:kobeissi letter'), 'the name is a parameter, lowercased')
  assert.ok(args.includes('--parameter=from:TIMESTAMP:2022-01-01 00:00:00') && args.includes('--parameter=to:TIMESTAMP:2024-01-01 00:00:00'), 'whole years are asked for')
  assert.ok(args.some(a => a.startsWith('--maximum_bytes_billed=')))
  assert.match(args.at(-1), /gkg_partitioned[\s\S]*_PARTITIONTIME >= @from[\s\S]*AllNames/)
  assert.deepEqual(gate.gdelt('kobeissi  LETTER', '2023-06-01', '2023-12-31', options).map(r => r.url), ['https://a.example/late'], 'the same name in another spelling, a range inside a kept year')
  assert.deepEqual(gate.gdelt('Kobeissi Letter', '2022-02-01', '2022-12-31', options), [], 'a kept year with nothing in the range')
  assert.equal(calls.length, 1, 'kept years are not asked for again')
  gate.gdelt('Kobeissi Letter', '2021-01-01', '2023-12-31', options)
  assert.ok(calls[1].includes('--parameter=from:TIMESTAMP:2021-01-01 00:00:00') && calls[1].includes('--parameter=to:TIMESTAMP:2022-01-01 00:00:00'), 'only the missing year')
})

test('gdelt asks again for the running year after a day, and says why BigQuery failed', () => {
  let calls = 0
  const call = () => { calls++; return { status: 0, stderr: '', stdout: '[]' } }
  const day1 = { state, call, project: 'p', now: new Date('2026-10-03T12:00:00Z') }
  gate.gdelt('someone new', '2026-01-01', '2026-10-03', day1)
  gate.gdelt('someone new', '2026-01-01', '2026-10-03', { ...day1, now: new Date('2026-10-03T20:00:00Z') })
  assert.equal(calls, 1, 'the same day')
  gate.gdelt('someone new', '2026-01-01', '2026-10-05', { ...day1, now: new Date('2026-10-05T12:00:00Z') })
  assert.equal(calls, 2, 'two days later the running year is asked for again')
  gate.gdelt('someone new', '2026-01-01', '2026-12-31', { ...day1, now: new Date('2027-02-01T12:00:00Z') })
  assert.equal(calls, 3, 'a year kept before it ended is asked for once more')
  gate.gdelt('someone new', '2026-01-01', '2026-12-31', { ...day1, now: new Date('2027-06-01T12:00:00Z') })
  assert.equal(calls, 3, 'a year kept after it ended stays')
  const failing = { state, project: 'p', now: day1.now, call: () => ({ status: 1, stdout: 'Quota exceeded: Your project exceeded quota for free query bytes scanned', stderr: '' }) }
  assert.throws(() => gate.gdelt('nobody', '2025-01-01', '2025-12-31', failing), /Quota exceeded/)
  assert.throws(() => gate.gdelt('nobody', '2025-01-01', '2025-12-31', { ...failing, project: '' }), /GDELT_BQ_PROJECT/)
  assert.throws(() => gate.gdelt('https://api.gdeltproject.org/api/v2/doc/doc?query=x', undefined, undefined, failing), /name/)
  assert.throws(() => gate.gdelt('nobody', '2025', '2025-12-31', failing), /YYYY-MM-DD/)
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
