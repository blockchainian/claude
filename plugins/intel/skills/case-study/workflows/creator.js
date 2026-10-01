export const meta = {
  name: 'case-study-creator',
  description: 'Research one creator into a reviewed sourced draft: scouts, parallel readers, numbers, chapter writers, sliced adversarial review, per-chapter fixes',
  phases: [
    { title: 'Scout', detail: 'four scouts find sources by lane', model: 'sonnet' },
    { title: 'Read', detail: 'readers in batches of 8 sources, plus two numbers agents', model: 'sonnet' },
    { title: 'Write', detail: 'one writer per chapter, then the introduction and reasoning chapters', model: 'sonnet' },
    { title: 'Review', detail: 'sources and quotes lenses on Opus, numbers lens on the session model, all in small slices' },
    { title: 'Fix', detail: 'one fixer per chapter', model: 'sonnet' },
  ],
}

// args: { subject, work, skill, lang, today, chapters, tools, product, seeds, caps }
// skill is the absolute path of the case-study skill folder; chapters is 11 or 12; tools, product, seeds and
// caps may be empty strings.
const A = args
const S = A.skill
const WORK = A.work
const pad = n => String(n).padStart(2, '0')
const LAST = pad(A.chapters)
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
await parallel([
  ...batches.map((batch, i) => () => agent(
    `${COMMON}\nYou are a reader. Follow ${S}/briefs/read.md. Your batch name: read-${pad(i + 1)}.${share(agentsReading)}\nYour sources:\n${batch.map(s => `- ${s.url} (${s.outlet}${s.year ? ' ' + s.year : ''}) ${s.why || ''}`).join('\n')}`,
    { label: `read:${pad(i + 1)}`, phase: 'Read', ...SONNET })),
  ...['archive', 'uploads'].map(lane => () => agent(
    `${COMMON}\nYou are a numbers agent. Follow ${S}/briefs/numbers.md. Your lane: ${lane}.${share(agentsReading)}`,
    { label: `numbers:${lane}`, phase: 'Read', ...SONNET })),
])
const mergedRead = String(await merge('merge:read'))
log(`read stage merged: ${mergedRead}`)
// readers follow reposts to originals, so the sources to review are counted after the merge
const sourceCount = Number((mergedRead.match(/"sources":\s*(\d+)/) || [])[1]) || urls.length

phase('Write')
const write = file => () => agent(
  `${COMMON}\nYou are a draft writer. Follow ${S}/briefs/write.md. Your chapter file: md/${file}.md (see the chapter table in the type file).${file === '11' ? `\nThe product for this chapter: ${A.product}` : ''}`,
  { label: `write:${file}`, phase: 'Write', ...SONNET })
await parallel(FROM_NOTES.map(write))
await parallel(FROM_CHAPTERS.map(write))

phase('Review')
const CHAPTERS = [...FROM_NOTES, ...FROM_CHAPTERS].sort()
const blocks = []
for (let i = 0; i < CHAPTERS.length; i += 2) blocks.push(CHAPTERS.slice(i, i + 2))
const sliceCount = Math.ceil(sourceCount / 25)
// Sources and quotes are found-or-not checks; the numbers lens judges what the record supports, so it keeps
// the session model.
const review = (lens, name, slice) => () => agent(
  `${COMMON}\nYou are an independent adversarial reviewer. Follow ${S}/briefs/review.md. Your lens: ${lens}. Your output name: ${name}.\nYour slice:\n${slice}`,
  { label: `review:${name}`, phase: 'Review', effort: 'high', agentType: 'general-purpose', ...(lens === 'numbers' ? {} : { model: 'opus' }) })
const reviews = (await parallel([
  ...Array.from({ length: sliceCount }, (_, i) => review('sources', `sources-${i + 1}`,
    `the urls printed by: ${S}/scripts/case_study.py slice "${WORK}" ${i + 1} ${sliceCount}`)),
  ...blocks.map((block, i) => review('numbers', `numbers-${i + 1}`, block.map(f => `- md/${f}.md`).join('\n'))),
  ...blocks.map((block, i) => review('quotes', `quotes-${i + 1}`, block.map(f => `- md/${f}.md`).join('\n'))),
])).filter(Boolean)

phase('Fix')
const fix = file => () => agent(
  `${COMMON}\nYou are a fixer. Follow ${S}/briefs/fix.md. Your chapter file: md/${file}.md (NN = ${file}).`,
  { label: `fix:${file}`, phase: 'Fix', ...SONNET })
const fixedFirst = await parallel(FROM_NOTES.map(fix))
const fixedLast = await parallel(FROM_CHAPTERS.map(fix))
const merged = await merge('merge:fix')

return { scouted: urls.length, batches: batches.length, sourcesChapter: LAST, reviews, fixes: [...fixedFirst, ...fixedLast], merged }
