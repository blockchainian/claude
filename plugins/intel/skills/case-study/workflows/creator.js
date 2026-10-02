export const meta = {
  name: 'case-study-creator',
  description: 'Research one creator into a reviewed sourced draft: scouts, parallel readers, numbers, chapter writers, adversarial review and fixes per chapter, pipelined',
  phases: [
    { title: 'Scout', detail: 'four scouts find sources by lane; the two numbers agents start with them', model: 'sonnet' },
    { title: 'Read', detail: 'readers in batches of 8 sources', model: 'sonnet' },
    { title: 'Write', detail: 'one writer per chapter, then the introduction and reasoning chapters', model: 'sonnet' },
    { title: 'Review', detail: 'sources lens on Opus from the merge on; per chapter, as each is written: a script matches its figures, the quotes lens (Opus) reviews it, and the record lens (session model) judges the timeline and turning-point chapters' },
    { title: 'Fix', detail: 'one fixer per chapter, as soon as its reviews and the sources lens are done', model: 'sonnet' },
  ],
}

// args: { subject, work, skill, lang, today, chapters, tools, product, seeds, caps }
// skill is the absolute path of the case-study skill folder; chapters is 11 or 12; tools, product, seeds and
// caps may be empty strings.
//
// Nothing waits for a stage it does not need: the numbers agents start with the scouts, the sources lens starts
// with the writers, and each chapter runs write → figures matched + quotes review → fix on its own. The only barriers are
// the merge after reading (writers and the sources lens need sources.json), the body chapters before the
// introduction and reasoning chapters (written from them, fixed after them), and the sources lens before any fix.
const A = args
const S = A.skill
const WORK = A.work
const pad = n => String(n).padStart(2, '0')
const REASONING = A.chapters === 12 ? ['10', '11'] : ['10']
const FROM_NOTES = ['02', '03', '04', '05', '06', '07', '08', '09']
const FROM_CHAPTERS = ['01', ...REASONING]

const COMMON = `Subject: ${A.subject}. Today is ${A.today}. Language of the study: ${A.lang}.
Work directory: ${WORK}
Evidence rules: ${S}/references/evidence.md
Tools: ${S}/references/tools.md${A.tools ? `; commands tested on this machine, preferred: ${A.tools}` : ''}
Subject type file: ${S}/types/creator.md
Read-only on every platform: never post, comment, like or follow. Waiting commands run in the foreground with a bounded time.`
const share = n => A.caps ? `\nMachine caps for this whole stage: ${A.caps}. You are one of ${n} agents in it: use at most a 1/${n} share.` : ''
const SONNET = { model: 'sonnet', effort: 'high', agentType: 'general-purpose' }
const merge = label => agent(`Run exactly this command and return its output, nothing else:\n${S}/scripts/case_study.py merge "${WORK}"`,
  { label, model: 'haiku', effort: 'low', agentType: 'general-purpose' })

phase('Scout')
const LANES = ['own-words', 'press', 'business-and-people', 'criticism-and-data']
const FOUND = { type: 'object', required: ['sources'], properties: { sources: { type: 'array', items: { type: 'object', required: ['url', 'outlet'],
  properties: { url: { type: 'string' }, outlet: { type: 'string' }, year: { type: 'string' }, kind: { type: 'string' }, why: { type: 'string' } } } } } }
// The numbers agents need no scout: the archive and the upload record are the profile's own addresses.
const READERS_EXPECTED = 15 // for the caps share before the scouts return; readers get their exact count
const numbersDone = parallel(['archive', 'uploads'].map(lane => () => agent(
  `${COMMON}\nYou are a numbers agent. Follow ${S}/briefs/numbers.md. Your lane: ${lane}.${share(READERS_EXPECTED + 2)}`,
  { label: `numbers:${lane}`, phase: 'Scout', ...SONNET })))
const scouted = (await parallel(LANES.map(lane => () => agent(
  `${COMMON}\nYou are a scout. Follow ${S}/briefs/scout.md. Your lane: ${lane} (see "Scout lanes" in the type file).${share(LANES.length)}${A.seeds ? `\nKnown starting sources: ${A.seeds}` : ''}`,
  { label: `scout:${lane}`, phase: 'Scout', schema: FOUND, ...SONNET })))).filter(Boolean)
const seen = new Set()
const urls = []
for (const s of scouted.flatMap(r => r.sources)) {
  const key = s.url.split('#')[0].replace(/[?&]utm_[^&]*/g, '').replace(/\/$/, '')
  if (!seen.has(key)) { seen.add(key); urls.push(s) }
}
const batches = []
for (let i = 0; i < urls.length; i += 8) batches.push(urls.slice(i, i + 8))
log(`scouts: ${scouted.length}/${LANES.length} lanes, ${urls.length} distinct sources, ${batches.length} reader batches`)

