// ABOUTME: Tests the case-study work-dir scaffold and the pre-render check.
// ABOUTME: Covers init (layout, cover, idempotence), check (sourced draft, book text, sources, counts), merge, slice, findings, figures, quotes, unread and bullets.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, test } from 'node:test'
import { fileURLToPath } from 'node:url'

const tmp = mkdtempSync(join(tmpdir(), 'case-study-'))
process.env.CASE_STUDIES_DIR = join(tmp, 'store')
const cs = await import('../scripts/case-study.mjs')

const out = join(tmp, 'pdf', 'jane-doe.pdf')
const read = (...parts) => readFileSync(join(...parts), 'utf8')
const write = (path, text) => writeFileSync(path, text, 'utf8')
const json = (...parts) => JSON.parse(read(...parts))
const chapters = report => report.map(c => c.chapter)
const pad = n => String(n).padStart(2, '0')
let work

before(() => {
  work = cs.init('jane-doe', 'How Jane Doe grew', 'https://example.com/@jane', out, 12, 'Jane Doe').work
})
after(() => rmSync(tmp, { recursive: true, force: true }))

test('init scaffolds the work dir under the store and keeps an existing sources.json', () => {
  assert.equal(work, join(tmp, 'store', 'jane-doe'))
  assert.ok(['drafts', 'book', 'notes', 'raw', 'review'].every(d => statSync(join(work, d)).isDirectory()))
  assert.deepEqual(json(work, 'sources.json'), {})
  const saved = json(work, 'chapters.json')
  assert.deepEqual(saved.chapters.map(c => c.id), Array.from({ length: 12 }, (_, n) => pad(n + 1)))
  assert.equal(saved.chapters[2].highlights, 'book/03.md', 'the typeset chapters are the book text')
  assert.equal(saved.title, 'How Jane Doe grew')
  assert.equal(saved.pdf, out)
  assert.equal(saved.cover, 'Jane Doe')
  assert.deepEqual(saved.accounts, ['https://example.com/@jane'], 'the profile URL is the account on the cover when no other is given')
  assert.ok(saved.unit === 'chapter' && saved.page_size.length === 2)

  write(join(work, 'sources.json'), JSON.stringify({ 'https://a.example/x': 'A 2020' }))
  cs.init('jane-doe', 'How Jane Doe grew', 'https://example.com/@jane', out, 12, 'Jane Doe')
  assert.deepEqual(json(work, 'sources.json'), { 'https://a.example/x': 'A 2020' })
})

test('check reports the missing chapters, the lead paragraphs and the source counts of the sourced draft', () => {
  let report = cs.check(work)
  assert.equal(report.missing_chapters.length, 12)
  assert.ok(report.draft_ok === false && report.ok === false)

  for (let n = 1; n <= 12; n++) write(join(work, 'drafts', `${pad(n)}.md`), `# Chapter ${n}\n\nLead paragraph.\n\n## Section\n\nBody.\n`)
  write(join(work, 'drafts', '04.md'), '# Chapter 4\n\n## Section first\n\nBody.\n')
  write(join(work, 'sources.json'), JSON.stringify({
    'https://www.a.example/x': 'A 2020',
    'https://a.example/y': 'A 2021',
    'https://b.example/z': 'B 2019',
    'https://web.archive.org/web/20200101000000/https://c.example/p': 'Archive 2020',
    'https://web.archive.org/web/20210101000000/https://c.example/p': 'Archive 2021',
  }))
  report = cs.check(work)
  assert.deepEqual(report.no_lead_paragraph, ['04'])
  assert.ok(report.sources === 3 && report.archive_snapshots === 2, 'archive snapshots are counted apart from other sources')
  assert.equal(report.sites, 2, 'distinct sites, without www and without the archive')
  assert.equal(report.draft_ok, false)

  write(join(work, 'drafts', '04.md'), '# Chapter 4\n\nLead.\n\n## Section\n\nBody.\n')
  report = cs.check(work)
  assert.equal(report.draft_ok, true)
  assert.ok(report.missing_book_chapters.length === 12 && report.book_ok === false, 'the book text is missing until it is written')
})

const linked = '# Sources\n\nThe list.\n\n## Press\n\n- A: [2020](https://www.a.example/x), [2021](https://a.example/y)\n- B: [2019](https://b.example/z)\n- The profile page as archived, 2020 to 2021\n'

