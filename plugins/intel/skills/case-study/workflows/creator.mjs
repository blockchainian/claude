// ABOUTME: Workflow script for a creator case study: scouts, readers, numbers agents, chapter writers,
// ABOUTME: adversarial reviewers and fixers, pipelined into one reviewed, sourced draft.
export const meta = {
  name: 'case-study-creator',
  description: 'Research one creator into a reviewed sourced draft: scouts that follow each other\'s leads, parallel readers, numbers, chapter writers, adversarial review and fixes per chapter, pipelined',
  phases: [
    { title: 'Scout', detail: 'one scout per lead, a few minutes each; every lead they return gets a scout of its own, until none returns a lead; the two numbers agents start with them', model: 'sonnet' },
    { title: 'Read', detail: 'readers in batches of 8 sources, started as the scouts return', model: 'sonnet' },
    { title: 'Check sources', detail: 'sources lens on Opus, 25 sources per reviewer, after the merge and before any chapter is written', model: 'opus' },
    { title: 'Write', detail: 'one writer per chapter, then the introduction and the reasoning chapter', model: 'sonnet' },
    { title: 'Review', detail: 'per chapter, as each is written: a script matches its figures, the quotes lens (Opus) reviews it, and the record lens (session model) judges the timeline and turning-point chapters' },
    { title: 'Fix', detail: 'one fixer per chapter, as soon as its reviews are done, on gpt-6-luna through codex exec' },
  ],
}

// args: { subject, work, skill, lang, today, product, seeds, caps, done, sources, fixer, maxScouts }
// skill is the absolute path of the case-study skill folder; product, seeds and
// caps may be empty strings. done names the stages whose files are already in the work directory and are not run
// again: 'scout' (the sources are known: args.sources, the output of `case-study.mjs sources <work>`, is read again
// without scouting), 'read' (the notes and the numbers: the run starts at the draft), 'sources' (the sources lens's findings).
//
// The numbers agents start with the scouts, readers start on a scout's sources when it returns, and each chapter
// runs write → figures matched + quotes review → fix on its own. The barriers are the merge after reading (the sources lens needs sources.json), the sources lens before
// any writer (a failed source's notes are not written up, reviewed and then removed), and the body chapters before
// the introduction and the reasoning chapter (written from them, fixed after them).
const A = args
const S = A.skill
const WORK = A.work
const pad = n => String(n).padStart(2, '0')
const FROM_NOTES = ['02', '03', '04', '05', '06', '07', '08', '09']
const FROM_CHAPTERS = ['01', '10']
const DONE = String(A.done || '').split(/[,\s]+/)
const READ_DONE = DONE.includes('read')
const SCOUT_DONE = READ_DONE || DONE.includes('scout')

const COMMON = `Subject: ${A.subject}. Today is ${A.today}. Language of the study: ${A.lang}.
Work directory: ${WORK}
Evidence rules: ${S}/references/evidence.md
Tools: ${S}/references/tools.md
Subject type file: ${S}/types/creator.md
Read-only on every platform: never post, comment, like or follow. Waiting commands run in the foreground with a bounded time.`
const share = n => A.caps ? `\nMachine caps for this whole stage: ${A.caps}. You are one of ${n} agents in it: use at most a 1/${n} share.` : ''
const SONNET = { model: 'sonnet', effort: 'high', agentType: 'general-purpose' }
const merge = label => agent(`Run exactly this command and return its output, nothing else:\n${S}/scripts/case-study.mjs merge "${WORK}"`,
  { label, model: 'haiku', effort: 'low', agentType: 'general-purpose' })

phase('Scout')
// A scout is a few minutes of work: one lead, a few searches, a dozen sources at most. Nothing is searched less
// for it: a scout returns what its lead still holds and every other lead it saw, each gets a scout of its own,
// and the scouting ends when no scout returns a lead. The seed leads are the rows of "Scout leads" in the type file.
const SEEDS = ['interviews', 'own-posts', 'documents', 'press-at-the-time', 'press-home', 'trade-press', 'later-profiles', 'books-and-films', 'people', 'filings', 'data-and-research', 'criticism', 'today']
const SCOUTS_EXPECTED = 24 // for the caps share: how many there will be is not known when the first ones start
// A guard against a search that never ends, far above what a subject needs; args.maxScouts sets another.
const MAX_SCOUTS = A.maxScouts || 150
const FOUND = { type: 'object', required: ['sources'], properties: {
  sources: { type: 'array', items: { type: 'object', required: ['url', 'outlet'],
    properties: { url: { type: 'string' }, outlet: { type: 'string' }, year: { type: 'string' }, kind: { type: 'string' }, why: { type: 'string' } } } },
  leads: { type: 'array', items: { type: 'string' } } } }
