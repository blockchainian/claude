// ABOUTME: Runs the creator workflow script against stand-in agents to check its control flow:
// ABOUTME: scouted sources are de-duplicated and batched, every chapter from the notes is written, has its figures matched
// ABOUTME: by a script, is reviewed and is fixed once the sources it names are checked; the introduction and the reasoning chapter come last.
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
      if (opts.label === 'cited:sources') return '{"cited": 60}'
      return `done ${opts.label}`
    }
    if (slow.includes(opts.label)) return new Promise(resolve => releases.push(() => resolve(finish())))
    return Promise.resolve().then(finish)
  }
  const parallel = thunks => Promise.all(thunks.map(t => t()))
  const body = new AsyncFunction('agent', 'parallel', 'pipeline', 'phase', 'log', 'args', source)
  const done = body(agent, parallel, null, () => {}, () => {}, args)
  done.catch(() => {}) // a script that refuses its args rejects here; the test reads it through result()
  const settle = () => new Promise(resolve => setTimeout(resolve, 20))
  await settle()
  const release = async () => { releases.splice(0).forEach(r => r()); await settle() }
  return { calls, release, result: () => done }
}

const ARGS = { subject: 'Jane Doe', work: '/w', skill: '/s', lang: 'English', today: '2026-01-01', product: 'a product', seeds: '', caps: 'video site: 300 requests' }
const labels = (calls, prefix) => calls.filter(c => c.label.startsWith(prefix)).map(c => c.label)
const BODY = ['02', '03', '04', '05', '06', '07', '08', '09']

test('sources found by several scouts are read once, eight to a reader; the numbers agents start with the scouts', async () => {
  const { calls, result } = await run(ARGS)
  assert.equal((await result()).scouted, 4 * 29 + 1)
  assert.equal(labels(calls, 'read:').length, Math.ceil(117 / 8))
  assert.equal(calls.filter(c => c.prompt.includes('https://shared.example/a/')).filter(c => c.label.startsWith('read:')).length, 1)
  assert.deepEqual(labels(calls, 'scout:').filter(l => l.endsWith('-numbers')), ['scout:follower-numbers', 'scout:posting-numbers'])
  assert.ok(labels(calls, 'scout:').includes('scout:analysts-and-critics'))
  assert.ok(calls.findIndex(c => c.label === 'scout:follower-numbers') < calls.findIndex(c => c.label === 'read:01'))
})

test('the chapters from the notes are written at once; one agent writes the introduction and the reasoning chapter into the book, last', async () => {
  const { calls } = await run(ARGS)
  assert.deepEqual(labels(calls, 'write:').sort(), BODY.map(f => `write:${f}`))
  const ends = calls.at(-1)
  assert.equal(ends.label, 'book:01-and-10')
  assert.match(ends.prompt, /briefs\/book\.md/)
  assert.ok(ends.prompt.includes('book/01.md') && ends.prompt.includes('book/10.md') && ends.prompt.includes('a product'))
  assert.ok(ends.model === undefined && ends.effort === 'high', 'the reasoning chapter is the book\'s point: it keeps the session model')
})

test('a product adds no chapter: it is given to the writer of the reasoning chapter, and only when there is one', async () => {
  const { calls } = await run({ ...ARGS, product: '' })
  assert.deepEqual(labels(calls, 'write:').sort(), BODY.map(f => `write:${f}`))
  assert.ok(!calls.find(c => c.label === 'book:01-and-10').prompt.includes('product'))
  assert.ok(labels(calls, 'write:').every(l => !calls.find(c => c.label === l).prompt.includes('product')))
})