test('check passes clean book text whose sources list links every source, and reports what is wrong with it', () => {
  for (let n = 1; n <= 12; n++) {
    write(join(work, 'drafts', `${pad(n)}.md`), `# Chapter ${n}\n\nShe had 19,936 followers in 2016 (Outlet 2020).\n\n## Section\n\nBody.\n`)
    write(join(work, 'book', `${pad(n)}.md`), `# Chapter ${n}\n\nShe had 19,936 followers in 2016.\n\n## Section\n\nBody.\n`)
  }
  write(join(work, 'book', '12.md'), linked)
  let report = cs.check(work)
  assert.ok(report.book_ok === true && report.ok === true, JSON.stringify(report))
  write(join(work, 'book', '12.md'), linked.replace('(https://b.example/z)', '(<https://b.example/z>)'))
  assert.equal(cs.check(work).book_ok, true, 'a link whose address is in angle brackets is a link')
  write(join(work, 'book', '12.md'), linked.replace(', [2021](https://a.example/y)', ', 2021').replace('- B:', '- [Elsewhere](https://d.example/q)\n- B:'))
  report = cs.check(work)
  assert.deepEqual(report.sources_not_linked, ['https://a.example/y'])
  assert.deepEqual(report.links_not_in_sources, ['https://d.example/q'])
  assert.equal(report.book_ok, false)
  write(join(work, 'book', '12.md'), linked)

  write(join(work, 'book', '02.md'), '# Chapter 2\n\nShe had 19,936 followers (Outlet 2020).\n\n## Section\n\n过了 100 万（Tubefilter 2022（人物页））。\n')
  write(join(work, 'book', '03.md'), '# Chapter 3\n\nIn 2016 (she was 19) it grew; the snapshot shows it, per sources.json.\n\n## Section\n\n没有第二个独立来源可以核对。她说“太长了”⟦too long⟧。\n')
  write(join(work, 'book', '05.md'), '# Chapter 5\n\nShe had 21,000 followers in 2016.\n\n## Section\n\nBody.\n')
  write(join(work, 'book', '12.md'), linked + '- Archive 2031\n')
  report = cs.check(work)
  assert.deepEqual(chapters(report.citations_in_book), ['02'], 'citations left in the book text are found')
  assert.equal(report.citations_in_book[0].found.length, 2)
  assert.ok(!chapters(report.citations_in_book).includes('03'), 'a plain parenthesis with an age or a date is not a citation')
  assert.deepEqual(chapters(report.process_terms_in_book), ['03'])
  for (const term of ['snapshot', 'sources.json', '独立来源', '核对', '⟦']) assert.ok(report.process_terms_in_book[0].found.includes(term), term)
  assert.ok(report.numbers_not_in_draft.some(c => c.chapter === '05' && c.found.join() === '21000'), 'a figure the sourced draft does not have is found')
  assert.ok(!chapters([...report.citations_in_book, ...report.numbers_not_in_draft]).includes('12'), 'the closing sources chapter may list outlets and years')
  assert.ok(!chapters(report.numbers_not_in_draft).includes('01'))
  assert.ok(report.book_ok === false && report.ok === false)

  write(join(work, 'drafts', '06.md'), '# Chapter 6\n\nShe had 24.8M subscribers and 1.2B views in 2016.\n\n## Section\n\nBody.\n')
  write(join(work, 'book', '06.md'), '# Chapter 6\n\n2016 年她有 2,480 万订阅、12 亿播放。\n\n## Section\n\nBody.\n')
  report = cs.check(work)
  assert.ok(!chapters(report.numbers_not_in_draft).includes('06'), 'a figure restated in another unit of ten is the draft\'s figure')

  write(join(work, 'drafts', '06.md'), '# Chapter 6\n\nThe video has 179,000,000 views and the channel 1.2 billion.\n\n## Section\n\nBody.\n')
  write(join(work, 'book', '06.md'), '# Chapter 6\n\n那条视频有 1.79 亿次播放，频道有 1,200,000,000 次，另一条有 7.77 亿次。\n\n## Section\n\nBody.\n')
  report = cs.check(work)
  assert.deepEqual(report.numbers_not_in_draft.find(c => c.chapter === '06')?.found, ['7.77'],
    'a figure written out in full is the draft\'s figure in 亿 or in billions, and other digits are not')

  const series = '2012 年 5 月 2 日是 603 个，8 月 2 日 762 个，11 月 2 日 969 个，2013 年 2 月 1 日 1,128 个，每月约 52 到 68 个。'
  write(join(work, 'drafts', '07.md'), `# Chapter 7\n\nLead.\n\n## Section\n\n${series}\n`)
  write(join(work, 'book', '07.md'), `# Chapter 7\n\nLead.\n\n## Section\n\n${series}\n`)
  report = cs.check(work)
  assert.deepEqual(chapters(report.series_in_prose), ['07'], 'a paragraph that recites a series of figures belongs in a chart')
  assert.ok(report.series_in_prose[0].found[0].startsWith('2012 年 5 月 2 日是 603') && report.book_ok === false)
  const chart = '```chart\ntype: line\ntitle: Videos\n2012-05-02 | 603\n2012-08-02 | 762\n2012-11-02 | 969\n2013-02-01 | 1,128\n```'
  write(join(work, 'book', '07.md'), `# Chapter 7\n\nLead.\n\n## Section\n\n每月约 52 到 68 个，2013 年 2 月 1 日到 1,128 个。\n\n${chart}\n`)
  report = cs.check(work)
  assert.deepEqual(report.series_in_prose, [], 'the same figures in a chart block pass')
  assert.ok(!chapters(report.numbers_not_in_draft).includes('07'), 'chart values are still checked against the draft')
  write(join(work, 'book', '07.md'), `# Chapter 7\n\nLead.\n\n${chart.replace('1,128', '1,182')}\n`)
  report = cs.check(work)
  assert.ok(report.numbers_not_in_draft.some(c => c.chapter === '07' && c.found.join() === '1182'), 'a chart value the draft does not have is reported')
  write(join(work, 'book', '07.md'), '# Chapter 7\n\nLead.\n\n## Section\n\nBody.\n')
})

