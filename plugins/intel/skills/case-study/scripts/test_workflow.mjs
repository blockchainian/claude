// ABOUTME: Runs the creator workflow script against stand-in agents to check its control flow:
// ABOUTME: scouted sources are de-duplicated and batched, every chapter is written, reviewed in slices, and fixed.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = readFileSync(new URL('../workflows/creator.js', import.meta.url), 'utf8').replace('export const meta', 'const meta')
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor

async function run(args) {
  const calls = []
  const agent = async (prompt, opts) => {
    calls.push({ prompt, ...opts })
    if (opts.label.startsWith('scout:')) {
      const lane = opts.label.slice(6)
      // 30 sources per lane, one shared by every lane
      return { sources: [{ url: 'https://shared.example/a/', outlet: 'Shared' },
        ...Array.from({ length: 29 }, (_, i) => ({ url: `https://${lane}.example/${i}`, outlet: lane, year: '2020' }))] }
    }
    if (opts.label === 'merge:read') return '{"sources": 130, "failed": 0, "gaps": 4}'
    return `done ${opts.label}`
  }
  const parallel = thunks => Promise.all(thunks.map(t => t()))
  const body = new AsyncFunction('agent', 'parallel', 'pipeline', 'phase', 'log', 'args', source)
  const result = await body(agent, parallel, null, () => {}, () => {}, args)
  return { calls, result }
}

const ARGS = { subject: 'Jane Doe', work: '/w', skill: '/s', lang: 'English', today: '2026-01-01', chapters: 12, tools: '', product: 'a product', seeds: '', caps: 'video site: 300 requests' }
const labels = (calls, prefix) => calls.filter(c => c.label.startsWith(prefix)).map(c => c.label)

test('sources found by several scouts are read once, eight to a reader', async () => {
  const { calls, result } = await run(ARGS)
  assert.equal(result.scouted, 4 * 29 + 1)
  assert.equal(labels(calls, 'read:').length, Math.ceil(117 / 8))
  assert.equal(calls.filter(c => c.prompt.includes('https://shared.example/a/')).filter(c => c.label.startsWith('read:')).length, 1)
  assert.deepEqual(labels(calls, 'numbers:'), ['numbers:archive', 'numbers:uploads'])
})

test('every chapter but the sources list is written, and the introduction and reasoning chapters come last', async () => {
  const { calls } = await run(ARGS)
  const written = labels(calls, 'write:')
  assert.deepEqual([...written].sort(), ['01', '02', '03', '04', '05', '06', '07', '08', '09', '10', '11'].map(f => `write:${f}`))
  assert.deepEqual(written.slice(-3), ['write:01', 'write:10', 'write:11'])
  assert.ok(calls.find(c => c.label === 'write:11').prompt.includes('a product'))
})

test('without a product there is no product chapter', async () => {
  const { calls } = await run({ ...ARGS, chapters: 11, product: '' })
  assert.ok(!labels(calls, 'write:').includes('write:11'))
  assert.ok(!labels(calls, 'fix:').includes('fix:11'))
})

test('the review covers every source and every chapter, and every chapter is fixed', async () => {
  const { calls } = await run(ARGS)
  // readers followed reposts to originals: 130 sources after the merge, not the 117 scouted
  const sourceSlices = calls.filter(c => c.label.startsWith('review:sources-'))
  assert.equal(sourceSlices.length, 6)
  sourceSlices.forEach((c, i) => assert.ok(c.prompt.includes(`case_study.py slice "/w" ${i + 1} 6`), c.prompt))
  for (const lens of ['numbers', 'quotes']) {
    const slices = calls.filter(c => c.label.startsWith(`review:${lens}-`))
    assert.ok(slices.every(c => c.prompt.match(/md\/\d\d\.md/g).length <= 2), 'at most two chapters per reviewer')
    const files = slices.flatMap(c => c.prompt.match(/md\/\d\d\.md/g))
    assert.deepEqual(files.sort(), ['01', '02', '03', '04', '05', '06', '07', '08', '09', '10', '11'].map(f => `md/${f}.md`))
  }
  assert.equal(labels(calls, 'fix:').length, 11)
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:fix'])
})

test('reviewers run on Sonnet at high effort and each agent is told its share of the caps', async () => {
  const { calls } = await run(ARGS)
  assert.ok(calls.filter(c => c.label.startsWith('review:')).every(c => c.model === 'sonnet' && c.effort === 'high'))
  assert.ok(calls.find(c => c.label === 'read:01').prompt.includes('1/17 share'))
})
