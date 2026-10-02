// ABOUTME: Runs the creator workflow script against stand-in agents to check its control flow:
// ABOUTME: scouted sources are de-duplicated and batched, every chapter is written, has its figures matched by a
// ABOUTME: script, is reviewed, and is fixed as soon as its own reviews and the sources lens are done.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = readFileSync(new URL('../workflows/creator.js', import.meta.url), 'utf8').replace('export const meta', 'const meta')
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor

// Every stand-in agent finishes on the next tick, except those named in `slow`, which finish when `release` is called:
// the order of `calls` then shows which agents waited for which.
async function run(args, slow = []) {
  const calls = []
  const releases = []
  const agent = (prompt, opts) => {
    calls.push({ prompt, ...opts })
    const finish = () => {
      if (opts.label.startsWith('scout:')) {
        const lane = opts.label.slice(6)
        // 30 sources per lane, one shared by every lane
        return { sources: [{ url: 'https://shared.example/a/', outlet: 'Shared' },
          ...Array.from({ length: 29 }, (_, i) => ({ url: `https://${lane}.example/${i}`, outlet: lane, year: '2020' }))] }
      }
      if (opts.label === 'merge:read') return '{"sources": 130, "failed": 0, "gaps": 4}'
      return `done ${opts.label}`
    }
    if (slow.includes(opts.label)) return new Promise(resolve => releases.push(() => resolve(finish())))
    return Promise.resolve().then(finish)
  }
  const parallel = thunks => Promise.all(thunks.map(t => t()))
  const body = new AsyncFunction('agent', 'parallel', 'pipeline', 'phase', 'log', 'args', source)
  const done = body(agent, parallel, null, () => {}, () => {}, args)
  const settle = () => new Promise(resolve => setTimeout(resolve, 20))
  await settle()
  const release = async () => { releases.splice(0).forEach(r => r()); await settle() }
  return { calls, release, result: () => done }
}

const ARGS = { subject: 'Jane Doe', work: '/w', skill: '/s', lang: 'English', today: '2026-01-01', chapters: 12, tools: '', product: 'a product', seeds: '', caps: 'video site: 300 requests' }
const labels = (calls, prefix) => calls.filter(c => c.label.startsWith(prefix)).map(c => c.label)
const ALL = ['01', '02', '03', '04', '05', '06', '07', '08', '09', '10', '11']

test('sources found by several scouts are read once, eight to a reader; the numbers agents start with the scouts', async () => {
  const { calls, result } = await run(ARGS)
  assert.equal((await result()).scouted, 4 * 29 + 1)
  assert.equal(labels(calls, 'read:').length, Math.ceil(117 / 8))
  assert.equal(calls.filter(c => c.prompt.includes('https://shared.example/a/')).filter(c => c.label.startsWith('read:')).length, 1)
  assert.deepEqual(labels(calls, 'numbers:'), ['numbers:archive', 'numbers:uploads'])
  assert.ok(calls.findIndex(c => c.label === 'numbers:archive') < calls.findIndex(c => c.label === 'read:01'))
})

test('every chapter but the sources list is written, and the introduction and reasoning chapters come after the others', async () => {
  const { calls } = await run(ARGS)
  const written = labels(calls, 'write:')
  assert.deepEqual([...written].sort(), ALL.map(f => `write:${f}`))
  assert.deepEqual(written.slice(-3).sort(), ['write:01', 'write:10', 'write:11'])
  assert.ok(calls.find(c => c.label === 'write:11').prompt.includes('a product'))
})

test('without a product there is no product chapter', async () => {
  const { calls } = await run({ ...ARGS, chapters: 11, product: '' })
  assert.ok(!labels(calls, 'write:').includes('write:11'))
  assert.ok(!labels(calls, 'fix:').includes('fix:11'))
})

test('the review covers every source and every chapter, one chapter per reviewer, and every chapter is fixed', async () => {
  const { calls } = await run(ARGS)
  // readers followed reposts to originals: 130 sources after the merge, not the 117 scouted
  const sourceSlices = calls.filter(c => c.label.startsWith('review:sources-'))
  assert.equal(sourceSlices.length, 6)
  sourceSlices.forEach((c, i) => assert.ok(c.prompt.includes(`case_study.py slice "/w" ${i + 1} 6`), c.prompt))
  const quotes = calls.filter(c => c.label.startsWith('review:quotes-'))
  assert.deepEqual(quotes.map(c => c.prompt.match(/md\/\d\d\.md/g)).sort(), ALL.map(f => [`md/${f}.md`]))
  assert.deepEqual(labels(calls, 'fix:').sort(), ALL.map(f => `fix:${f}`))
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:fix'])
})

test('the sources lens starts with the writers; a chapter is fixed when its own reviews and the sources lens are done', async () => {
  const { calls, release } = await run(ARGS, ['write:05', 'review:sources-2'])
  assert.ok(labels(calls, 'review:sources-').length === 6 && labels(calls, 'write:').length >= 8, 'sources reviewers run alongside the writers')
  // the body chapters other than 05 are reviewed at once; the introduction and reasoning chapters wait for 05 to be written
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), ['02', '03', '04', '06', '07', '08', '09'].map(f => `review:quotes-${f}`))
  assert.deepEqual(labels(calls, 'write:').filter(l => ['write:01', 'write:10', 'write:11'].includes(l)), [])
  assert.deepEqual(labels(calls, 'fix:'), [], 'no chapter is fixed before the sources lens is done')
  await release() // sources-2 and write:05 finish; the stand-ins let the rest run through
  const fixes = labels(calls, 'fix:')
  assert.equal(fixes.length, 11)
  assert.deepEqual(fixes.slice(-3).sort(), ['fix:01', 'fix:10', 'fix:11'], 'the introduction and reasoning chapters are fixed after the others')
})

test('a script matches every chapter\'s figures; only the timeline and turning-point chapters get a record reviewer, who judges the unmatched ones', async () => {
  const { calls } = await run(ARGS, ['review:record-03'])
  assert.equal(labels(calls, 'review:numbers-').length, 0)
  const matchers = calls.filter(c => c.label.startsWith('figures:'))
  assert.deepEqual(matchers.map(c => c.label).sort(), ALL.map(f => `figures:${f}`))
  assert.ok(matchers.every(c => c.model === 'haiku' && c.prompt.includes(`case_study.py figures "/w" ${c.label.slice(8)}`)))
  assert.deepEqual(matchers.filter(c => c.prompt.includes('--worklist')).map(c => c.label).sort(), ['figures:03', 'figures:07'])
  assert.deepEqual(labels(calls, 'review:record-').sort(), ['review:record-03', 'review:record-07'])
  assert.ok(calls.findIndex(c => c.label === 'figures:03') < calls.findIndex(c => c.label === 'review:record-03'), 'the reviewer starts from the script\'s worklist')
  assert.ok(!labels(calls, 'fix:').includes('fix:03') && labels(calls, 'fix:').includes('fix:04'), 'chapter 03 is fixed only after its record review')
})

test('source and quote reviewers run on Opus, record reviewers on the session model, all at high effort, and each agent is told its share of the caps', async () => {
  const { calls } = await run(ARGS)
  const reviewers = calls.filter(c => c.label.startsWith('review:'))
  assert.ok(reviewers.every(c => c.effort === 'high'))
  assert.ok(reviewers.filter(c => !c.label.startsWith('review:record')).every(c => c.model === 'opus'))
  assert.ok(reviewers.filter(c => c.label.startsWith('review:record')).every(c => c.model === undefined))
  assert.ok(calls.find(c => c.label === 'read:01').prompt.includes('1/17 share'))
})