test('merge builds sources.json, raw.json and gaps.md from every agent\'s files; slice and findings cut them up', () => {
  write(join(work, 'notes', 'read-01.sources.json'), JSON.stringify({ 'https://a.example/x': 'A 2020', 'https://seller.example/s': 'Seller 2021' }))
  write(join(work, 'notes', 'read-02.sources.json'), JSON.stringify({ 'https://b.example/z': 'B 2019' }))
  write(join(work, 'notes', 'read-01.gaps.md'), '- could not reach c.example\n')
  write(join(work, 'notes', 'read-02.gaps.md'), '- paywall at d.example\n')
  let merged = cs.merge(work)
  let saved = json(work, 'sources.json')
  assert.deepEqual(Object.keys(saved).sort(), ['https://a.example/x', 'https://b.example/z', 'https://seller.example/s'])
  assert.equal(merged.sources, 3)
  const gaps = read(work, 'gaps.md')
  assert.ok(gaps.includes('c.example') && gaps.includes('d.example'), 'merge gathers the gaps')
  write(join(work, 'review', 'sources-1.failed.json'), JSON.stringify(['https://seller.example/s']))
  write(join(work, 'review', 'fix-03.added.json'), JSON.stringify({ 'https://e.example/new': 'E 2022' }))
  cs.merge(work)
  saved = json(work, 'sources.json')
  assert.deepEqual(Object.keys(saved).sort(), ['https://a.example/x', 'https://b.example/z', 'https://e.example/new'], 'merge drops failed sources and adds what the fix round read')
  const parts = [1, 2].map(n => cs.sliceSources(work, n, 2))
  assert.deepEqual([...parts[0], ...parts[1]].sort(), Object.keys(saved).sort(), 'the slices cover every url once')
  assert.ok(parts[0].length === 2 && parts[1].length === 1)

  write(join(work, 'review', 'numbers-3.md'), '| row | checked |\n- [03] wrong | 24.8M | source says 24.6M | 24.6M | print 24.6M\n- [04] missing | a | b | c | d\n')
  write(join(work, 'review', 'sources-1.md'), '- [A 2020] seller-source | sells a course | | drop\n- [B 2019] mislabelled | a | b | c | d\n')
  write(join(work, 'drafts', '03.md'), '# Chapter 2\n\nShe had 24.8M subscribers (on record, A 2020).\n\n## Section\n\nBody.\n')
  assert.deepEqual(cs.findings(work, '03').map(l => l.replace(/^- F[0-9a-f]{6} /, '- ')), ['- [03] wrong | 24.8M | source says 24.6M | 24.6M | print 24.6M', '- [A 2020] seller-source | sells a course | | drop'],
    'findings for a chapter are its own lines plus the sources-lens lines about labels the chapter names')

  write(join(work, 'notes', 'read-01.raw.json'), JSON.stringify({ 'https://a.example/x': ['raw/a.html'], 'https://b.example/z': ['raw/b1.txt'] }))
  write(join(work, 'notes', 'numbers-archive.raw.json'), JSON.stringify({ 'https://b.example/z': ['raw/b2.txt'] }))
  write(join(work, 'notes', 'numbers-uploads.sources.json'), JSON.stringify([{ url: 'https://c.example/y', kind: 'on record' }]))
  merged = cs.merge(work)
  assert.deepEqual(merged.malformed, ['notes/numbers-uploads.sources.json'], 'notes files that are not a url-keyed object are named: their sources would be dropped unseen')
  unlinkSync(join(work, 'notes', 'numbers-uploads.sources.json'))
  const raw = json(work, 'raw.json')
  assert.deepEqual(Object.fromEntries(Object.entries(raw).map(([u, f]) => [u, [...f].sort()])),
    { 'https://a.example/x': ['raw/a.html'], 'https://b.example/z': ['raw/b1.txt', 'raw/b2.txt'] }, 'merge gathers where each source\'s text was saved')
  const last = read(work, 'drafts', '12.md')
  assert.ok(last.startsWith('# Sources\n') && last.includes('A 2020') && !last.includes('Seller 2021') && cs.hasLeadParagraph(last), 'merge writes the draft\'s sources chapter')
})