// With the scouting done, the list to read comes in as args.sources (the output of `case-study.mjs sources <work>`):
// a script cannot be read from here, and an agent relaying 366 entries dropped 65 of them.
if (SCOUT_DONE && !READ_DONE && !(Array.isArray(A.sources) && A.sources.length)) throw new Error("done 'scout' needs args.sources: the output of case-study.mjs sources <work>")
// Readers start before the scouts are done, so their number is a guess unless the sources are known.
const READERS_EXPECTED = SCOUT_DONE && !READ_DONE ? Math.ceil(A.sources.length / 8) : 24
// The numbers agents need no scout: the archive and the upload record are the profile's own addresses.
const numbersDone = READ_DONE ? null : parallel(['archive', 'uploads'].map(lane => () => agent(
  `${COMMON}\nYou are a numbers agent. Follow ${S}/briefs/numbers.md. Your lane: ${lane}.${share(READERS_EXPECTED + 2)}`,
  { label: `numbers:${lane}`, phase: 'Scout', ...SONNET })))

// Sources go to a reader eight at a time, as soon as there are eight no reader has: no reader waits for the
// slowest scout. Two scouts can list one source under two spellings of its address; it is read once.
const seen = new Set()
const urls = []
const unread = []
const readers = []
const read = batch => {
  const name = `read-${pad(readers.length + 1)}`
  readers.push(agent(
    `${COMMON}\nYou are a reader. Follow ${S}/briefs/read.md. Your batch name: ${name}.${share(READERS_EXPECTED + 2)}\nYour sources:\n${batch.map(s => `- ${s.url} (${s.outlet}${s.year ? ' ' + s.year : ''}) ${s.why || ''}`).join('\n')}`,
    { label: name.replace('-', ':'), phase: 'Read', ...SONNET }))
}
const found = sources => {
  const before = urls.length
  for (const s of sources) {
    const key = s.url.split('#')[0].replace(/[?&]utm_[^&]*/g, '').replace(/\/$/, '')
    if (seen.has(key)) continue
    seen.add(key)
    urls.push(s)
    unread.push(s)
  }
  while (unread.length >= 8) read(unread.splice(0, 8))
  return urls.length - before
}

// A lead is searched once: two scouts that name it in the same words get one scout. A branch ends where a scout
// found no source that was new: its leads are not followed. The run reports those, and the leads the guard left.
const leadKey = lead => lead.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim()
const given = new Map(SEEDS.map(seed => [seed, seed]))
const leadsDropped = []
const leadsDry = []
let scouts = 0
const scout = (name, lead) => {
  const others = [...given.values()].filter(other => other !== lead)
  return agent(
    `${COMMON}\nYou are a scout. Follow ${S}/briefs/scout.md. Your scout name: ${name}. Your lead: ${lead}${SEEDS.includes(lead) ? ' (see "Scout leads" in the type file)' : ''}.${share(SCOUTS_EXPECTED)}` +
    `\nClaim a source before you open it: ${S}/scripts/case-study.mjs claim "${WORK}" ${name} <url>...` +
    `\nLeads other scouts have, which are not yours to search or to return: ${others.join('; ')}${A.seeds ? `\nKnown starting sources: ${A.seeds}` : ''}`,
    { label: `scout:${name}`, phase: 'Scout', schema: FOUND, ...SONNET }).then(result => {
    if (!result) return
    const fresh = found(result.sources)
    const dry = fresh === 0 && !SEEDS.includes(lead)
    const followers = []
    for (const next of result.leads || []) {
      if (given.has(leadKey(next))) continue
      if (dry) { leadsDry.push(next); continue }
      if (scouts >= MAX_SCOUTS) { leadsDropped.push(next); continue }
      given.set(leadKey(next), next)
      scouts += 1
      followers.push({ name: `lead-${pad(scouts - SEEDS.length)}`, lead: next })
    }
    return parallel(followers.map(f => () => scout(f.name, f.lead)))
  })
}
if (SCOUT_DONE && !READ_DONE) found(A.sources)
else if (!READ_DONE) {
  scouts = SEEDS.length
  await parallel(SEEDS.map(seed => () => scout(seed, seed)))
}
if (unread.length) read(unread.splice(0))
if (!READ_DONE) log(SCOUT_DONE ? `known sources: ${urls.length}, ${readers.length} reader batches`
  : `scouts: ${scouts}, ${urls.length} distinct sources, ${readers.length} reader batches${leadsDry.length ? `; ${leadsDry.length} leads of scouts that found nothing new, not followed: ${leadsDry.join('; ')}` : ''}${leadsDropped.length ? `; STOPPED AT THE GUARD of ${MAX_SCOUTS} scouts, ${leadsDropped.length} leads not followed: ${leadsDropped.join('; ')}` : ''}`)

phase('Read')
await Promise.all(readers)
await numbersDone
const mergedRead = String(await merge('merge:read'))
log(`read stage merged: ${mergedRead}`)
// readers follow reposts to originals, so the sources to review are counted after the merge
const sourceCount = Number((mergedRead.match(/"sources":\s*(\d+)/) || [])[1]) || urls.length

