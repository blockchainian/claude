#!/usr/bin/env node
// ABOUTME: Scaffolds a case-study work dir in the digest store and checks it before rendering.
// ABOUTME: init writes chapters.json + sources.json; check verifies the sourced draft, the book text and sources.
//
// Usage: case_study.mjs init <slug> --title <title> --cover <name> --source <url> [--account <url>]... --out <pdf> [--chapters 11]
//        case_study.mjs merge <work dir>
//        case_study.mjs slice <work dir> <n> <of>      (the urls of one reviewer's slice, one per line)
//        case_study.mjs figures <work dir> <NN> [--worklist]   (match chapter NN's figures against the saved source text)
//        case_study.mjs findings <work dir> <NN>       (the review lines one fixer applies to chapter NN)
//        case_study.mjs check <work dir> [--draft]
// A study has two layers: md/NN.md is the sourced draft the reviewers audit (sources named in every sentence);
// book/NN.md is the text that is typeset (no citations, no account of the research).
// Agents working in parallel never share a file: each writes its own notes/<name>.sources.json (url -> label) and
// notes/<name>.gaps.md, reviewers write review/<name>.failed.json (a list of urls), fixers write
// review/<name>.added.json (url -> label). merge turns those into sources.json, gaps.md and the draft's last chapter,
// and notes/<name>.raw.json (url -> the files its text was saved to) into raw.json.
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const PAGE_SIZE = [427.92, 660.0] // the book format digest's render typesets
const ARCHIVE_HOSTS = new Set(['web.archive.org', 'archive.org'])
// A Markdown link's target.
const LINK = /\]\((https?:\/\/[^)\s]+)\)/g
// A parenthesis that names a source: a word, then a year that is not part of a date.
const CITATION = /[（(][^（()）]*?(?<word>[A-Za-z一-鿿][\p{L}\p{N}_.&'’-]*)\s+(?:19|20)\d\d(?!\s*年)[^（()）]*[）)（(]/gu
const DATE_WORDS = new Set(['in', 'since', 'from', 'by', 'until', 'to', 'of', 'late', 'early', 'mid', 'born', 'and', 'january',
  'february', 'march', 'april', 'may', 'june', 'july', 'august', 'september', 'october', 'november', 'december'])
// Wording that describes the research instead of the subject.
const PROCESS_TERMS = ['sources.json', 'notes.md', 'gaps.md', 'subagent', 'research agent', '调查 agent', 'snapshot',
  'yt-dlp', 'self-reported', 'independent source', '独立来源', '核对', '快照', '存档页', '有记录佐证',
  '本人说的', '当时的报道', '事后报道']
const NUMBER = /\d[\d,]*(?:\.\d+)?/g
// A figure with the unit that scales it; separators inside it may be commas, dots or spaces.
const CJK_UNITS = new Set(['十万', '百万', '千万', '万', '亿', '千'])
const UNITS = { '': 1, k: 1e3, K: 1e3, thousand: 1e3, m: 1e6, M: 1e6, mn: 1e6, million: 1e6, bn: 1e9, b: 1e9, B: 1e9,
  billion: 1e9, '千': 1e3, '万': 1e4, '十万': 1e5, '百万': 1e6, '千万': 1e7, '亿': 1e8 }
// A unit ends where a word ends: the same boundary for Latin and CJK letters.
const AMOUNT = /(?<![\p{L}\p{N}_.])(\d{1,3}(?:[  ]\d{3})+(?!\d)|\d[\d,.]*\d|\d)\s*(十万|百万|千万|万|亿|千|thousand(?![\p{L}\p{N}_])|million(?![\p{L}\p{N}_])|billion(?![\p{L}\p{N}_])|mn(?![\p{L}\p{N}_])|bn(?![\p{L}\p{N}_])|[kKmMbB](?![\p{L}\p{N}_]))?/gu
const CHART = /^```chart[ \t]*\n[\s\S]*?\n```[ \t]*$/gm // a figure block the renderer draws
// A date, whose digits are not figures of a series: 2012 年 5 月 2 日, 8 月 2 日, 2012-05-02, a bare year.
const DATE = /(?:19|20)\d\d\s*年(?:\s*\d+\s*月)?(?:\s*\d+\s*日)?|\d+\s*月(?:\s*\d+\s*日)?|\d+\s*日|(?:19|20)\d\d(?:-\d\d){0,2}/g
const SERIES = 5 // a paragraph with this many figures besides its dates recites a series

const root = () => process.env.HIGHLIGHTS_DIR || join(homedir(), 'Documents', 'highlights')
const read = path => readFileSync(path, 'utf8')
const write = (path, text) => writeFileSync(path, text, 'utf8')
const dump = value => JSON.stringify(value, null, 2)
const pad = n => String(n).padStart(2, '0')
const sorted = items => [...items].sort()
// The files of a folder whose names end with a suffix, sorted by name; none when the folder is missing.
const files = (dir, suffix) => (existsSync(dir) ? readdirSync(dir).filter(name => name.endsWith(suffix)).sort().map(name => join(dir, name)) : [])
const host = url => {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return ''
  }
}
const lines = text => text.split(/\r?\n/)
const paragraphsOf = text => text.split(/\n\s*\n/)

export function init(slug, title, source, out, chapters, cover, accounts = null) {
  // Create <store>/.work/<slug>/ with md/, book/, notes/, raw/, review/, chapters.json and an empty sources.json.
  const work = join(root(), '.work', slug)
  for (const sub of ['md', 'book', 'notes', 'raw', 'review']) mkdirSync(join(work, sub), { recursive: true })
  const ids = Array.from({ length: chapters }, (_, n) => pad(n + 1))
  const meta = { title, cover, accounts: accounts && accounts.length ? accounts : [source], slug, source, pdf: String(out), work,
    unit: 'chapter', page_size: PAGE_SIZE,
    chapters: ids.map(i => ({ id: i, title: i, pages: [1, 1], chars: 0, text: `text/${i}.txt`, highlights: `book/${i}.md` })) }
  write(join(work, 'chapters.json'), dump(meta))
  const sources = join(work, 'sources.json')
  if (!existsSync(sources)) write(sources, '{}')
  return meta
}

const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value)

function readJson(path, kind) {
  // The JSON in a file when it is of the expected kind ('object' or 'array'), else an empty one.
  const empty = kind === 'array' ? [] : {}
  let data
  try {
    data = JSON.parse(read(path))
  } catch {
    return empty
  }
  return (kind === 'array' ? Array.isArray(data) : isObject(data)) ? data : empty
}

export function merge(work) {
  // Rebuild sources.json, gaps.md and the draft's sources chapter from the files the parallel agents wrote.
  const meta = JSON.parse(read(join(work, 'chapters.json')))
  const notes = join(work, 'notes')
  const review = join(work, 'review')
  let sources = {}
  for (const path of [...files(notes, '.sources.json'), ...files(review, '.added.json')]) {
    for (const [k, v] of Object.entries(readJson(path, 'object'))) if (typeof v === 'string') sources[k] = v
  }
  const failed = new Set(files(review, '.failed.json').flatMap(path => readJson(path, 'array')))
  sources = Object.fromEntries(Object.entries(sources).filter(([url]) => !failed.has(url)))
  write(join(work, 'sources.json'), dump(sources))
  const saved = {}
  for (const path of [...files(notes, '.raw.json'), ...files(review, '.raw.json')]) {
    for (const [url, list] of Object.entries(readJson(path, 'object'))) {
      saved[url] ??= []
      for (const f of typeof list === 'string' ? [list] : list) if (!saved[url].includes(f)) saved[url].push(f)
    }
  }
  write(join(work, 'raw.json'), dump(saved))
  const gaps = files(notes, '.gaps.md').map(path => read(path).trim()).filter(Boolean)
  write(join(work, 'gaps.md'), gaps.join('\n') + '\n')
  const labels = sorted(new Set(Object.entries(sources).map(([url, label]) => `${label} (${host(url)})`)))
  const last = join(work, 'md', `${meta.chapters.at(-1).id}.md`)
  write(last, `# Sources\n\n${Object.keys(sources).length} sources, listed in sources.json.\n\n## List\n\n` + labels.map(l => `- ${l}`).join('\n') + '\n')
  const malformed = [...files(notes, '.json'), ...files(review, '.added.json'), ...files(review, '.raw.json')]
    .filter(path => Object.keys(readJson(path, 'object')).length === 0 && !['', '{}'].includes(read(path).trim()))
    .map(path => relative(work, path))
  return { sources: Object.keys(sources).length, failed: failed.size, gaps: gaps.reduce((n, g) => n + g.split('\n').length, 0), malformed }
}

export function sliceSources(work, index, of) {
  // Slice number index (from 1) of the source urls, sorted, cut into `of` near-equal parts.
  const urls = sorted(Object.keys(readJson(join(work, 'sources.json'), 'object')))
  const size = Math.ceil(urls.length / of)
  return urls.slice((index - 1) * size, index * size)
}

export function findings(work, chapter) {
  // The review lines a fixer applies to one chapter: those tagged with its number, and the sources-lens lines
  // (tagged with a source label) about labels the chapter names.
  const text = read(join(work, 'md', `${chapter}.md`))
  const out = []
  for (const path of files(join(work, 'review'), '.md')) {
    const fromSources = path.split('/').at(-1).startsWith('sources-')
    for (const line of lines(read(path))) {
      const tag = /^- \[([^\]]+)\]/.exec(line)
      if (tag && (tag[1] === chapter || (fromSources && text.includes(tag[1])))) out.push(line)
    }
  }
  return out
}