test('figures matches every figure of a chapter against the saved text of the sources its sentence names', () => {
  write(join(work, 'raw', 'a.html'), '<p>She passed 1.002.877 subscribers and earned $12 million; <span title="24,8 miljoner">many</span> watched 603 videos.</p>')
  write(join(work, 'raw', 'b1.txt'), 'The channel showed 29 321 179 subscribers, 11.941 million by another count.\n')
  write(join(work, 'raw', 'b2.txt'), 'Views that week: 50,566,204. Turnover 7 million kronor.\n')
  write(join(work, 'drafts', '05.md'),
    '# Chapter 4\n\n她一共有 1,002,877 订阅，另一处写 11.941M。\n\n## Section\n\n' +
    '2012 年 7 月 11 日她过了 1,002,877 订阅，收入 1,200 万美元，有 603 条视频（A 2020，当时的报道）。' +
    'B 2019 的页面写 29,321,179 订阅、当周 50,566,204 次观看。' +
    '她 7 天发了 8 条，占 45%（A 2020）。\n\n' +
    '她的公司营业额 720 万克朗，另一篇写 2,480 万人看过（B 2019）。' +
    '同一年她有 31,000 个付费会员。\n\n' +
    'E 2022 写她有 5,555 个订阅。\n')
  let report = cs.figures(work, '05')
  const missed = new Set(report.unmatched.map(m => `${m.figure}|${m.labels.join(',')}`))
  const figures = new Set(report.unmatched.map(m => m.figure))
  for (const f of ['1,002,877', '1,200 万', '603', '29,321,179', '50,566,204', '11.941M']) assert.ok(!figures.has(f), `${f} is in its source's saved text`)
  assert.ok(missed.has('720 万|B 2019') && missed.has('2,480 万|B 2019'), 'a figure in another source\'s text, or in none, is reported with the sources its sentence names')
  assert.ok(missed.has('31,000|B 2019'), 'a sentence that names no source takes its paragraph\'s')
  assert.ok(missed.has('5,555|E 2022'), 'a source with no saved text leaves its figures unmatched')
  assert.ok(missed.size === 4 && report.checked === 11 && report.small === 3, `dates and small figures are not checked: ${JSON.stringify(report)}`)
  let lines = read(work, 'review', 'figures-05.md').split('\n')
  assert.equal(lines.filter(l => l.startsWith('- [05] unsupported | ')).length, 3, 'one finding line per sentence')
  assert.ok(lines.some(l => l.includes('720 万') && l.includes('2,480 万') && l.includes('B 2019')))
  assert.ok(cs.findings(work, '05').some(l => l.includes('5,555')), 'the fixer receives them with its other findings')
  cs.figures(work, '05', true)
  lines = read(work, 'review', 'figures-05.md').split('\n')
  assert.equal(lines.filter(l => l.startsWith('* ')).length, 3, 'as a reviewer\'s worklist the unmatched figures are not findings yet')
  assert.ok(!cs.findings(work, '05').some(l => l.includes('5,555')))
  cs.figures(work, '05')

  write(join(work, 'drafts', '06.md'),
    '# Chapter 5\n\n## Section\n\n' +
    '她过了 1,002,877 订阅。〔当时的报道 · A 2020〕当周有 50,566,204 次观看。〔记录 · B 2019〕\n\n' +
    '她有 603 条视频。（A 2020，记录）页面写 29,321,179 订阅。（B 2019）\n\n' +
    '她有 603 条视频。〔记录 · A 2020〕当周有 50,566,204 次观看〔记录 · B 2019〕。\n')
  report = cs.figures(work, '06')
  const after = new Set(report.unmatched.map(m => m.figure))
  assert.ok(!['1,002,877', '603', '29,321,179'].some(f => after.has(f)), 'a label after its sentence\'s full stop belongs to that sentence, in either kind of bracket')
  assert.deepEqual(report.unmatched.filter(m => m.sentence.startsWith('当周')), [])

  write(join(work, 'drafts', '07.md'), '# Chapter 6\n\n## Section\n\n开头一句没有数字。[记录: B 2019] 当周有 50,566,204 次观看。[当时的报道: A 2020] 她过了 1,002,877 订阅。\n\n' +
    '[记录: A 2020] 她有 603 条视频。\n')
  report = cs.figures(work, '07')
  assert.deepEqual(report.unmatched, [], 'a label before its sentence belongs to the sentence after it, in a chapter whose paragraphs open with a label')
})