test('the review covers the sources the chapters name and every chapter, one chapter per reviewer, and every chapter is fixed', async () => {
  const { calls } = await run(ARGS)
  // 130 sources were read; the chapters name 60 of them
  const sourceSlices = calls.filter(c => c.label.startsWith('review:sources-'))
  assert.equal(sourceSlices.length, 3)
  sourceSlices.forEach((c, i) => assert.ok(c.prompt.includes(`case-study.mjs slice "/w" ${i + 1} 3`), c.prompt))
  assert.match(calls.find(c => c.label === 'cited:sources').prompt, /case-study\.mjs cited "\/w"/)
  const quotes = calls.filter(c => c.label.startsWith('review:quotes-'))
  assert.deepEqual(quotes.map(c => c.prompt.match(/drafts\/\d\d\.md/g)).sort(), BODY.map(f => [`drafts/${f}.md`]))
  assert.deepEqual(labels(calls, 'fix:').sort(), BODY.map(f => `fix:${f}`))
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:sources', 'merge:fix'])
})

test('the phases are listed in the order they start: the chapters are written right after the reading, the sources lens starts once they are', async () => {
  const { calls } = await run(ARGS)
  const listed = [...source.matchAll(/\{ title: '([^']+)'/g)].map(m => m[1])
  const started = [...new Set(calls.map(c => c.phase).filter(Boolean))]
  assert.deepEqual(started, listed)
  assert.deepEqual(listed, ['Scout', 'Read', 'Write', 'Review', 'Check sources', 'Fix', 'Book'])
  assert.deepEqual([...new Set(calls.filter(c => c.label.startsWith('review:sources-')).map(c => c.phase))], ['Check sources'])
})

test('no writer waits for the sources lens; the lens starts when every chapter is written, and no chapter is fixed before its failed sources are merged out', async () => {
  const { calls, release } = await run(ARGS, ['write:05', 'review:sources-2'])
  assert.equal(labels(calls, 'write:').length, 8, 'every writer starts with the reading done')
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), ['02', '03', '04', '06', '07', '08', '09'].map(f => `review:quotes-${f}`), 'a written chapter is reviewed at once')
  assert.deepEqual([...labels(calls, 'review:sources-'), ...labels(calls, 'fix:')], [], 'the sources lens waits for the last chapter: it checks the sources the chapters name')
  await release() // write:05 finishes: the cited sources are counted and their reviewers start
  assert.equal(labels(calls, 'review:sources-').length, 3)
  assert.deepEqual([...labels(calls, 'fix:'), ...labels(calls, 'merge:sources')], [], 'fixers wait for every sources reviewer')
  await release() // sources-2 finishes; the stand-ins let the rest run through
  assert.ok(calls.findIndex(c => c.label === 'merge:sources') < calls.findIndex(c => c.label.startsWith('fix:')), 'the merge comes before the first fixer')
  assert.equal(labels(calls, 'fix:').length, 8)
  assert.ok(calls.findIndex(c => c.label === 'merge:fix') < calls.findIndex(c => c.label === 'book:01-and-10'), 'the introduction and the reasoning chapter are written from the fixed chapters')
})

test('a script matches every chapter\'s figures; only the timeline and turning-point chapters get a record reviewer, who judges the unmatched ones', async () => {
  const { calls } = await run(ARGS, ['review:record-03'])
  assert.equal(labels(calls, 'review:numbers-').length, 0)
  const matchers = calls.filter(c => c.label.startsWith('figures:'))
  assert.deepEqual(matchers.map(c => c.label).sort(), BODY.map(f => `figures:${f}`))
  assert.ok(matchers.every(c => c.model === 'haiku' && c.prompt.includes(`case-study.mjs figures "/w" ${c.label.slice(8)}`)))
  assert.deepEqual(matchers.filter(c => c.prompt.includes('--worklist')).map(c => c.label).sort(), ['figures:03', 'figures:07'])
  assert.deepEqual(labels(calls, 'review:record-').sort(), ['review:record-03', 'review:record-07'])
  assert.ok(calls.findIndex(c => c.label === 'figures:03') < calls.findIndex(c => c.label === 'review:record-03'), 'the reviewer starts from the script\'s worklist')
  assert.ok(!labels(calls, 'fix:').includes('fix:03') && labels(calls, 'fix:').includes('fix:04'), 'chapter 03 is fixed only after its record review')
})

