// ABOUTME: Runs the creator workflow script against stand-in agents to check its control flow:
// ABOUTME: scouts follow each other's leads, their sources are de-duplicated and read as they come in, every chapter is written, has its figures matched by a
// ABOUTME: script, is reviewed and is fixed as soon as its own reviews are done; no chapter is written before the sources lens is in.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = readFileSync(new URL('../workflows/creator.mjs', import.meta.url), 'utf8').replace('export const meta', 'const meta')
const SEEDS = JSON.parse(source.match(/const SEEDS = (\[[^\]]+\])/)[1].replaceAll("'", '"'))
// 13 seeds, a scout for each seed's own lead, one for the lead they share (which finds nothing new) and one for a lead's lead
const SCOUTS = 13 + 13 + 1 + 1
const SCOUTED = (SCOUTS - 1) * 9 + 1
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
        const name = opts.label.slice(6)
        // 10 sources per scout, one shared by every scout; a seed scout names two leads, one of them named by every seed.
        // The scout of the shared lead finds nothing new and names a lead; the scout of the first seed's own lead names one too.
        const shared = { url: 'https://shared.example/a/', outlet: 'Shared' }
        if (prompt.includes('Your lead: The 2019 lawsuit.')) return { sources: [shared], leads: ['the lawsuit\'s appeal'] }
        const first = prompt.includes(`Your lead: the people around ${SEEDS[0]}.`)
        return { sources: [shared, ...Array.from({ length: 9 }, (_, i) => ({ url: `https://${name}.example/${i}`, outlet: name, year: '2020' }))],
          leads: first ? ['her first editor'] : name.startsWith('lead-') ? [] : [`the people around ${name}`, 'The 2019 lawsuit'] }
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
  done.catch(() => {}) // a script that refuses its args rejects here; the test reads it through result()
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
  assert.equal((await result()).scouted, SCOUTED)
  assert.equal(labels(calls, 'read:').length, Math.ceil(SCOUTED / 8))
  assert.equal(calls.filter(c => c.prompt.includes('https://shared.example/a/')).filter(c => c.label.startsWith('read:')).length, 1)
  assert.deepEqual(labels(calls, 'numbers:'), ['numbers:archive', 'numbers:uploads'])
  assert.ok(calls.findIndex(c => c.label === 'numbers:archive') < calls.findIndex(c => c.label === 'read:01'))
})

test('every seed lead has its scout, is in the type file, and is told its name and the claim command', async () => {
  const { calls } = await run(ARGS)
  const type = readFileSync(new URL('../types/creator.md', import.meta.url), 'utf8')
  assert.equal(SEEDS.length, 13)
  for (const seed of SEEDS) {
    assert.ok(type.includes(`| ${seed} |`), `${seed} is in the type file's table`)
    const scout = calls.find(c => c.label === `scout:${seed}`)
    assert.ok(scout.prompt.includes(`Your scout name: ${seed}.`) && scout.prompt.includes(`/s/scripts/case-study.mjs claim "/w" ${seed} `), scout.prompt)
  }
})

test('every lead a scout returns gets its own scout, once, and so do the leads of that scout: no lead is left for want of scouts', async () => {
  const { calls, result } = await run(ARGS)
  const scouts = calls.filter(c => c.label.startsWith('scout:'))
  assert.equal(scouts.length, SCOUTS)
  const followers = scouts.filter(c => c.label.startsWith('scout:lead-'))
  assert.deepEqual(followers.map(c => c.label), Array.from({ length: SCOUTS - 13 }, (_, i) => `scout:lead-${String(i + 1).padStart(2, '0')}`))
  assert.equal(followers.filter(c => c.prompt.includes('Your lead: The 2019 lawsuit.')).length, 1, 'a lead named by every seed is searched once')
  assert.ok(followers[0].prompt.includes(`Your lead: the people around ${SEEDS[0]}.`))
  assert.ok(followers.at(-1).prompt.includes('Your lead: her first editor.'), 'a lead\'s own lead is followed')
  assert.ok(followers.at(-1).prompt.includes('The 2019 lawsuit'), 'a scout is told the leads other scouts have')
  const { leadsDry, leadsDropped } = await result()
  assert.deepEqual(leadsDry, ['the lawsuit\'s appeal'], 'a branch ends where a scout found no source that was new')
  assert.deepEqual(leadsDropped, [])
})