const round6 = n => Math.round(n * 1e6) / 1e6

export function amounts(text) {
  // Every figure in a text as {written: the figure as written, values: the values it may stand for, digits: its digits
  // without leading or trailing zeros}. A unit after the figure scales the value: 1,200 万 and 12 million are both
  // 12,000,000. One separator before a last group of three digits may be a thousands mark or a decimal point: both are kept.
  const found = []
  for (const m of text.matchAll(AMOUNT)) {
    const token = m[1]
    const unit = m[2] || ''
    const groups = token.split(/[.,\s ]/)
    const whole = Number(groups.join(''))
    const decimal = groups.length > 1 ? Number(groups.slice(0, -1).join('') + '.' + groups.at(-1)) : whole
    let numbers
    if (groups.length === 1 || (groups.length > 2 && groups.at(-1).length === 3)) numbers = [whole]
    else if (groups.at(-1).length === 3) numbers = [whole, decimal]
    else numbers = [decimal]
    const scale = UNITS[unit in UNITS ? unit : unit.toLowerCase()]
    const written = CJK_UNITS.has(unit) ? `${token} ${unit}`.trim() : token + unit
    found.push({ written, values: numbers.map(n => round6(n * scale)), digits: groups.join('').replace(/^0+|0+$/g, '') })
  }
  return found
}