test('writers, source and quote reviewers run on Opus 4.8, record reviewers on the session model, all at high effort, and each agent is told its share of the caps', async () => {
  const { calls } = await run(ARGS)
  const reviewers = calls.filter(c => c.label.startsWith('review:'))
  assert.ok(reviewers.every(c => c.effort === 'high'))
  assert.ok(reviewers.filter(c => !c.label.startsWith('review:record')).every(c => c.model === 'claude-opus-4-8'))
  assert.ok(reviewers.filter(c => c.label.startsWith('review:record')).every(c => c.model === undefined))
  assert.ok(calls.filter(c => c.label.startsWith('write:')).every(c => c.model === 'claude-opus-4-8' && c.effort === 'high'), 'a writer\'s chapter is the text the reader gets')
  assert.ok(calls.find(c => c.label === 'read:01').prompt.includes('1/17 share'))
})

test('with the scouting done, the known sources are read again, eight to a reader, and the numbers agents still run', async () => {
  const KNOWN = Array.from({ length: 20 }, (_, i) => ({ url: `https://known.example/${i}`, outlet: 'Known' }))
  const { calls, result } = await run({ ...ARGS, done: 'scout', sources: KNOWN })
  assert.deepEqual(labels(calls, 'scout:'), ['scout:follower-numbers', 'scout:posting-numbers'], 'only the numbers agents run in the scout stage')
  assert.equal(calls.filter(c => c.label.startsWith('sources:')).length, 0, 'no agent relays the list: an agent drops entries')
  await assert.rejects(async () => (await run({ ...ARGS, done: 'scout' })).result(), /sources/, 'the list is required')
  assert.equal(labels(calls, 'read:').length, Math.ceil(20 / 8))
  assert.ok(calls.find(c => c.label === 'read:03').prompt.includes('https://known.example/19 (Known)'))
  assert.equal((await result()).scouted, 20)
})

test('with the reading done, the run starts at the chapters: no scout, reader or numbers agent', async () => {
  const { calls, result } = await run({ ...ARGS, done: 'read' })
  assert.deepEqual([...labels(calls, 'scout:'), ...labels(calls, 'read:')], [])
  assert.equal(calls[0].label, 'merge:read')
  assert.equal(labels(calls, 'review:sources-').length, 3)
  assert.deepEqual(labels(calls, 'write:').sort(), BODY.map(f => `write:${f}`))
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), BODY.map(f => `review:quotes-${f}`))
  assert.deepEqual(labels(calls, 'fix:').sort(), BODY.map(f => `fix:${f}`))
  assert.deepEqual((await result()).sources, Array.from({ length: 3 }, (_, i) => `done review:sources-${i + 1}`))
})

test('with the sources lens done too, no source is reviewed again and every chapter is still reviewed and fixed', async () => {
  const { calls, result } = await run({ ...ARGS, done: 'read, sources' })
  assert.deepEqual([...labels(calls, 'review:sources-'), ...labels(calls, 'cited:')], [])
  assert.deepEqual(labels(calls, 'review:quotes-').sort(), BODY.map(f => `review:quotes-${f}`))
  assert.deepEqual(labels(calls, 'fix:').sort(), BODY.map(f => `fix:${f}`))
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:fix'], 'the merge after reading already left the failed sources out')
  assert.deepEqual((await result()).sources, [])
})

test('a Codex fixer is run again while findings of its chapter remain, four runs at most', async () => {
  const { calls } = await run(ARGS)
  const fixer = calls.find(c => c.label === 'fix:07')
  assert.match(fixer.prompt, /case-study\.mjs codex "\/w" 07 --model gpt-6-luna/)
  assert.match(fixer.prompt, /"remaining"/)
  assert.match(fixer.prompt, /four runs/)
})
