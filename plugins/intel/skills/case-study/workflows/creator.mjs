// ABOUTME: Workflow script for a creator case study: scouts over the news lists fetched before it, readers, numbers agents and chapter writers,
// ABOUTME: then the book's two framing chapters written from the others.
export const meta = {
  name: 'case-study-creator',
  description: 'Research one creator into a book: scouts, parallel readers, numbers, a writer per chapter, then the introduction and the reasoning chapter',
  phases: [
    { title: 'Scout', detail: 'one scout per source type, picking press from the news lists fetched before the workflow; the two numbers agents start at once', model: 'sonnet' },
    { title: 'Read', detail: 'readers in batches of 8 sources, 2 for videos and podcast episodes, which start first', model: 'sonnet' },
    { title: 'Write', detail: 'one writer per chapter, as soon as the reading is merged: the text the reader gets, with its source marks', model: 'claude-opus-5-5' },
    { title: 'Book', detail: 'one agent strips the marks from the chapters and writes the introduction and the reasoning chapter from them' },
  ],
}

// args: { subject, work, skill, lang, today, product, seeds, caps, done, sources, since }
// skill is the absolute path of the case-study skill folder; product, seeds and
// caps may be empty strings. since is the first year of the subject's growth (YYYY), which the scouts search every year from. The news lists are in <work>/raw/news/ before the workflow starts: the orchestrator fetches
// them with `case-study.mjs news` and re-runs a list that failed, which a cached agent here could not. done names the stages whose files are already in the work directory and are not run
// again: 'scout' (the sources are known: args.sources, the output of `case-study.mjs sources <work>`, is read again
// without scouting), 'read' (the notes and the numbers: the run starts at the chapters).
//
// A chapter is written once, as the text the reader gets, with its source marks. The numbers agents start with the
// scouts. The barriers are the merge after reading (the writers name sources by their labels in sources.json) and
// every chapter written before the introduction and the reasoning chapter (written from them).
const A = args
const S = A.skill
const WORK = A.work
const pad = n => String(n).padStart(2, '0')
const FROM_NOTES = ['02', '03', '04', '05', '06', '07', '08', '09']
const FROM_CHAPTERS = ['01', '10'] // the introduction and the reasoning chapter
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
// A script's command, run by the smallest model, which only relays its output.
const runs = (command, label, phaseName) => agent(`Run exactly this command and return its output, nothing else:\n${S}/scripts/case-study.mjs ${command}`,
  { label, phase: phaseName, model: 'haiku', effort: 'low', agentType: 'general-purpose' })
const merge = label => runs(`merge "${WORK}"`, label)
// An agent that died (its account's limit, a crash) returns nothing: it is asked once more, to pick up from the files
// it left. One that dies twice stops the run, named, before anything is built on the part it lost.
const settled = (prompt, opts) => agent(prompt, opts).then(done => done ?? agent(`${prompt}\nA first attempt at this stopped before it finished: pick up from the files it left.`, { ...opts, label: `${opts.label}:again` }))
const allSettled = (results, names, stage) => {
  const lost = results.map((done, i) => (done == null ? names[i] : null)).filter(Boolean)
  if (lost.length) throw Error(`${stage}: ${lost.join(', ')} stopped twice; relaunch the run from its files (see "Rules for the orchestrator")`)
  return results
}

phase('Scout')
// One scout per source type (see "Scout lanes" in the type file): a scout with one kind of source to find needs few turns.
const LANES = ['interviews', 'own-explainers', 'internal-documents', 'press-at-the-time', 'trade-and-profiles', 'books-and-films', 'people', 'records', 'data-and-today', 'criticism']
const FOUND = { type: 'object', required: ['sources'], properties: { sources: { type: 'array', items: { type: 'object', required: ['url', 'outlet'],
  properties: { url: { type: 'string' }, outlet: { type: 'string' }, year: { type: 'string' }, kind: { type: 'string' }, why: { type: 'string' } } } } } }
// The numbers agents need no scout: the archive and the upload record are the profile's own addresses.
const READERS_EXPECTED = 15 // for the caps share before the scouts return; readers get their exact count
const numbersDone = READ_DONE ? null : parallel(['follower-numbers', 'posting-numbers'].map(lane => () => agent(
  `${COMMON}\nYou are a numbers agent. Follow ${S}/briefs/numbers.md. Your lane: ${lane}.${share(READERS_EXPECTED + 2)}`,
  { label: `scout:${lane}`, phase: 'Scout', ...SONNET })))
