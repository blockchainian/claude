// ABOUTME: Workflow script for a creator case study: scouts, readers, numbers agents, chapter writers,
// ABOUTME: adversarial reviewers and fixers, pipelined into the reviewed chapters of the book and its two framing chapters.
export const meta = {
  name: 'case-study-creator',
  description: 'Research one creator into a reviewed book: scouts, parallel readers, numbers, a writer per chapter, adversarial review and fixes per chapter, then the introduction and the reasoning chapter',
  phases: [
    { title: 'Scout', detail: 'four scouts find sources by lane; the two numbers agents start with them', model: 'sonnet' },
    { title: 'Read', detail: 'readers in batches of 8 sources', model: 'sonnet' },
    { title: 'Write', detail: 'one writer per chapter, as soon as the reading is merged: the text the reader gets, with its source marks', model: 'claude-opus-4-8' },
    { title: 'Review', detail: 'per chapter, as each is written: a script matches its figures, the quotes lens (Opus 4.8) reviews it, and the record lens (session model) judges the timeline and turning-point chapters' },
    { title: 'Check sources', detail: 'sources lens on Opus 4.8 over the sources the chapters name, 25 per reviewer, once every chapter is written', model: 'claude-opus-4-8' },
    { title: 'Fix', detail: 'one fixer per chapter, when its reviews and the sources lens are done, on gpt-6-luna through codex exec' },
    { title: 'Book', detail: 'one agent strips the marks from the fixed chapters and writes the introduction and the reasoning chapter from them' },
  ],
}

// args: { subject, work, skill, lang, today, product, seeds, caps, done, sources, fixer }
// skill is the absolute path of the case-study skill folder; product, seeds and
// caps may be empty strings. done names the stages whose files are already in the work directory and are not run
// again: 'scout' (the sources are known: args.sources, the output of `case-study.mjs sources <work>`, is read again
// without scouting), 'read' (the notes and the numbers: the run starts at the chapters), 'sources' (the sources lens's findings).
//
// A chapter is written once, as the text the reader gets, with the marks the checks need; the review and the fixes
// work on that text and on nothing the book does not print. The numbers agents start with the scouts, and each
// chapter runs write → figures matched + quotes review → fix on its own. The barriers are the merge after reading
// (the writers name sources by their labels in sources.json), every chapter written before the sources lens (it
// checks the sources the chapters name, and a fixer needs its findings), and every chapter fixed before the
// introduction and the reasoning chapter (written from them).
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

