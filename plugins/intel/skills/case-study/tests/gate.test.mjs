// ABOUTME: Tests the machine-wide gate's limits: proxy exits, pacing, slots, the Exa credit window and the log stats.
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