test('quotes looks up every quotation of a chapter in the saved text of the sources its sentence names', () => {
  const dir = cs.init('quoted', 'How she grew', 'https://example.com/@q', out, 5, 'Q').work
  write(join(dir, 'sources.json'), JSON.stringify({ 'https://f.example/a': 'Forbes 2026a', 'https://v.example/b': 'Variety 2021', 'https://i.example/c': 'Insider 2020' }))
  write(join(dir, 'raw.json'), JSON.stringify({ 'https://f.example/a': ['raw/forbes.html'], 'https://v.example/b': ['raw/variety.vtt'] }))
  write(join(dir, 'raw', 'forbes.html'), '<p>Asked about captions, Monk said: &quot;Is it long? If you make your video caption <em>too long</em>,\nit will be too much for people to digest on a short platform.&quot; ' +
    'Her text is “words off the top of my head, always very simplified”. She called the old clips, which she doesn&#39;t watch, so cringey.</p>')
  write(join(dir, 'raw', 'variety.vtt'), 'WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nher manager called her a specialist\n\n00:00:03.000 --> 00:00:05.000\nin short-form TikToks back then\n')
  write(join(dir, 'drafts', '04.md'), '# Chapter\n\nLead.\n\n## Section\n\n' +
    '她说“太长吗？标题太长，观众消化不了”⟦Is it long? If you make your video caption too long, it will be too much for people to digest⟧，文案是 "words off the top of my head … very simplified"（自述，Forbes 2026a）。' +
    '她说旧视频她“doesn’t watch”，经纪人叫她“短视频专家”⟦a specialist in short-form TikToks⟧（Forbes 2026a）。' +
    '她说“我每天发三条”（Forbes 2026a）。\n\n' +
    '有人说她 "posts five times a day"（Insider 2020）。\n')
  const report = cs.quotes(dir, '04')
  const row = start => report.rows.find(r => r.looked.startsWith(start))
  assert.equal(report.rows.length, 6)
  const first = row('Is it long?')
  assert.ok(first.verdict === 'found' && first.file === 'raw/forbes.html', 'a translated quotation is looked up by the original after it')
  assert.ok(first.passage.includes('Monk said') && first.passage.includes('on a short platform') && !first.passage.includes('<em>'), 'the passage is the source\'s text around the words, without markup')
  assert.ok(first.sentence.includes('Forbes 2026a'), 'a full stop inside a quotation does not end the sentence')
  assert.equal(row('words off').verdict, 'found', 'the pieces around an ellipsis are found in order')
  assert.equal(row('doesn’t watch').verdict, 'found', 'apostrophes written as entities or curly marks match')
  const elsewhere = row('a specialist')
  assert.ok(elsewhere.verdict === 'in another source' && elsewhere.label === 'Variety 2021' && elsewhere.passage.includes('her manager called her'), 'words found only in another source\'s text name that source; captions are read across their cues')
  assert.equal(row('我每天发三条').verdict, 'not found')
  assert.equal(row('posts five').verdict, 'no saved text')
  assert.deepEqual([report.found, report.elsewhere, report.missing, report.unsaved], [3, 1, 1, 1])
  const file = read(dir, 'review', 'quotations-04.md')
  assert.ok(file.includes('* found | ') && file.includes('* not found | ') && file.includes('Forbes 2026a: raw/forbes.html'), 'the worklist has a row per quotation and the saved files of every label the chapter names')
  assert.deepEqual(cs.findings(dir, '04'), [], 'worklist rows are not findings')
})

