// ABOUTME: Runs the creator workflow script against stand-in agents to check its control flow:
// ABOUTME: the scouts start at once, scouted sources are de-duplicated and batched, every chapter from the notes is
// ABOUTME: written, and the introduction and the reasoning chapter come last.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = readFileSync(new URL('../workflows/creator.mjs', import.meta.url), 'utf8').replace('export const meta', 'const meta')
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor

// Every stand-in agent finishes on the next tick, except those named in `slow`, which finish when `release` is called:
// the order of `calls` then shows which agents waited for which.
async function run(args, slow = [], dead = []) {
  const calls = []
  const releases = []
  const agent = (prompt, opts) => {
    calls.push({ prompt, ...opts })
    if (dead.includes(opts.label)) return Promise.resolve(null) // an agent that died returns nothing
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

const LANES = ['interviews', 'own-explainers', 'internal-documents', 'press-at-the-time', 'trade-and-profiles', 'books-and-films', 'people', 'records', 'data-and-today', 'criticism']

test('one scout per source type, each told to search at once and open nothing, with its share of the caps', async () => {
  const { calls } = await run(ARGS)
  const scouts = calls.filter(c => c.label.startsWith('scout:') && !c.label.endsWith('-numbers'))
  assert.deepEqual(scouts.map(c => c.label.slice(6)).sort(), [...LANES].sort())
  assert.ok(scouts.every(c => c.prompt.includes('1/12 share')), 'the two numbers agents run beside the ten scouts')
})

test('sources found by several scouts are read once, eight to a reader; the numbers agents start with the scouts', async () => {
  const { calls, result } = await run(ARGS)
  assert.equal((await result()).scouted, 10 * 29 + 1)
  assert.equal(labels(calls, 'read:').length, Math.ceil(291 / 8))
  assert.equal(calls.filter(c => c.prompt.includes('https://shared.example/a/')).filter(c => c.label.startsWith('read:')).length, 1)
  assert.deepEqual(labels(calls, 'scout:').filter(l => l.endsWith('-numbers')), ['scout:follower-numbers', 'scout:posting-numbers'])
  assert.ok(labels(calls, 'scout:').includes('scout:criticism'))
  assert.ok(calls.findIndex(c => c.label === 'scout:follower-numbers') < calls.findIndex(c => c.label === 'read:01'))
})

test('the workflow fetches no news lists: the orchestrator fetched them before launching it, and every scout is pointed at them', async () => {
  const { calls } = await run({ ...ARGS, subject: 'Jane Doe, YouTube https://www.youtube.com/@janedoe' })
  assert.equal(labels(calls, 'news').length, 0)
  assert.ok(!calls.some(c => /case-study\.mjs news/.test(c.prompt)))
  for (const scout of calls.filter(c => c.label.startsWith('scout:') && !c.label.endsWith('-numbers'))) assert.match(scout.prompt, /raw\/news\//)
})

test('every scout is told the first year of growth, which it searches every year from', async () => {
  const { calls } = await run({ ...ARGS, since: '2017' })
  const scouts = calls.filter(c => c.label.startsWith('scout:') && !c.label.endsWith('-numbers'))
  assert.ok(scouts.every(c => c.prompt.includes('First year of growth: 2017.')))
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

test('the phases are listed in the order they start: the chapters are written right after the reading, the book once they are', async () => {
  const { calls } = await run(ARGS)
  const listed = [...source.matchAll(/\{ title: '([^']+)'/g)].map(m => m[1])
  const started = [...new Set(calls.map(c => c.phase).filter(Boolean))]
  assert.deepEqual(started, listed)
  assert.deepEqual(listed, ['Scout', 'Read', 'Write', 'Book'])
})

test('nothing reviews or fixes a chapter: the introduction and the reasoning chapter start when the last chapter is written', async () => {
  const { calls, release } = await run(ARGS, ['write:05'])
  assert.equal(labels(calls, 'write:').length, 8, 'every writer starts with the reading done')
  assert.deepEqual(labels(calls, 'book:'), [], 'the book waits for the last chapter')
  await release()
  assert.deepEqual(labels(calls, 'book:'), ['book:01-and-10'])
  assert.deepEqual(calls.map(c => c.label).filter(l => /^(review|figures|cited|fix):/.test(l)), [])
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read'])
})

test('writers run on Opus 4.8 at high effort, and each agent is told its share of the caps', async () => {
  const { calls } = await run(ARGS)
  assert.ok(calls.filter(c => c.label.startsWith('write:')).every(c => c.model === 'claude-opus-4-8' && c.effort === 'high'), 'a writer\'s chapter is the text the reader gets')
  assert.ok(calls.find(c => c.label === 'read:01').prompt.includes('1/39 share'))
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

test('a video or an episode is read two to a reader, in the first batches, so the long ones start first', async () => {
  const TEXT = Array.from({ length: 10 }, (_, i) => ({ url: `https://text.example/${i}`, outlet: 'Text' }))
  const MEDIA = ['https://www.youtube.com/watch?v=aaaaaaaaaaa', 'https://youtu.be/bbbbbbbbbbb', 'https://www.spreaker.com/episode/x--1',
    'https://podcasts.apple.com/us/podcast/x/id1?i=2', 'https://open.spotify.com/episode/abc'].map(url => ({ url, outlet: 'Media' }))
  const { calls } = await run({ ...ARGS, done: 'scout', sources: [...TEXT.slice(0, 3), ...MEDIA, ...TEXT.slice(3)] })
  const readers = calls.filter(c => c.label.startsWith('read:'))
  const sourcesOf = c => c.prompt.split('\n').filter(l => l.startsWith('- ')).map(l => l.slice(2).split(' ')[0])
  assert.deepEqual(readers.map(sourcesOf), [MEDIA.slice(0, 2), MEDIA.slice(2, 4), MEDIA.slice(4), TEXT.slice(0, 8), TEXT.slice(8)].map(b => b.map(s => s.url)))
  assert.deepEqual(readers.map(c => c.label), ['read:01', 'read:02', 'read:03', 'read:04', 'read:05'])
})

test('with the reading done, the run starts at the chapters: no scout, reader or numbers agent', async () => {
  const { calls } = await run({ ...ARGS, done: 'read' })
  assert.deepEqual([...labels(calls, 'scout:'), ...labels(calls, 'read:')], [])
  assert.equal(calls[0].label, 'merge:read')
  assert.deepEqual(labels(calls, 'write:').sort(), BODY.map(f => `write:${f}`))
  assert.deepEqual(labels(calls, 'book:'), ['book:01-and-10'])
})

test('an agent that dies is asked once more to pick up from its files; one that dies twice stops the run, naming it', async () => {
  const once = await run(ARGS, [], ['read:03', 'scout:criticism', 'write:05'])
  await once.result()
  for (const label of ['read:03', 'scout:criticism', 'write:05']) {
    const again = once.calls.find(c => c.label === `${label}:again`)
    assert.ok(again, `${label} is asked again`)
    assert.match(again.prompt, /pick up from the files/)
  }
  const twice = await run(ARGS, [], ['read:03', 'read:03:again'])
  await assert.rejects(twice.result(), /read-03/)
  assert.equal(labels(twice.calls, 'write:').length, 0, 'no chapter is written from a reading that lost a batch')
})