phase('Scout')
const LANES = ['own-words', 'press', 'business-and-people', 'analysts-and-critics']
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
const scouted = READ_DONE ? [] : SCOUT_DONE ? known() : (await parallel(LANES.map(lane => () => agent(
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
if (!READ_DONE) log(SCOUT_DONE ? `known sources: ${urls.length}, ${batches.length} reader batches` : `scouts: ${scouted.length}/${LANES.length} lanes, ${urls.length} distinct sources, ${batches.length} reader batches`)

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
// A writer's chapter is the text the reader gets: it runs on the model that reviews it. Eight at once on the
// session's model have run into the plan's quota.
const WRITER = { model: 'claude-opus-4-8', effort: 'high', agentType: 'general-purpose' }
const written = Object.fromEntries(FROM_NOTES.map(file => [file, agent(
  `${COMMON}\nYou are a chapter writer. Follow ${S}/briefs/write.md. Your chapter file: drafts/${file}.md (see the chapter table in the type file).`,
  { label: `write:${file}`, phase: 'Write', ...WRITER })]))
const allWritten = Promise.all(Object.values(written))

phase('Review')
// Sources and quotes are found-or-not checks; the record lens judges what the record supports, so it keeps
// the session model.
const review = (lens, name, slice) => agent(
  `${COMMON}\nYou are an independent adversarial reviewer. Follow ${S}/briefs/review.md. Your lens: ${lens}. Your output name: ${name}.\nYour slice:\n${slice}`,
  { label: `review:${name}`, phase: lens === 'sources' ? 'Check sources' : 'Review', effort: 'high', agentType: 'general-purpose', ...(lens === 'record' ? {} : { model: 'claude-opus-4-8' }) })
// Whether a figure is in its source is a lookup: a script does it for every chapter. Its unmatched figures go to
// the fixer as findings, except in the chapters that argue from the curve (the timeline, the turning points),
// where a record reviewer judges them first, with the derived figures and what each growth step is credited to.
const RECORD = ['03', '07']
const matchFigures = file => runs(`figures "${WORK}" ${file}${RECORD.includes(file) ? ' --worklist' : ''}`, `figures:${file}`, 'Review')

phase('Check sources')
// The sources lens checks the sources themselves, and only those a chapter names: a source nothing rests on is
// not vetted. So it starts when every chapter is written, beside the chapters' own reviews; a merge then takes
// the sources it failed out of sources.json, and the fixers remove or re-source the sentences that rested on them.
const sourcesReviewed = DONE.includes('sources') ? Promise.resolve([]) : allWritten.then(async () => {
  const counted = String(await runs(`cited "${WORK}"`, 'cited:sources', 'Check sources'))
  const sliceCount = Math.ceil((Number((counted.match(/"cited":\s*(\d+)/) || [])[1]) || sourceCount) / 25)
  return parallel(Array.from({ length: sliceCount }, (_, i) => () => review('sources', `sources-${i + 1}`,
    `the urls printed by: ${S}/scripts/case-study.mjs slice "${WORK}" ${i + 1} ${sliceCount}`)))
})
const sourcesMerged = DONE.includes('sources') ? Promise.resolve() : sourcesReviewed.then(() => merge('merge:sources'))

phase('Fix')
// The fixer is a bounded edit under listed findings; a Codex model does it as well as Sonnet for a fraction of the
// cost (pilot on brooke-monk chapter 04), so it runs there through the script, driven by a Haiku agent that only
// saves the prompt and runs the command. The brief makes an interrupted fixer resume, so a run cut off by the
// agent's command timeout, or one that stopped with findings left, is run again. args.fixer names a Claude model (sonnet, opus, haiku) to keep it here.
const FIXER = A.fixer || 'gpt-6-luna'
const fixerPrompt = file => `${COMMON}\nYou are a fixer. Follow ${S}/briefs/fix.md. Your chapter file: drafts/${file}.md (NN = ${file}).`
const fix = file => ['sonnet', 'opus', 'haiku'].includes(FIXER)
  ? agent(fixerPrompt(file), { label: `fix:${file}`, phase: 'Fix', ...SONNET, model: FIXER })
  : agent(
    `Save the text between the lines of === to ${WORK}/review/fix-${file}.prompt.txt, exactly, with the Write tool. Then run exactly this command with the longest timeout you can give it:\n${S}/scripts/case-study.mjs codex "${WORK}" ${file} --model ${FIXER}\nIts output ends with "remaining": the findings of the chapter the fixer has not reached. Run the command again when it times out or when "remaining" is above 0, four runs at most. Return the last run's output, nothing else.\n===\n${fixerPrompt(file)}\n===`,
    { label: `fix:${file}`, phase: 'Fix', model: 'haiku', effort: 'low', agentType: 'general-purpose' })
// One chapter's chain: written → figures matched and reviewed → fixed, once the sources lens is in.
const chapters = await Promise.all(FROM_NOTES.map(file => written[file].then(async () => {
  const reviews = await parallel([
    () => review('quotes', `quotes-${file}`, `- drafts/${file}.md`),
    async () => {
      const matched = await matchFigures(file)
      return RECORD.includes(file) ? review('record', `record-${file}`, `- drafts/${file}.md\nThe figures a script could not match: review/figures-${file}.md`) : matched
    },
  ])
  await sourcesMerged
  return { file, reviews, fixed: await fix(file) }
})))
const merged = await merge('merge:fix')

phase('Book')
// The introduction and the reasoning chapter rest on the fixed chapters and are written once, straight into the
// book: one agent, so it keeps the session's model. It first has a script strip the marks from the other chapters.
const ends = await agent(
  `${COMMON}\nYou write the introduction and the reasoning chapter of the book. Follow ${S}/briefs/book.md. Your files: ${FROM_CHAPTERS.map(file => `book/${file}.md`).join(' and ')}.${A.product ? `\nThe product for the reasoning chapter: ${A.product}` : ''}`,
  { label: `book:${FROM_CHAPTERS.join('-and-')}`, phase: 'Book', effort: 'high', agentType: 'general-purpose' })

return { scouted: urls.length, batches: batches.length, sources: (await sourcesReviewed).filter(Boolean), chapters, merged, ends }