test('check rejects a sources.json that is not a url-to-label object', () => {
  write(join(work, 'sources.json'), '[1, 2]')
  const report = cs.check(work)
  assert.ok(report.draft_ok === false && report.sources_valid === false)
  assert.ok(existsSync(join(work, 'chapters.json')))
})

test('merge gives each of several sources that share a label its own label, in sources.json and in the notes', () => {
  const twins = cs.init('label-twins', 'How Twins grew', 'https://example.com/@twins', join(tmp, 'pdf', 'twins.pdf'), 12, 'Twins').work
  const [first, second, third, alone] = ['https://tube.example/a', 'https://tube.example/b', 'https://tube.example/c', 'https://variety.example/v']
  write(join(twins, 'notes', 'read-01.sources.json'), JSON.stringify({ [first]: 'Tubefilter 2025', [second]: 'Tubefilter 2025', [alone]: 'Variety 2018' }))
  write(join(twins, 'notes', 'read-02.sources.json'), JSON.stringify({ [third]: 'Tubefilter 2025' }))
  write(join(twins, 'notes', 'read-01.md'),
    `## Tubefilter — Team Water (2025-08-01)\nurl: ${first}\n- [c06] [reported at the time] (2025-08) raised 40M — Tubefilter 2025\n\n` +
    `## Tubefilter — Netflix deal (2025-08-19)\nurl: ${second}\n- [c08] [reported at the time] (2025-08) a show — Tubefilter 2025\n\n` +
    `## Variety — Profile (2018)\nurl: ${alone}\n- [c02] [reported later] (2015) joined a company — Variety 2018\n`)
  const expected = { [first]: 'Tubefilter 2025a', [second]: 'Tubefilter 2025b', [alone]: 'Variety 2018', [third]: 'Tubefilter 2025c' }
  cs.merge(twins)
  assert.deepEqual(json(twins, 'sources.json'), expected, 'a label only one source has is kept')
  const notes = read(twins, 'notes', 'read-01.md')
  assert.ok(notes.includes('raised 40M — Tubefilter 2025a\n') && notes.includes('a show — Tubefilter 2025b\n') && notes.includes('— Variety 2018\n'),
    'the bullets under a source name it by its own label')
  cs.merge(twins)
  assert.deepEqual(json(twins, 'sources.json'), expected, 'merging again changes nothing')
  assert.equal(read(twins, 'notes', 'read-01.md'), notes)
  write(join(twins, 'review', 'sources-1.failed.json'), JSON.stringify([first]))
  cs.merge(twins)
  assert.equal(json(twins, 'sources.json')[second], 'Tubefilter 2025b', 'a label does not move when another source with it fails')
  const many = Object.fromEntries(Array.from({ length: 27 }, (_, n) => [`https://x.example/${pad(n)}`, 'X post 2025']))
  write(join(twins, 'notes', 'read-03.sources.json'), JSON.stringify(many))
  cs.merge(twins)
  const posts = json(twins, 'sources.json')
  assert.deepEqual([posts['https://x.example/00'], posts['https://x.example/26']], ['X post 2025aa', 'X post 2025ba'], 'no label is the start of another')
})