test('the limit on scouts is a guard against a search that never ends: the leads it leaves are reported', async () => {
  const { calls, result } = await run({ ...ARGS, maxScouts: 24 })
  assert.equal(labels(calls, 'scout:').length, 24)
  assert.deepEqual((await result()).leadsDropped, [...SEEDS.slice(-3).map(seed => `the people around ${seed}`), 'her first editor'])
  assert.equal((await run(ARGS)).calls.find(c => c.label === 'scout:interviews').prompt.includes('1/24 share'), true)
})

test('readers start on what a scout found as soon as it returns; only the merge waits for every scout', async () => {
  const { calls, release, result } = await run(ARGS, ['scout:press-at-the-time'])
  const early = labels(calls, 'read:').length
  assert.ok(early >= 20, `${early} readers started while a scout was still searching`)
  assert.ok(!calls.some(c => c.label === 'read:01' && c.prompt.includes('press-at-the-time.example')))
  assert.deepEqual(labels(calls, 'merge:'), [], 'the merge waits for the last scout and its readers')
  await release()
  assert.equal(labels(calls, 'read:').length, Math.ceil(SCOUTED / 8))
  assert.ok(labels(calls, 'merge:').includes('merge:read'))
  assert.equal((await result()).scouted, SCOUTED)
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
  // readers followed reposts to originals: 130 sources after the merge, whatever the scouts listed
  const sourceSlices = calls.filter(c => c.label.startsWith('review:sources-'))
  assert.equal(sourceSlices.length, 6)
  sourceSlices.forEach((c, i) => assert.ok(c.prompt.includes(`case-study.mjs slice "/w" ${i + 1} 6`), c.prompt))
  const quotes = calls.filter(c => c.label.startsWith('review:quotes-'))
  assert.deepEqual(quotes.map(c => c.prompt.match(/drafts\/\d\d\.md/g)).sort(), ALL.map(f => [`drafts/${f}.md`]))
  assert.deepEqual(labels(calls, 'fix:').sort(), ALL.map(f => `fix:${f}`))
  assert.deepEqual(labels(calls, 'merge:'), ['merge:read', 'merge:sources', 'merge:fix'])
})

test('the phases are listed in the order they start: the sources lens has its own phase, between reading and writing', async () => {
  const { calls } = await run(ARGS)
  const listed = [...source.matchAll(/\{ title: '([^']+)'/g)].map(m => m[1])
  const started = [...new Set(calls.map(c => c.phase).filter(Boolean))]
  assert.deepEqual(started, listed)
  assert.deepEqual(listed.slice(1, 4), ['Read', 'Check sources', 'Write'])
  assert.deepEqual([...new Set(calls.filter(c => c.label.startsWith('review:sources-')).map(c => c.phase))], ['Check sources'])
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
  assert.ok(calls.find(c => c.label === 'read:01').prompt.includes('1/26 share'), 'readers and the numbers agents share a stage')
  assert.ok(calls.find(c => c.label === 'scout:interviews').prompt.includes('1/24 share'))
})

test('with the scouting done, the known sources are read again, eight to a reader, and the numbers agents still run', async () => {
  const KNOWN = Array.from({ length: 20 }, (_, i) => ({ url: `https://known.example/${i}`, outlet: 'Known' }))
  const { calls, result } = await run({ ...ARGS, done: 'scout', sources: KNOWN })
  assert.deepEqual(labels(calls, 'scout:'), [])
  assert.equal(calls.filter(c => c.label.startsWith('sources:')).length, 0, 'no agent relays the list: an agent drops entries')
  await assert.rejects(async () => (await run({ ...ARGS, done: 'scout' })).result(), /sources/, 'the list is required')
  assert.equal(labels(calls, 'read:').length, Math.ceil(20 / 8))
  assert.ok(calls.find(c => c.label === 'read:03').prompt.includes('https://known.example/19 (Known)'))
  assert.deepEqual(labels(calls, 'numbers:'), ['numbers:archive', 'numbers:uploads'])
  assert.ok(calls.find(c => c.label === 'read:01').prompt.includes('1/5 share'), 'with the sources known, so is the number of readers')
  assert.equal((await result()).scouted, 20)
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