phase('Read')
const agentsReading = batches.length + 2
await parallel(batches.map((batch, i) => () => agent(
  `${COMMON}\nYou are a reader. Follow ${S}/briefs/read.md. Your batch name: read-${pad(i + 1)}.${share(agentsReading)}\nYour sources:\n${batch.map(s => `- ${s.url} (${s.outlet}${s.year ? ' ' + s.year : ''}) ${s.why || ''}`).join('\n')}`,
  { label: `read:${pad(i + 1)}`, phase: 'Read', ...SONNET })))
await numbersDone
const mergedRead = String(await merge('merge:read'))
log(`read stage merged: ${mergedRead}`)
// readers follow reposts to originals, so the sources to review are counted after the merge
const sourceCount = Number((mergedRead.match(/"sources":\s*(\d+)/) || [])[1]) || urls.length

phase('Write')
// Sources and quotes are found-or-not checks; the record lens judges what the record supports, so it keeps
// the session model.
const review = (lens, name, slice) => agent(
  `${COMMON}\nYou are an independent adversarial reviewer. Follow ${S}/briefs/review.md. Your lens: ${lens}. Your output name: ${name}.\nYour slice:\n${slice}`,
  { label: `review:${name}`, phase: 'Review', effort: 'high', agentType: 'general-purpose', ...(lens === 'record' ? {} : { model: 'opus' }) })
// Whether a figure is in its source is a lookup: a script does it for every chapter. Its unmatched figures go to
// the fixer as findings, except in the chapters that argue from the curve (the timeline, the turning points),
// where a record reviewer judges them first, with the derived figures and what each growth step is credited to.
const RECORD = ['03', '07']
const matchFigures = file => agent(
  `Run exactly this command and return its output, nothing else:\n${S}/scripts/case_study.py figures "${WORK}" ${file}${RECORD.includes(file) ? ' --worklist' : ''}`,
  { label: `figures:${file}`, phase: 'Review', model: 'haiku', effort: 'low', agentType: 'general-purpose' })
// The sources lens checks the sources themselves, not the chapters: it runs while the chapters are written.
const sliceCount = Math.ceil(sourceCount / 25)
const sourcesReviewed = parallel(Array.from({ length: sliceCount }, (_, i) => () => review('sources', `sources-${i + 1}`,
  `the urls printed by: ${S}/scripts/case_study.py slice "${WORK}" ${i + 1} ${sliceCount}`)))

const write = file => agent(
  `${COMMON}\nYou are a draft writer. Follow ${S}/briefs/write.md. Your chapter file: md/${file}.md (see the chapter table in the type file).${file === '11' ? `\nThe product for this chapter: ${A.product}` : ''}`,
  { label: `write:${file}`, phase: 'Write', ...SONNET })
const fix = file => agent(
  `${COMMON}\nYou are a fixer. Follow ${S}/briefs/fix.md. Your chapter file: md/${file}.md (NN = ${file}).`,
  { label: `fix:${file}`, phase: 'Fix', ...SONNET })
// One chapter's chain: written → figures matched and reviewed → fixed once the sources lens is in.
const written = {}
const chain = (file, writeAfter, fixAfter) => {
  written[file] = writeAfter.then(() => write(file))
  return written[file].then(async () => {
    const reviews = await parallel([
      () => review('quotes', `quotes-${file}`, `- md/${file}.md`),
      async () => {
        const matched = await matchFigures(file)
        return RECORD.includes(file) ? review('record', `record-${file}`, `- md/${file}.md\nThe figures a script could not match: review/figures-${file}.md`) : matched
      },
    ])
    await sourcesReviewed
    await fixAfter
    return { file, reviews, fixed: await fix(file) }
  })
}
const bodyChains = FROM_NOTES.map(file => chain(file, Promise.resolve(), Promise.resolve()))
const bodyWritten = Promise.all(FROM_NOTES.map(file => written[file]))
const bodyFixed = Promise.all(bodyChains)
const tailChains = FROM_CHAPTERS.map(file => chain(file, bodyWritten, bodyFixed))
const chapters = await Promise.all([...bodyChains, ...tailChains])
const merged = await merge('merge:fix')

return { scouted: urls.length, batches: batches.length, sources: (await sourcesReviewed).filter(Boolean), chapters, merged }