test('unread tells an interrupted reader which sources are done, which are saved and which to fetch; merge reads the saved list', () => {
  const cut = cs.init('cut-reader', 'How Cut grew', 'https://example.com/@cut', join(tmp, 'pdf', 'cut.pdf'), 12, 'Cut').work
  const [done, saved, never] = ['https://a.example/done', 'https://b.example/saved', 'https://c.example/never']
  assert.deepEqual(cs.unread(cut, 'read-01', [done, saved, never]), [done, saved, never].map(u => `fetch ${u}`), 'a first run fetches everything')
  write(join(cut, 'notes', 'read-01.raw.tsv'), `${done}\traw/read-01/a.txt\n${saved}\traw/read-01/b.txt\n${saved}\traw/read-01/b2.txt\n`)
  write(join(cut, 'notes', 'read-01.md'), `## A — title (2020)\nurl: ${done}\nread: curl, full\n- [c03] [on record] (2020) a fact — A 2020\n`)
  assert.deepEqual(cs.unread(cut, 'read-01', [done, saved, never]),
    [`done ${done}`, `saved ${saved} raw/read-01/b.txt raw/read-01/b2.txt`, `fetch ${never}`])
  write(join(cut, 'notes', 'read-01.sources.json'), JSON.stringify({ [done]: 'A 2020' }))
  write(join(cut, 'notes', 'numbers-archive.raw.json'), JSON.stringify({ [done]: ['raw/archive/000'] }))
  cs.merge(cut)
  assert.deepEqual(json(cut, 'raw.json'), { [done]: ['raw/read-01/a.txt', 'raw/archive/000'], [saved]: ['raw/read-01/b.txt', 'raw/read-01/b2.txt'] })
})