const LABEL_AFTER_STOP = /([。！？!?])\s*([〔（(\[][^〔〕（）()\[\]]*[〕）)\]])/g
const LABEL_FIRST = /^\s*[〔（(\[]/
const LABEL_LAST = /[。！？!?]\s*[〔（(\[][^〔〕（）()\[\]]*[〕）)\]]\s*$/

export function figures(work, chapter, worklist = false) {
  // Match every figure in a draft chapter against the saved text of the sources its sentence names, and write
  // the ones that are not there as findings for the chapter's fixer, or, as a worklist, for a reviewer to judge.
  const labels = new Map() // label -> urls, in sources.json's order (an object would move a label that looks like an integer first)
  for (const [url, label] of Object.entries(readJson(join(work, 'sources.json'), 'object'))) labels.set(label, [...(labels.get(label) || []), url])
  const saved = readJson(join(work, 'raw.json'), 'object')
  const texts = {}
  const isFile = path => existsSync(path) && statSync(path).isFile()
  const known = label => {
    // {values, digits} of the figures in everything saved for a label's sources.
    if (!(label in texts)) {
      const parts = labels.get(label).flatMap(url => (saved[url] || []).filter(f => isFile(join(work, f))).map(f => read(join(work, f))))
      const found = amounts(parts.join('\n'))
      texts[label] = { values: new Set(found.flatMap(f => f.values)), digits: new Set(found.filter(f => f.digits.length >= 4).map(f => f.digits)) }
    }
    return texts[label]
  }
  const ordered = [...labels.keys()].sort((a, b) => b.length - a.length)
  const named = text => ordered.filter(label => text.includes(label))
  let checked = 0
  let small = 0
  const unmatched = []
  const out = []
  const body = read(join(work, 'md', `${chapter}.md`))
  const anywhere = named(body) // a paragraph that names no source (a lead, a summary) restates the chapter's own figures
  const paragraphs = paragraphsOf(body)
  // A label in brackets right after a full stop belongs to the sentence before it in a chapter whose paragraphs
  // end on a label, and to the sentence after it in a chapter whose paragraphs open with one.
  const count = re => paragraphs.filter(p => re.test(p)).length
  const labelsFollow = count(LABEL_LAST) > count(LABEL_FIRST)
  for (let paragraph of paragraphs) {
    if (paragraph.trimStart().startsWith('#')) continue
    if (labelsFollow) paragraph = paragraph.replace(LABEL_AFTER_STOP, (whole, stop, label) => (named(label).length ? label + stop : whole))
    for (const sentence of paragraph.split(/(?<=[。！？!?])|(?<=\.)\s+(?=[A-Z])|\n/)) {
      const cited = [named(sentence), named(paragraph), anywhere].find(list => list.length) || []
      let bare = sentence
      for (const label of ordered) bare = bare.replaceAll(label, '')
      const missing = []
      for (const { written, values, digits } of amounts(bare.replace(DATE, ''))) {
        if (Math.max(...values) < 100 && digits.length < 3) {
          small += 1
          continue
        }
        checked += 1
        const held = cited.some(label => values.some(v => known(label).values.has(v)) || (digits.length >= 4 && known(label).digits.has(digits)))
        if (!held) {
          missing.push(written)
          unmatched.push({ figure: written, labels: cited, sentence: sentence.trim() })
        }
      }
      if (missing.length) {
        const where = cited.length ? `not in the saved text of ${cited.join(', ')}` : 'its sentence names no source'
        out.push(`${worklist ? '*' : `- [${chapter}] unsupported |`} ${sentence.trim().slice(0, 200)} | ${missing.join(', ')}: ${where} | | ` +
          'open the source and find each figure: correct it, say what it was computed from if it is derived, or remove it')
      }
    }
  }
  mkdirSync(join(work, 'review'), { recursive: true })
  write(join(work, 'review', `figures-${chapter}.md`),
    `# Figures in md/${chapter}.md matched against the saved text of their sources\n\n` +
    `${checked} checked, ${checked - unmatched.length} matched, ${unmatched.length} not matched, ${small} too small to match (left to the quotes lens).\n\n` +
    out.join('\n') + '\n')
  return { chapter, checked, matched: checked - unmatched.length, small, unmatched }
}

export function hasLeadParagraph(text) {
  // True when prose sits between the chapter's title line and its first '## ' heading.
  const kept = lines(text).map(line => line.trim()).filter(Boolean)
  const body = kept.length && kept[0].startsWith('# ') ? kept.slice(1) : kept
  return body.length > 0 && !body[0].startsWith('#')
}

export function numbers(text) {
  // The figures in a text, without thousands separators.
  return new Set([...text.matchAll(NUMBER)].map(m => m[0].replaceAll(',', '').replace(/\.+$/, '')))
}

export function recitedSeries(text) {
  // The openings of the paragraphs that recite a run of figures in prose instead of showing a chart or a table.
  const found = []
  for (const paragraph of paragraphsOf(text.replace(CHART, ''))) {
    if (!paragraph.trimStart().startsWith('#') && [...paragraph.replace(DATE, '').matchAll(NUMBER)].length >= SERIES) found.push(paragraph.trim().slice(0, 40))
  }
  return found
}

export function restated(number, knownValues) {
  // True when the figure is a known one in another unit: 24.8M as 2,480 万, 1.2B as 12 亿.
  const value = Number(number)
  const tolerance = 1e-9 * Math.max(value, 1)
  return knownValues.some(k => [-4, -3, -2, -1, 0, 1, 2, 3, 4].some(e => Math.abs(value - k * 10 ** e) <= tolerance))
}

function layer(work, folder, ids) {
  // {missing ids, ids with no lead paragraph, id -> text} for one layer of chapters.
  const missing = []
  const noLead = []
  const texts = new Map() // id -> text, in chapter order (an object would put '10' before '01')
  for (const i of ids) {
    const path = join(work, folder, `${i}.md`)
    const text = existsSync(path) ? read(path) : ''
    if (!text.trim()) {
      missing.push(i)
      continue
    }
    texts.set(i, text)
    if (!hasLeadParagraph(text)) noLead.push(i)
  }
  return { missing, noLead, texts }
}

export function check(work) {
  // Report what blocks the review (the sourced draft, sources.json) and what blocks rendering (the book text).
  const meta = JSON.parse(read(join(work, 'chapters.json')))
  const ids = meta.chapters.map(chapter => chapter.id)
  const { missing, noLead, texts: draft } = layer(work, 'md', ids)
  const { missing: bookMissing, noLead: bookNoLead, texts: book } = layer(work, 'book', ids)
  const known = new Set([...draft.values()].flatMap(text => [...numbers(text)]))
  const knownValues = [...known].map(Number)
  const citations = []
  const process_ = []
  const unknown = []
  const series = []
  let links = new Set()
  for (const [i, text] of book) {
    const lower = text.toLowerCase()
    const terms = PROCESS_TERMS.filter(term => lower.includes(term))
    if (terms.length && i !== ids.at(-1)) process_.push({ chapter: i, found: terms })
    if (i === ids.at(-1)) { // the closing sources chapter lists outlets and years, each linked to its source
      links = new Set([...text.matchAll(LINK)].map(m => m[1]))
      continue
    }
    const cited = [...text.matchAll(CITATION)].filter(m => !DATE_WORDS.has(m.groups.word.toLowerCase())).map(m => m[0])
    if (cited.length) citations.push({ chapter: i, found: cited })
    const extra = sorted([...numbers(text)].filter(n => !known.has(n) && !restated(n, knownValues)))
    if (extra.length) unknown.push({ chapter: i, found: extra })
    const recited = recitedSeries(text)
    if (recited.length) series.push({ chapter: i, found: recited })
  }
  let sources = null
  try {
    sources = JSON.parse(read(join(work, 'sources.json')))
  } catch {
    sources = null
  }
  const valid = isObject(sources) && Object.values(sources).every(v => typeof v === 'string')
  const urls = valid ? Object.keys(sources) : []
  const hosts = urls.map(host)
  const snapshots = hosts.filter(h => ARCHIVE_HOSTS.has(h)).length
  const listed = book.has(ids.at(-1))
  const unlinked = urls.filter((url, n) => listed && !ARCHIVE_HOSTS.has(hosts[n]) && !links.has(url))
  const strangers = sorted([...links].filter(link => !urls.includes(link)))
  const draftOk = valid && !missing.length && !noLead.length
  const bookOk = !(bookMissing.length || bookNoLead.length || citations.length || process_.length || unknown.length || series.length || unlinked.length || strangers.length)
  return { ok: draftOk && bookOk, draft_ok: draftOk, book_ok: bookOk,
    missing_chapters: missing, no_lead_paragraph: noLead, sources_valid: valid,
    missing_book_chapters: bookMissing, book_no_lead_paragraph: bookNoLead,
    citations_in_book: citations, process_terms_in_book: process_, numbers_not_in_draft: unknown,
    series_in_prose: series, sources_not_linked: unlinked, links_not_in_sources: strangers,
    sources: hosts.length - snapshots, archive_snapshots: snapshots,
    sites: new Set(hosts.filter(h => !ARCHIVE_HOSTS.has(h))).size }
}

const OPTIONS = {
  init: { title: { type: 'string' }, cover: { type: 'string' }, source: { type: 'string' }, account: { type: 'string', multiple: true },
    out: { type: 'string' }, chapters: { type: 'string', default: '11' } },
  merge: {},
  slice: {},
  figures: { worklist: { type: 'boolean', default: false } },
  findings: {},
  check: { draft: { type: 'boolean', default: false } },
}
const USAGE = 'usage: case_study.mjs {init,merge,slice,figures,findings,check} ...'

function fail(message) {
  console.error(message)
  process.exit(2)
}

function main(argv) {
  const [cmd, ...rest] = argv
  if (!(cmd in OPTIONS)) fail(USAGE)
  let parsed
  try {
    parsed = parseArgs({ args: rest, options: OPTIONS[cmd], allowPositionals: true, strict: true })
  } catch (error) {
    fail(`${USAGE}\ncase_study.mjs ${cmd}: ${error.message}`)
  }
  const { values, positionals } = parsed
  const need = (count, names) => {
    if (positionals.length !== count) fail(`case_study.mjs ${cmd}: expected ${names}, got ${positionals.length} argument(s)`)
  }
  if (cmd === 'slice') {
    need(3, 'work, index, of')
    console.log(sliceSources(positionals[0], Number(positionals[1]), Number(positionals[2])).join('\n'))
    return
  }
  if (cmd === 'figures') {
    need(2, 'work, chapter')
    const result = figures(positionals[0], positionals[1], values.worklist)
    console.log(JSON.stringify({ ...result, unmatched: result.unmatched.length }))
    return
  }
  if (cmd === 'findings') {
    need(2, 'work, chapter')
    console.log(findings(positionals[0], positionals[1]).join('\n'))
    return
  }
  let result
  let passed
  if (cmd === 'init') {
    need(1, 'slug')
    for (const name of ['title', 'cover', 'source', 'out']) if (values[name] === undefined) fail(`case_study.mjs init: --${name} is required`)
    const out = values.out.replace(/^~(?=$|\/)/, homedir())
    result = init(positionals[0], values.title, values.source, out, Number(values.chapters), values.cover, values.account)
    passed = true
  } else if (cmd === 'merge') {
    need(1, 'work')
    result = merge(positionals[0])
    passed = true
  } else {
    need(1, 'work')
    result = check(positionals[0])
    passed = values.draft ? result.draft_ok : result.ok
  }
  console.log(dump(result))
  process.exit(passed ? 0 : 1)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2))