// With the scouting done, the list to read comes in as args.sources (the output of `case-study.mjs sources <work>`):
// a script cannot be read from here, and an agent relaying 366 entries dropped 65 of them.
const known = () => {
  if (!Array.isArray(A.sources) || !A.sources.length) throw new Error("done 'scout' needs args.sources: the output of case-study.mjs sources <work>")
  return [{ sources: A.sources }]
}
const scouted = READ_DONE ? [] : SCOUT_DONE ? known() : allSettled(await parallel(LANES.map(lane => () => settled(
  `${COMMON}\nYou are a scout. Follow ${S}/briefs/scout.md. Your lane: ${lane} (see "Scout lanes" in the type file).${A.since ? ` First year of growth: ${A.since}.` : ''}${share(LANES.length + 2)}${A.seeds ? `\nKnown starting sources: ${A.seeds}` : ''}\nThe news lists are fetched: ${WORK}/raw/news/ (gnews-<name>.jsonl for each name, gdelt.jsonl), one JSON article per line.`,
  { label: `scout:${lane}`, phase: 'Scout', schema: FOUND, ...SONNET }))), LANES.map(lane => `scout ${lane}`), 'Scout')
const seen = new Set()
const urls = []
for (const s of scouted.flatMap(r => r.sources)) {
  const key = s.url.split('#')[0].replace(/[?&]utm_[^&]*/g, '').replace(/\/$/, '')
  if (!seen.has(key)) { seen.add(key); urls.push(s) }
}
// A video or a podcast episode is often an hour to download, transcribe and read: two go to a reader, and their
// batches come first, so the long ones start in the first wave of readers instead of waiting for a free one.
const MEDIA = /^https?:\/\/((www|m)\.)?(youtube\.com\/(watch|live\/)|youtu\.be\/|spreaker\.com\/episode|podcasts\.apple\.com\/|open\.spotify\.com\/episode)/
const slices = (list, size) => Array.from({ length: Math.ceil(list.length / size) }, (_, i) => list.slice(i * size, (i + 1) * size))
const batches = [...slices(urls.filter(s => MEDIA.test(s.url)), 2), ...slices(urls.filter(s => !MEDIA.test(s.url)), 8)]
if (!READ_DONE) log(SCOUT_DONE ? `known sources: ${urls.length}, ${batches.length} reader batches` : `scouts: ${scouted.length}/${LANES.length} lanes, ${urls.length} distinct sources, ${batches.length} reader batches`)

phase('Read')
const agentsReading = batches.length + 2
allSettled(await parallel(batches.map((batch, i) => () => settled(
  `${COMMON}\nYou are a reader. Follow ${S}/briefs/read.md. Your batch name: read-${pad(i + 1)}.${share(agentsReading)}\nYour sources:\n${batch.map(s => `- ${s.url} (${s.outlet}${s.year ? ' ' + s.year : ''}) ${s.why || ''}`).join('\n')}`,
  { label: `read:${pad(i + 1)}`, phase: 'Read', ...SONNET }))), batches.map((_, i) => `read-${pad(i + 1)}`), 'Read')
await numbersDone
const mergedRead = String(await merge('merge:read'))
log(`read stage merged: ${mergedRead}`)

phase('Write')
// A writer's chapter is the text the reader gets. Eight at once on the session's model have run into the plan's quota.
const WRITER = { model: 'claude-opus-5-5', effort: 'high', agentType: 'general-purpose' }
allSettled(await Promise.all(FROM_NOTES.map(file => settled(
  `${COMMON}\nYou are a chapter writer. Follow ${S}/briefs/write.md. Your chapter file: drafts/${file}.md (see the chapter table in the type file).`,
  { label: `write:${file}`, phase: 'Write', ...WRITER }))), FROM_NOTES.map(file => `drafts/${file}.md`), 'Write')

phase('Book')
// The introduction and the reasoning chapter rest on the other chapters and are written once, straight into the
// book: one agent, so it keeps the session's model. It first has a script strip the marks from the other chapters.
const ends = await agent(
  `${COMMON}\nYou write the introduction and the reasoning chapter of the book. Follow ${S}/briefs/book.md. Your files: ${FROM_CHAPTERS.map(file => `book/${file}.md`).join(' and ')}.${A.product ? `\nThe product for the reasoning chapter: ${A.product}` : ''}`,
  { label: `book:${FROM_CHAPTERS.join('-and-')}`, phase: 'Book', effort: 'high', agentType: 'general-purpose' })

return { scouted: urls.length, batches: batches.length, ends }