test('findings gives each finding a name that stays the same, and leaves out the ones the fix log already names', () => {
  const half = cs.init('half-fixed', 'How Half grew', 'https://example.com/@half', join(tmp, 'pdf', 'half.pdf'), 12, 'Half').work
  write(join(half, 'drafts', '04.md'), '# Methods\n\nShe posted daily (self-reported, A 2020).\n')
  write(join(half, 'review', 'quotes-04.md'), '- [04] wrong | first | a | b | c\n- [04] unsupported | second | a | b | c\n- [05] wrong | other chapter | a | b | c\n')
  write(join(half, 'review', 'sources-1.md'), '- [A 2020] seller-source | sells a course | | drop\n')
  const all = cs.findings(half, '04')
  assert.equal(all.length, 3)
  const names = all.map(l => /^- (F[0-9a-f]{6}) \[/.exec(l)[1])
  assert.equal(new Set(names).size, 3)
  assert.ok(all[0].endsWith('[04] wrong | first | a | b | c'))
  write(join(half, 'review', 'fix-04.md'), `${names[0]} fixed | first\n${names[2]} removed | the seller's sentence\n`)
  write(join(half, 'drafts', '04.md'), '# Methods\n\nShe posted daily (self-reported, A 2020). An edit.\n')
  assert.deepEqual(cs.findings(half, '04'), [all[1]], 'an edited chapter keeps the names; only the finding with no log line is left')
})

test('bullets prints a chapter\'s lines from the notes, with a dated table of the numbers notes thinned to a row per quarter', () => {
  const long = cs.init('long-curve', 'How Long grew', 'https://example.com/@long', join(tmp, 'pdf', 'long.pdf'), 12, 'Long').work
  const row = (date, n, tags = '[c03]') => `${tags} [on record] ${date} | ${n} followers | https://web.archive.org/web/${date.replaceAll('-', '')}/x`
  write(join(long, 'notes', 'numbers-archive.md'), ['## Curve: profile captures', '',
    row('2020-05-07', '6,940,874'), row('2020-05-20', '7,500,000'), row('2020-06-30', '8,800,000'),
    row('2020-08-01', '9,800,000'), row('2020-08-02', '10,000,000', '[c03] [c07]'), row('2021-01-09', '13,100,000'), row('2021-02-01', '13,500,000'),
    '', '## Milestones', '', '[c03] [c07] [on record] 10M | first at/above: 2020-08-02', '[c07] [on record] 15M | first at/above: 2021-05-03', ''].join('\n'))
  write(join(long, 'notes', 'read-01.md'), '## A — title (2020)\nurl: https://a.example/x\n- [c03][c04] [on record] (2020) a fact — A 2020\n- [c04] [self-reported] (2020) a method — A 2020\n')
  assert.deepEqual(cs.bullets(long, '03'), [
    '## Curve: profile captures',
    row('2020-05-07', '6,940,874'), row('2020-08-01', '9,800,000'), row('2021-01-09', '13,100,000'), row('2021-02-01', '13,500,000'),
    '(3 more rows of this table are in notes/numbers-archive.md)',
    '## Milestones',
    '[c03] [c07] [on record] 10M | first at/above: 2020-08-02',
    '- [c03][c04] [on record] (2020) a fact — A 2020'])
  assert.deepEqual(cs.bullets(long, '07'), ['## Curve: profile captures', row('2020-08-02', '10,000,000', '[c03] [c07]'),
    '## Milestones', '[c03] [c07] [on record] 10M | first at/above: 2020-08-02', '[c07] [on record] 15M | first at/above: 2021-05-03'])
  write(join(long, 'notes', 'read-02.md'), '## B — a seller (2021)\nurl: https://b.example/y\n- [c04] [self-reported] (2021) a claim — B 2021\n## C — press (2021)\nurl: https://c.example/z\n- [c04] [on record] (2021) a report — C 2021\n')
  write(join(long, 'review', 'sources-1.failed.json'), JSON.stringify(['https://b.example/y']))
  assert.deepEqual(cs.bullets(long, '04'), ['- [c03][c04] [on record] (2020) a fact — A 2020', '- [c04] [self-reported] (2020) a method — A 2020', '- [c04] [on record] (2021) a report — C 2021'],
    'the bullets of a source the reviewers failed are left out')
})

test('codex runs the fixer prompt of a chapter on a Codex model, in the work directory, and reports its last message and usage', () => {
  const work = join(tmp, 'codex')
  for (const dir of ['review', 'drafts']) mkdirSync(join(work, dir), { recursive: true })
  write(join(work, 'review', 'fix-04.prompt.txt'), 'You are a fixer. Your chapter file: drafts/04.md (NN = 04).')
  const argv = cs.codexCommand(work, '04', 'gpt-6-luna')
  assert.equal(argv[0], 'codex')
  assert.ok(argv.includes('exec') && argv.includes('--json') && argv.includes('--skip-git-repo-check'), 'one-shot, machine-readable, outside a repo')
  assert.deepEqual(argv.slice(argv.indexOf('-m'), argv.indexOf('-m') + 2), ['-m', 'gpt-6-luna'])
  assert.ok(argv.some(a => a.includes('model_reasoning_effort=high')), 'the fixer reasons at high effort')
  assert.ok(argv.some(a => a.includes('writable_roots') && a.includes('case-study-limits')), 'the gate script keeps its pacing files outside the work directory')
  assert.equal(argv.at(-1), 'You are a fixer. Your chapter file: drafts/04.md (NN = 04).', 'the prompt is the saved one')
  const events = [
    { type: 'item.completed', item: { type: 'agent_message', text: 'first' } },
    { type: 'item.completed', item: { type: 'command_execution', command: 'ls' } },
    { type: 'item.completed', item: { type: 'agent_message', text: '14 findings: 12 fixed, 2 removed.' } },
    { type: 'turn.completed', usage: { input_tokens: 100, cached_input_tokens: 80, output_tokens: 7 } },
  ].map(e => JSON.stringify(e)).join('\n')
  assert.deepEqual(cs.codexResult(events), { message: '14 findings: 12 fixed, 2 removed.', usage: { input_tokens: 100, cached_input_tokens: 80, output_tokens: 7 } })
  assert.deepEqual(cs.codexResult(''), { message: '', usage: null }, 'a run that wrote nothing reports nothing')
})

test('both scripts run when called through a symlink to their folder, as an installed plugin is', () => {
  const linked = join(tmp, 'linked-scripts')
  symlinkSync(fileURLToPath(new URL('../scripts', import.meta.url)), linked)
  for (const name of ['case-study.mjs', 'wayback.mjs']) {
    const res = spawnSync(process.execPath, [join(linked, name)], { encoding: 'utf8' })
    assert.match(res.stdout + res.stderr, new RegExp(`usage: ${name}`))
  }
})