phase('Check sources')
// Sources and quotes are found-or-not checks; the record lens judges what the record supports, so it keeps
// the session model.
const review = (lens, name, slice) => agent(
  `${COMMON}\nYou are an independent adversarial reviewer. Follow ${S}/briefs/review.md. Your lens: ${lens}. Your output name: ${name}.\nYour slice:\n${slice}`,
  { label: `review:${name}`, phase: lens === 'sources' ? 'Check sources' : 'Review', effort: 'high', agentType: 'general-purpose', ...(lens === 'record' ? {} : { model: 'opus' }) })
// Whether a figure is in its source is a lookup: a script does it for every chapter. Its unmatched figures go to
// the fixer as findings, except in the chapters that argue from the curve (the timeline, the turning points),
// where a record reviewer judges them first, with the derived figures and what each growth step is credited to.
const RECORD = ['03', '07']
const matchFigures = file => agent(
  `Run exactly this command and return its output, nothing else:\n${S}/scripts/case-study.mjs figures "${WORK}" ${file}${RECORD.includes(file) ? ' --worklist' : ''}`,
  { label: `figures:${file}`, phase: 'Review', model: 'haiku', effort: 'low', agentType: 'general-purpose' })
// The sources lens checks the sources themselves, not the chapters, and runs before them: a merge then takes the
// sources it failed out of sources.json, and the writers get no notes from them.
const sliceCount = Math.ceil(sourceCount / 25)
const sourcesReviewed = DONE.includes('sources') ? Promise.resolve([]) : parallel(Array.from({ length: sliceCount }, (_, i) => () => review('sources', `sources-${i + 1}`,
  `the urls printed by: ${S}/scripts/case-study.mjs slice "${WORK}" ${i + 1} ${sliceCount}`)))
const sourcesMerged = DONE.includes('sources') ? Promise.resolve() : sourcesReviewed.then(() => merge('merge:sources'))

phase('Write')
const write = file => agent(
  `${COMMON}\nYou are a draft writer. Follow ${S}/briefs/write.md. Your chapter file: drafts/${file}.md (see the chapter table in the type file).${file === '10' && A.product ? `\nThe product for this chapter: ${A.product}` : ''}`,
  { label: `write:${file}`, phase: 'Write', ...SONNET })
// The fixer is a bounded edit under listed findings; a Codex model does it as well as Sonnet for a fraction of the
// cost (pilot on brooke-monk chapter 04), so it runs there through the script, driven by a Haiku agent that only
// saves the prompt and runs the command. The brief makes an interrupted fixer resume, so a run cut off by the
// agent's command timeout is run again. args.fixer names a Claude model (sonnet, opus, haiku) to keep it here.
const FIXER = A.fixer || 'gpt-6-luna'
const fixerPrompt = file => `${COMMON}\nYou are a fixer. Follow ${S}/briefs/fix.md. Your chapter file: drafts/${file}.md (NN = ${file}).`
const fix = file => ['sonnet', 'opus', 'haiku'].includes(FIXER)
  ? agent(fixerPrompt(file), { label: `fix:${file}`, phase: 'Fix', ...SONNET, model: FIXER })
  : agent(
    `Save the text between the lines of === to ${WORK}/review/fix-${file}.prompt.txt, exactly, with the Write tool. Then run exactly this command with the longest timeout you can give it, and run it again if it times out, until it exits on its own:\n${S}/scripts/case-study.mjs codex "${WORK}" ${file} --model ${FIXER}\nReturn its output, nothing else.\n===\n${fixerPrompt(file)}\n===`,
    { label: `fix:${file}`, phase: 'Fix', model: 'haiku', effort: 'low', agentType: 'general-purpose' })
// One chapter's chain: written → figures matched and reviewed → fixed.
const written = {}
const chain = (file, writeAfter, fixAfter) => {
  written[file] = writeAfter.then(() => write(file))
  return written[file].then(async () => {
    const reviews = await parallel([
      () => review('quotes', `quotes-${file}`, `- drafts/${file}.md`),
      async () => {
        const matched = await matchFigures(file)
        return RECORD.includes(file) ? review('record', `record-${file}`, `- drafts/${file}.md\nThe figures a script could not match: review/figures-${file}.md`) : matched
      },
    ])
    await fixAfter
    return { file, reviews, fixed: await fix(file) }
  })
}
const bodyChains = FROM_NOTES.map(file => chain(file, sourcesMerged, Promise.resolve()))
const bodyWritten = Promise.all(FROM_NOTES.map(file => written[file]))
const bodyFixed = Promise.all(bodyChains)
const tailChains = FROM_CHAPTERS.map(file => chain(file, bodyWritten, bodyFixed))
const chapters = await Promise.all([...bodyChains, ...tailChains])
const merged = await merge('merge:fix')

return { scouted: urls.length, scouts, leadsDry, leadsDropped, batches: readers.length, sources: (await sourcesReviewed).filter(Boolean), chapters, merged }
