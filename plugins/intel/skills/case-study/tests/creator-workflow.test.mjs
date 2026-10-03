// ABOUTME: Runs the creator workflow script against stand-in agents to check its control flow:
// ABOUTME: scouted sources are de-duplicated and batched, every chapter is written, has its figures matched by a
// ABOUTME: script, is reviewed and is fixed as soon as its own reviews are done; no chapter is written before the sources lens is in.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = readFileSync(new URL('../workflows/creator.mjs', import.meta.url), 'utf8').replace('export const meta', 'const meta')
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

const ARGS = { subject: 'Jane Doe', work: '/w', skill: '/s', lang: 'English', today: '2026-01-01', product: 'a product', seeds: '', caps: 'video site: 300 requests' }
const labels = (calls, prefix) => calls.filter(c => c.label.startsWith(prefix)).map(c => c.label)
const ALL = ['01', '02', '03', '04', '05', '06', '07', '08', '09', '10']

test('sources found by several scouts are read once, eight to a reader; the numbers agents start with the scouts', async () => {
  const { calls, result } = await run(ARGS)
  assert.equal((await result()).scouted, 4 * 29 + 1)
  assert.equal(labels(calls, 'read:').length, Math.ceil(117 / 8))
  assert.equal(calls.filter(c => c.prompt.includes('https://shared.example/a/')).filter(c => c.label.startsWith('read:')).length, 1)
  assert.deepEqual(labels(calls, 'numbers:'), ['numbers:archive', 'numbers:uploads'])
  assert.ok(calls.findIndex(c => c.label === 'numbers:archive') < calls.findIndex(c => c.label === 'read:01'))
})

test('every chapter but the sources list is written, and the introduction and the reasoning chapter come after the others', async () => {
  const { calls } = await run(ARGS)
  const written = labels(calls, 'write:')
  assert.deepEqual([...written].sort(), ALL.map(f => `write:${f}`))
  assert.deepEqual(written.slice(-2).sort(), ['write:01', 'write:10'])
  assert.ok(calls.find(c => c.label === 'write:10').prompt.includes('a product'))
})

test('a product adds no chapter: it is given to the writer of the one reasoning chapter, and only when there is one', async () => {
  const { calls } = await run({ ...ARGS, product: '' })
  assert.deepEqual(labels(calls, 'write:').sort(), ALL.map(f => `write:${f}`))
  assert.ok(!calls.find(c => c.label === 'write:10').prompt.includes('product'))
})

test('the review covers every source and every chapter, one chapter per reviewer, and every chapter is fixed', async () => {
  const { calls } = await run(ARGS)
  // readers followed reposts to originals: 130 sources after the merge, not the 117 scouted
  const sourceSlices = calls.filter(c => c.label.startsWith('review:sources-'))
  assert.equal(sourceSlices.length, 6)
  sourceSlices.forEach((c, i) => assert.ok(c.prompt.includes(`case-study.mjs slice "/w" ${i + 1} 6`), c.prompt))
  const quotes = calls.filter(c => c.label.startsWith('review:quotes-'))
  assert.deepEqual(quotes.map(c => c.prompt.match(/drafts\/\d\d\.md/g)).sort(), ALL.map(f => [`drafts/${f}.md`]))
  assert.deepEqual(labels(calls, 'fix:').sort(), ALL.map(f => `fix:${f}`))
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:sources', 'merge:fix'])
})

test('no chapter is written before the sources lens is in and its failed sources are merged out; a chapter is fixed when its own reviews are done', async () => {
  const { calls, release } = await run(ARGS, ['review:sources-2', 'write:05'])
  assert.equal(labels(calls, 'review:sources-').length, 6)
  assert.deepEqual([...labels(calls, 'write:'), ...labels(calls, 'merge:sources')], [], 'writers wait for every sources reviewer')
  await release() // sources-2 finishes: the failed sources are merged out, then the body chapters are written
  assert.ok(calls.findIndex(c => c.label === 'merge:sources') < calls.findIndex(c => c.label === 'write:02'), 'the merge comes before the first writer')
  // the body chapters other than 05 are reviewed and fixed at once; the introduction and the reasoning chapter wait for 05
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), ['02', '03', '04', '06', '07', '08', '09'].map(f => `review:quotes-${f}`))
  assert.deepEqual(labels(calls, 'write:').filter(l => ['write:01', 'write:10'].includes(l)), [])
  assert.deepEqual(labels(calls, 'fix:').sort(), ['02', '03', '04', '06', '07', '08', '09'].map(f => `fix:${f}`))
  await release() // write:05 finishes; the stand-ins let the rest run through
  const fixes = labels(calls, 'fix:')
  assert.equal(fixes.length, 10)
  assert.deepEqual(fixes.slice(-2).sort(), ['fix:01', 'fix:10'], 'the introduction and the reasoning chapter are fixed after the others')
})

test('a script matches every chapter\'s figures; only the timeline and turning-point chapters get a record reviewer, who judges the unmatched ones', async () => {
  const { calls } = await run(ARGS, ['review:record-03'])
  assert.equal(labels(calls, 'review:numbers-').length, 0)
  const matchers = calls.filter(c => c.label.startsWith('figures:'))
  assert.deepEqual(matchers.map(c => c.label).sort(), ALL.map(f => `figures:${f}`))
  assert.ok(matchers.every(c => c.model === 'haiku' && c.prompt.includes(`case-study.mjs figures "/w" ${c.label.slice(8)}`)))
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

test('with the reading done, the run starts at the draft: no scout, reader or numbers agent, the merge still sizes the sources lens', async () => {
  const { calls, result } = await run({ ...ARGS, done: 'read' })
  assert.deepEqual([...labels(calls, 'scout:'), ...labels(calls, 'read:'), ...labels(calls, 'numbers:')], [])
  assert.equal(calls[0].label, 'merge:read')
  assert.equal(labels(calls, 'review:sources-').length, 6)
  assert.deepEqual(labels(calls, 'write:').sort(), ALL.map(f => `write:${f}`))
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), ALL.map(f => `review:quotes-${f}`))
  assert.deepEqual(labels(calls, 'fix:').sort(), ALL.map(f => `fix:${f}`))
  assert.deepEqual((await result()).sources, Array.from({ length: 6 }, (_, i) => `done review:sources-${i + 1}`))
})

test('with the sources lens done too, no source is reviewed again and every chapter is still reviewed and fixed', async () => {
  const { calls, result } = await run({ ...ARGS, done: 'read, sources' })
  assert.deepEqual(labels(calls, 'review:sources-'), [])
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), ALL.map(f => `review:quotes-${f}`))
  assert.deepEqual(labels(calls, 'fix:').sort(), ALL.map(f => `fix:${f}`))
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:fix'], 'the merge after reading already left the failed sources out')
  assert.deepEqual((await result()).sources, [])
})
