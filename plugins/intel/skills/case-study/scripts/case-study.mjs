#!/usr/bin/env node
// ABOUTME: Scaffolds a case-study work dir under ~/Documents/case-studies and checks it before rendering.
// ABOUTME: init writes chapters.json + sources.json; book strips the chapters' source marks; check verifies the chapters, the book text and sources.
//
// Usage: case-study.mjs init <slug> --title <title> --cover <name> --source <url> [--account <url>]... --out <pdf> [--chapters 11]
//        case-study.mjs news <work dir> [--since <year>] <name>...   (the news lists the scouts pick press from, into raw/news/)
//        case-study.mjs merge <work dir>
//        case-study.mjs sources <work dir>   (the sources already in sources.json, as a scout would list them)
//        case-study.mjs cited <work dir>               (how many sources the chapters name)
//        case-study.mjs slice <work dir> <n> <of>      (the urls of one reviewer's slice of the cited sources, one per line)
//        case-study.mjs figures <work dir> <NN> [--worklist]   (match chapter NN's figures against the saved source text)
//        case-study.mjs quotes <work dir> <NN>         (look up chapter NN's quotations and reported words in the saved source text)
//        case-study.mjs findings <work dir> <NN>       (the review lines one fixer has still to apply to chapter NN)
//        case-study.mjs unread <work dir> <batch> <url>...   (per source of a reader's batch: done, saved, or fetch)
//        case-study.mjs bullets <work dir> <NN>        (the notes' lines for chapter NN, long dated tables thinned)
//        case-study.mjs codex <work dir> <NN> [--model gpt-6-luna]   (run the fixer prompt saved in review/fix-NN.prompt.txt on a Codex model)
//        case-study.mjs book <work dir> [--sources-title <title>]   (book/ from the chapters: the marks stripped, the cited sources listed)
//        case-study.mjs check <work dir> [--draft]
// A chapter is written once, as the text the reader gets. drafts/NN.md is that text with the marks the reviewers and
// the scripts need (a source's label in brackets in every sentence, a source's own words in ⟦ ⟧); book/NN.md is
// the same text without them, which is typeset. The introduction and the reasoning chapter are written from the
// other chapters, into book/ only.
// Agents working in parallel never share a file: each writes its own notes/<name>.sources.json (url -> label) and
// notes/<name>.gaps.md, reviewers write review/<name>.failed.json (a list of urls), fixers write
// review/<name>.added.json (url -> label). merge turns those into sources.json, gaps.md and the draft's last chapter,
// and notes/<name>.raw.tsv (a line of url, tab, file per saved text) and notes/<name>.raw.json (url -> the files its
// text was saved to) into raw.json. Sources the readers gave the same
// label get a letter each (Outlet 2025a, Outlet 2025b), in sources.json and in the notes' bullets, so that a label names one source.
import { spawn, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const PAGE_SIZE = [427.92, 660.0] // the book format digest's render typesets
const ARCHIVE_HOSTS = new Set(['web.archive.org', 'archive.org'])
// A Markdown link's target, with or without angle brackets around it.
const LINK = /\]\(<?(https?:\/\/[^)\s>]+)>?\)/g
// A parenthesis that names a source: a word, then a year that is not part of a date.
const CITATION = /[（(][^（()）]*?(?<word>[A-Za-z一-鿿][\p{L}\p{N}_.&'’-]*)\s+(?:19|20)\d\d(?!\s*年)[^（()）]*[）)（(]/gu
const DATE_WORDS = new Set(['in', 'since', 'from', 'by', 'until', 'to', 'of', 'late', 'early', 'mid', 'born', 'and', 'january',
  'february', 'march', 'april', 'may', 'june', 'july', 'august', 'september', 'october', 'november', 'december'])
// Wording that describes the research instead of the subject, and the draft's mark for a quotation's original words.
const PROCESS_TERMS = ['⟦', 'sources.json', 'notes.md', 'gaps.md', 'subagent', 'research agent', '调查 agent', 'snapshot',
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

const root = () => process.env.CASE_STUDIES_DIR || join(homedir(), 'Documents', 'case-studies')
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
  // Create <root>/<slug>/ with drafts/, book/, notes/, raw/, review/, chapters.json and an empty sources.json.
  const work = join(root(), slug)
  for (const sub of ['drafts', 'book', 'notes', 'raw', 'review']) mkdirSync(join(work, sub), { recursive: true })
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

function ownLabels(sources) {
  // url -> label where every source has a label no other source has: sources that share one get a letter after it
  // (aa, ab… when more than 26 share it, so that no label is the start of another).
  const sharing = {}
  for (const [url, label] of Object.entries(sources)) (sharing[label] ??= []).push(url)
  const letters = (n, of) => {
    let width = 1
    while (26 ** width < of) width += 1
    let out = ''
    for (let i = 0; i < width; i += 1, n = Math.floor(n / 26)) out = String.fromCharCode(97 + (n % 26)) + out
    return out
  }
  return Object.fromEntries(Object.entries(sources).map(([url, label]) => {
    const urls = sharing[label]
    return [url, urls.length > 1 ? label + letters(urls.indexOf(url), urls.length) : label]
  }))
}

function nameInNotes(notes, sources, own) {
  // In the readers' notes, end each bullet under a source's `url:` line with that source's own label.
  for (const path of files(notes, '.md')) {
    let url = ''
    const text = read(path)
    const named = lines(text).map(line => {
      if (line.startsWith('url:')) url = line.slice(4).trim()
      const shared = `— ${sources[url]}`
      return line.startsWith('- ') && own[url] !== sources[url] && line.trimEnd().endsWith(shared)
        ? line.trimEnd().slice(0, -shared.length) + `— ${own[url]}` : line
    }).join('\n')
    if (named !== text) write(path, named)
  }
}

function savedByReader(path) {
  // url -> the files its text was saved to, from a reader's list of `url<TAB>file` lines; empty when there is no list.
  const saved = {}
  if (!existsSync(path)) return saved
  for (const line of lines(read(path))) {
    const [url, file] = line.split('\t').map(part => part.trim())
    if (url && file) (saved[url] ??= []).push(file)
  }
  return saved
}

// One gate.mjs command, run without blocking: resolves with its status and its output once it ends.
function runGate(args) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [fileURLToPath(new URL('./gate.mjs', import.meta.url)), ...args])
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', chunk => { stdout += chunk })
    child.stderr.on('data', chunk => { stderr += chunk })
    child.on('close', status => resolve({ status, stdout, stderr }))
  })
}

// The news lists the scouts pick press from, fetched once for the whole study: Google News for each name and GDELT
// for all of them, all at once, from the first day of the year `since` (without it, the commands' own first day) to
// today, into raw/news/. Returns per file the articles it holds, or why its command failed.
export async function news(work, since, names, run = runGate) {
  // A URL or a description in the list would fail GDELT for every name: refused before anything is asked.
  const wrong = names.filter(name => /:\/\/|[,()]/.test(name))
  if (wrong.length) throw Error(`not a name: ${wrong.join(' | ')}; give each name the subject goes by on its own`)
  const dir = join(work, 'raw', 'news')
  mkdirSync(dir, { recursive: true })
  const from = since ? [`${since}-01-01`] : []
  const jobs = [...names.map(name => [`gnews-${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}.jsonl`, ['gnews', name, ...from]]),
    ['gdelt.jsonl', ['gdelt', ...names, ...from]]]
  return Object.fromEntries(await Promise.all(jobs.map(async ([file, args]) => {
    const { status, stdout, stderr } = await run(args)
    writeFileSync(join(dir, file), stdout)
    const why = stderr.trim().split('\n').at(-1)
    // gate.mjs exits 3 when GDELT's free quota of the month is used up: GDELT is then a gap, as the tool list says
    return [file, status === 0 ? stdout.split('\n').filter(Boolean).length : status === 3 ? `GAP: ${why}` : `FAILED: ${why}`]
  })))
}

// Whether a list the news command fetched failed; a gap (GDELT out of quota) is not a failure.
export const newsFailed = result => Object.values(result).some(v => String(v).startsWith('FAILED'))

export function unread(work, batch, urls) {
  // What an interrupted reader has left of its batch, one line per source: `done` when the notes have its section,
  // `saved` with the files when its text was saved but not read into the notes, `fetch` otherwise.
  const notes = join(work, 'notes', `${batch}.md`)
  const noted = new Set(existsSync(notes) ? lines(read(notes)).filter(l => l.startsWith('url:')).map(l => l.slice(4).trim()) : [])
  const saved = savedByReader(join(work, 'notes', `${batch}.raw.tsv`))
  return urls.map(url => (noted.has(url) ? `done ${url}` : saved[url] ? `saved ${url} ${saved[url].join(' ')}` : `fetch ${url}`))
}

export function merge(work) {
  // Rebuild sources.json, gaps.md and the draft's sources chapter from the files the parallel agents wrote.
  const meta = JSON.parse(read(join(work, 'chapters.json')))
  const notes = join(work, 'notes')
  const review = join(work, 'review')
  let sources = {}
  const gather = paths => {
    for (const path of paths) for (const [k, v] of Object.entries(readJson(path, 'object'))) if (typeof v === 'string') sources[k] = v
  }
  gather(files(notes, '.sources.json'))
  const own = ownLabels(sources)
  nameInNotes(notes, sources, own)
  sources = own
  gather(files(review, '.added.json'))
  const failed = new Set(files(review, '.failed.json').flatMap(path => readJson(path, 'array')))
  sources = Object.fromEntries(Object.entries(sources).filter(([url]) => !failed.has(url)))
  write(join(work, 'sources.json'), dump(sources))
  const saved = {}
  const lists = [...files(notes, '.raw.tsv').map(savedByReader), ...[...files(notes, '.raw.json'), ...files(review, '.raw.json')].map(path => readJson(path, 'object'))]
  for (const listed of lists) {
    for (const [url, list] of Object.entries(listed)) {
      saved[url] ??= []
      for (const f of typeof list === 'string' ? [list] : list) if (!saved[url].includes(f)) saved[url].push(f)
    }
  }
  write(join(work, 'raw.json'), dump(saved))
  const gaps = files(notes, '.gaps.md').map(path => read(path).trim()).filter(Boolean)
  write(join(work, 'gaps.md'), gaps.join('\n') + '\n')
  const labels = sorted(new Set(Object.entries(sources).map(([url, label]) => `${label} (${host(url)})`)))
  const last = join(work, 'drafts', `${meta.chapters.at(-1).id}.md`)
  write(last, `# Sources\n\n${Object.keys(sources).length} sources, listed in sources.json.\n\n## List\n\n` + labels.map(l => `- ${l}`).join('\n') + '\n')
  const malformed = [...files(notes, '.json'), ...files(review, '.added.json'), ...files(review, '.raw.json')]
    .filter(path => Object.keys(readJson(path, 'object')).length === 0 && !['', '{}'].includes(read(path).trim()))
    .map(path => relative(work, path))
  return { sources: Object.keys(sources).length, failed: failed.size, gaps: gaps.reduce((n, g) => n + g.split('\n').length, 0), malformed }
}

const chapterIds = work => JSON.parse(read(join(work, 'chapters.json'))).chapters.map(chapter => chapter.id)

export function cited(work) {
  // url -> label of the sources a chapter names. The draft's closing list names every source and does not count.
  const text = chapterIds(work).slice(0, -1).map(id => join(work, 'drafts', `${id}.md`)).filter(path => existsSync(path)).map(read).join('\n')
  return Object.fromEntries(Object.entries(readJson(join(work, 'sources.json'), 'object')).filter(([, label]) => typeof label === 'string' && text.includes(label)))
}

export function sliceSources(work, index, of) {
  // Slice number index (from 1) of the urls of the cited sources, sorted, cut into `of` near-equal parts.
  const urls = sorted(Object.keys(cited(work)))
  const size = Math.ceil(urls.length / of)
  return urls.slice((index - 1) * size, index * size)
}

export function listSources(work) {
  // The sources already in sources.json, in the shape the scouts return, for a run that reads them again without scouting.
  const sources = readJson(join(work, 'sources.json'), 'object')
  return { sources: sorted(Object.keys(sources)).map(url => ({ url, outlet: sources[url] })) }
}

export function findings(work, chapter) {
  // The review lines a fixer has still to apply to one chapter: those tagged with its number, and the sources-lens
  // lines (tagged with a source label) about labels the chapter names. Each line carries a name made from its own
  // text (F and six hex digits), the same on every call; a finding whose name is in the chapter's fix log is left out.
  const text = read(join(work, 'drafts', `${chapter}.md`))
  const log = join(work, 'review', `fix-${chapter}.md`)
  const logged = new Set(existsSync(log) ? read(log).match(/\bF[0-9a-f]{6}\b/g) : [])
  const out = []
  for (const path of files(join(work, 'review'), '.md')) {
    const fromSources = path.split('/').at(-1).startsWith('sources-')
    for (const line of lines(read(path))) {
      const tag = /^- \[([^\]]+)\]/.exec(line)
      if (!tag || !(tag[1] === chapter || (fromSources && text.includes(tag[1])))) continue
      const name = 'F' + createHash('sha1').update(line).digest('hex').slice(0, 6)
      if (!logged.has(name)) out.push(`- ${name} ${line.slice(2)}`)
    }
  }
  return out
}

const DATED_ROW = /^(?:\[[^\]]+\]\s*)+((?:19|20)\d\d)-(\d\d)-\d\d\b/

export function bullets(work, chapter) {
  // The lines of the notes tagged for one chapter, in file order. In the numbers notes each `##` section is headed by
  // its title, and a section's dated rows are thinned to the first of each quarter and the last, with a line saying
  // how many rows stayed behind in the file. The bullets of a source the reviewers failed are left out: no sentence
  // may rest on it.
  const tag = `[c${chapter}]`
  const failed = new Set(files(join(work, 'review'), '.failed.json').flatMap(path => readJson(path, 'array')))
  const out = []
  for (const path of files(join(work, 'notes'), '.md')) {
    const name = relative(work, path)
    if (!name.startsWith('notes/numbers-')) {
      let url = ''
      for (const line of lines(read(path))) {
        if (line.startsWith('url:')) url = line.slice(4).trim()
        if (line.includes(tag) && !failed.has(url)) out.push(line)
      }
      continue
    }
    for (const section of read(path).split(/^(?=## )/m)) {
      const tagged = lines(section).filter(line => line.includes(tag) && !line.startsWith('## '))
      if (!tagged.length) continue
      const dated = tagged.filter(line => DATED_ROW.test(line))
      const quarters = new Set()
      const kept = new Set(dated.filter(line => {
        const [, year, month] = DATED_ROW.exec(line)
        const quarter = `${year}-${Math.ceil(Number(month) / 3)}`
        return !quarters.has(quarter) && quarters.add(quarter)
      }))
      if (dated.length) kept.add(dated.at(-1))
      if (section.startsWith('## ')) out.push(lines(section)[0])
      out.push(...tagged.filter(line => !DATED_ROW.test(line) || kept.has(line)))
      if (dated.length > kept.size) out.push(`(${dated.length - kept.size} more rows of this table are in ${name})`)
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

const MAX_TEXT = 8e6 // a saved file larger than this is a video or a scan, not text to match against
const OPENS = /[“「⟦]/g
const CLOSES = /[”」⟧]/g
const STRAIGHT = /"/g
const count = (text, re) => (text.match(re) || []).length

function sentencesOf(line) {
  // A line cut into sentences. A full stop inside a quotation, or inside the original words after one, ends nothing.
  const out = []
  let open = ''
  for (const piece of line.split(/(?<=[。！？!?])|(?<=\.)\s+(?=[A-Z])/)) {
    open += piece
    if (count(open, OPENS) > count(open, CLOSES) || count(open, STRAIGHT) % 2) continue
    out.push(open)
    open = ''
  }
  if (open) out.push(open)
  return out
}

function sourced(work, chapter) {
  // A chapter as its sentences, each with the labels of the sources it rests on: the ones it names, else the
  // ones its paragraph names (for a chart, the paragraph after it), else the ones the chapter names (a lead or a
  // summary restates the chapter's own claims);
  // `named` says the sentence names them itself.
  // With them: every label, longest first; the labels a text names; the saved texts of a label's sources; and
  // a label's urls.
  const labels = new Map() // label -> urls, in sources.json's order (an object would move a label that looks like an integer first)
  for (const [url, label] of Object.entries(readJson(join(work, 'sources.json'), 'object'))) labels.set(label, [...(labels.get(label) || []), url])
  const saved = readJson(join(work, 'raw.json'), 'object')
  const isText = path => existsSync(path) && statSync(path).isFile() && statSync(path).size <= MAX_TEXT
  const cache = {}
  const texts = label => (cache[label] ??= labels.get(label).flatMap(url => (saved[url] || []))
    .filter(file => isText(join(work, file))).map(file => ({ file, text: read(join(work, file)) })))
  const ordered = [...labels.keys()].sort((a, b) => b.length - a.length)
  const named = text => ordered.filter(label => text.includes(label))
  const body = read(join(work, 'drafts', `${chapter}.md`))
  const anywhere = named(body)
  const paragraphs = paragraphsOf(body)
  // A label in brackets right after a full stop belongs to the sentence before it in a chapter whose paragraphs
  // end on a label, and to the sentence after it in a chapter whose paragraphs open with one.
  const having = re => paragraphs.filter(p => re.test(p)).length
  const labelsFollow = having(LABEL_LAST) > having(LABEL_FIRST)
  const sentences = []
  for (let [n, paragraph] of paragraphs.entries()) {
    if (paragraph.trimStart().startsWith('#')) continue
    if (labelsFollow) paragraph = paragraph.replace(LABEL_AFTER_STOP, (whole, stop, label) => (named(label).length ? label + stop : whole))
    // A chart's rows name no source: the paragraph after the chart does.
    const around = paragraph.trimStart().startsWith('```chart') ? named(paragraphs[n + 1] || '') : named(paragraph)
    for (const sentence of lines(paragraph).flatMap(sentencesOf)) {
      const own = named(sentence)
      sentences.push({ sentence, cited: [own, around, anywhere].find(list => list.length) || [], named: own.length > 0 })
    }
  }
  return { sentences, ordered, anywhere, texts, urls: label => labels.get(label) }
}

export function figures(work, chapter, worklist = false) {
  // Match every figure in a draft chapter against the saved text of the sources its sentence names, and write
  // the ones that are not there as findings for the chapter's fixer, or, as a worklist, for a reviewer to judge.
  const { sentences, ordered, texts } = sourced(work, chapter)
  const figuresOf = {}
  const known = label => {
    // {values, digits} of the figures in everything saved for a label's sources.
    if (!(label in figuresOf)) {
      const found = amounts(texts(label).map(saved => saved.text).join('\n'))
      figuresOf[label] = { values: new Set(found.flatMap(f => f.values)), digits: new Set(found.filter(f => f.digits.length >= 4).map(f => f.digits)) }
    }
    return figuresOf[label]
  }
  let checked = 0
  let small = 0
  const unmatched = []
  const out = []
  for (const { sentence, cited } of sentences) {
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
  mkdirSync(join(work, 'review'), { recursive: true })
  write(join(work, 'review', `figures-${chapter}.md`),
    `# Figures in drafts/${chapter}.md matched against the saved text of their sources\n\n` +
    `${checked} checked, ${checked - unmatched.length} matched, ${unmatched.length} not matched, ${small} too small to match (left to the quotes lens).\n\n` +
    out.join('\n') + '\n')
  return { chapter, checked, matched: checked - unmatched.length, small, unmatched }
}

// A quotation, and the source's own words right after it when the quotation is a translation; or the source's own
// words alone, after reported speech.
const QUOTATION = /[“"「]([^“”"「」\n]{2,}?)[”"」](?:\s*⟦([^⟧\n]+)⟧)?|⟦([^⟧\n]+)⟧/g
const OMISSION = /…+|\.{3,}|\[[^\]]*\]/
const LETTER = /[\p{L}\p{N}]/u
const DISTINCT = 15 // letters: words shorter than this turn up in sources that never said them
const AROUND = 200 // characters of the source shown on each side of the words found
const BETWEEN = 4 // words a source may have between two of the words looked up: a filler, a caption's timing

function plain(text) {
  // A text's letters and digits in lower case, every run of anything else as one space, apostrophes dropped, markup,
  // link addresses and caption timings blanked; `at` holds each kept character's place in the text.
  const blank = whole => ' '.repeat(whole.length)
  const clean = text.replace(/<[^>]*>/g, blank).replace(/^.*-->.*$/gm, blank).replace(/(?<=\])\([^()\s]*\)/g, blank)
    .replace(/&(?:#39|#x27|apos|rsquo|lsquo|#8217|#8216);/gi, whole => "'".padEnd(whole.length, '\0'))
    .replace(/&(?:quot|amp|nbsp|ldquo|rdquo|#\d+|#x[0-9a-f]+);/gi, blank)
  const out = []
  const at = []
  for (let i = 0; i < clean.length; i += 1) {
    const ch = clean[i]
    if (LETTER.test(ch)) {
      const lower = ch.toLowerCase()
      out.push(lower.length === 1 ? lower : ch)
      at.push(i)
    } else if (!"'’‘\0".includes(ch) && out.length && out.at(-1) !== ' ') {
      out.push(' ')
      at.push(i)
    }
  }
  return { clean, text: out.join(''), at }
}

function spanApart(piece, source, from) {
  // Where a piece's words stand in the source in order, with at most BETWEEN other words between two of them:
  // [start, end) in the source's plain text, or null.
  source.words ??= [...source.text.matchAll(/\S+/g)].map(found => ({ word: found[0], at: found.index }))
  const wanted = piece.split(' ')
  const { words } = source
  for (let start = words.findIndex(w => w.at >= from); start >= 0 && start < words.length; start += 1) {
    if (words[start].word !== wanted[0]) continue
    let here = start
    const whole = wanted.slice(1).every(word => {
      const next = words.slice(here + 1, here + 2 + BETWEEN).findIndex(w => w.word === word)
      here += next + 1
      return next >= 0
    })
    if (whole) return [words[start].at, words[here].at + words[here].word.length]
  }
  return null
}

function passageOf(words, source, apart = false) {
  // The source's text around the words of a quotation, its pieces (the parts between omissions) found in order;
  // null when a piece is not there. With `apart`, a piece's words may have a few others between them.
  let from = 0
  let first = -1
  let last = -1
  for (const piece of words.split(OMISSION).map(part => plain(part).text.trim()).filter(Boolean)) {
    const at = source.text.indexOf(piece, from)
    const span = at >= 0 ? [at, at + piece.length] : apart && spanApart(piece, source, from)
    if (!span) return null
    if (first < 0) first = span[0]
    from = span[1]
    last = from - 1
  }
  if (first < 0) return null
  return source.clean.slice(Math.max(0, source.at[first] - AROUND), source.at[last] + 1 + AROUND).replace(/[\s\0]+/g, ' ').trim()
}

const DATED_HEADING = /[（(]([^()（）]*(?:19|20)\d\d[^()（）]*)[)）]\s*$/

function datesOf(work) {
  // url -> the date a source's notes heading ends with (`## <Outlet> — <title> (<date>)`, its `url:` line under it).
  const dates = {}
  for (const path of files(join(work, 'notes'), '.md')) {
    let date = ''
    for (const line of lines(read(path))) {
      if (line.startsWith('## ')) date = (DATED_HEADING.exec(line) || [])[1] || ''
      else if (line.startsWith('url:') && date) dates[line.slice(4).trim()] = date
    }
  }
  return dates
}

export function quotes(work, chapter) {
  // Look up every quotation of a draft chapter in the saved text of the sources its sentence names, by the original
  // words after it when it is a translation, and the source's words after reported speech the same way, and write
  // a reviewer's worklist: per quotation, whether the words are there and the passage around them, or the other
  // source whose text has them; then the sentences that name a source and carry none of its words; then every
  // source the chapter names, with its date and saved files.
  const { sentences, ordered, anywhere, texts, urls } = sourced(work, chapter)
  const plains = {}
  const sources = label => (plains[label] ??= texts(label).map(saved => ({ file: saved.file, ...plain(saved.text) })))
  const lookUp = (words, labels, apart = false) => {
    for (const label of labels) {
      for (const source of sources(label)) {
        const passage = passageOf(words, source, apart)
        if (passage) return { label, file: source.file, passage, apart }
      }
    }
    return null
  }
  const rows = []
  const wordless = []
  const out = []
  for (const { sentence, cited, named } of sentences) {
    const found = [...sentence.matchAll(QUOTATION)]
    if (!found.length) {
      if (named) wordless.push({ sentence: sentence.trim(), labels: cited })
      continue
    }
    out.push('', `## ${sentence.trim()}`)
    for (const [, quote = '', original, reported] of found) {
      const looked = (reported || original || quote).trim()
      const row = { quote, looked, labels: cited, sentence: sentence.trim(), verdict: 'not found' }
      const distinct = plain(looked).text.length >= DISTINCT
      const here = lookUp(looked, cited) || (distinct && lookUp(looked, cited, true))
      const there = here || (distinct && lookUp(looked, ordered.filter(label => !cited.includes(label))))
      if (here) Object.assign(row, { verdict: 'found', ...here })
      else if (there) Object.assign(row, { verdict: 'in another source', ...there })
      else if (!cited.some(label => sources(label).length)) row.verdict = 'no saved text'
      rows.push(row)
      out.push(`* ${row.verdict}${row.apart ? ', with other words between' : ''} | ${looked} | ${row.file ? `${row.label}, ${row.file}` : named ? cited.join(', ') : 'its sentence names no source'}${row.passage ? ` | ${row.passage}` : ''}`)
    }
  }
  const tally = verdict => rows.filter(row => row.verdict === verdict).length
  const dates = datesOf(work)
  const result = { chapter, quotations: rows.length, found: tally('found'), elsewhere: tally('in another source'), missing: tally('not found'), unsaved: tally('no saved text') }
  mkdirSync(join(work, 'review'), { recursive: true })
  write(join(work, 'review', `quotations-${chapter}.md`),
    `# Quotations in drafts/${chapter}.md looked up in the saved text of their sources\n\n` +
    `${result.quotations} quotations: ${result.found} found, ${result.elsewhere} in another source, ${result.missing} not found, ${result.unsaved} with no saved text.\n` +
    'A row is: verdict | the words looked up | the source and file they are in, or the sources the sentence names | the passage around them.\n' +
    `"with other words between": the source has the words in this order with up to ${BETWEEN} others between two of them.\n` +
    'The words looked up are a quotation, the source\'s words in ⟦ ⟧ after a translated one, or the source\'s words in ⟦ ⟧ after reported speech.\n' +
    out.join('\n') + '\n\n# Sentences that name a source and carry none of its words\n\n' +
    wordless.map(({ sentence, labels }) => `- ${sentence} | ${labels.join(', ')}`).join('\n') +
    '\n\n# Every source this chapter names: its date in the notes, and its saved text\n\n' +
    anywhere.map(label => {
      const dated = [...new Set(urls(label).map(url => dates[url]).filter(Boolean))].join(', ')
      return `- ${label}${dated ? ` (${dated})` : ''}: ${sources(label).map(source => source.file).join(', ') || 'none saved'}`
    }).join('\n') + '\n')
  return { ...result, rows, wordless }
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

const UNIT_SHIFTS = Array.from({ length: 19 }, (_, i) => i - 9)

export function restated(number, knownValues) {
  // True when the figure is a known one in another unit: 24.8M as 2,480 万, 1.2B as 12 亿, 179,000,000 as 1.79 亿.
  // The largest unit is a billion, so the two differ by at most nine powers of ten.
  const value = Number(number)
  const tolerance = 1e-9 * Math.max(value, 1)
  return knownValues.some(k => UNIT_SHIFTS.some(e => Math.abs(value - k * 10 ** e) <= tolerance))
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

// The fixer runs on a Codex model: the fix is a bounded edit under findings a script lists, and the pilot on
// brooke-monk chapter 04 showed gpt-6-luna applies them as Sonnet does at a fraction of the cost. The prompt is
// the one creator.mjs gives a fixer, saved to review/fix-NN.prompt.txt by the agent that runs this command.
// The gate script paces its Google calls through files in ~/.cache/case-study-limits, which the Codex sandbox
// must be allowed to write.
export function codexCommand(work, chapter, model) {
  const prompt = readFileSync(join(work, 'review', `fix-${chapter}.prompt.txt`), 'utf8')
  const limits = join(homedir(), '.cache', 'case-study-limits')
  return ['codex', 'exec', '--skip-git-repo-check', '--json', '-m', model, '-c', 'model_reasoning_effort=high',
    '-c', `sandbox_workspace_write.writable_roots=${JSON.stringify([limits])}`, prompt]
}

// The last message of a codex exec run and its token usage, from the JSON event stream it printed.
export function codexResult(events) {
  let message = ''
  let usage = null
  for (const line of events.split('\n')) {
    if (!line) continue
    let event
    try { event = JSON.parse(line) } catch { continue }
    if (event.type === 'item.completed' && event.item?.type === 'agent_message') message = event.item.text
    if (event.type === 'turn.completed' && event.usage) usage = event.usage
  }
  return { message, usage }
}

// A fixer can stop with findings it never reached (a refused tool, a cut-off turn) and still exit 0: `remaining`
// counts the findings of the chapter that have no line in its fix log after the run.
export function codex(work, chapter, model, spawn = spawnSync) {
  const [command, ...args] = codexCommand(work, chapter, model)
  mkdirSync(join(work, 'review'), { recursive: true })
  const run = spawn(command, args, { cwd: work, stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8', maxBuffer: 1 << 28 })
  writeFileSync(join(work, 'review', `fix-${chapter}.codex.jsonl`), run.stdout ?? '', 'utf8')
  const result = codexResult(run.stdout ?? '')
  return { ...result, remaining: findings(work, chapter).length, status: run.status, stderr: (run.stderr ?? '').split('\n').filter(l => l && !l.startsWith('Reading additional input')).join('\n') }
}

const BRACKETED = /[ \t]*[〔（(\[][^〔〕（）()\[\]]*[〕）)\]]/g
const ORIGINAL_WORDS = /[ \t]*⟦[^⟧\n]*⟧/g
const YEAR = /(?<![\p{L}\p{N}])(?:19|20)\d\d/u

export function book(work, sourcesTitle = 'Sources') {
  // Write book/ from the chapters: each chapter without its marks (a bracket that names a source goes with all it
  // holds, and so do the source's words in ⟦ ⟧; a chart block is left as written), and the closing list of the
  // sources the chapters name, an outlet a line with its articles linked by year. A chapter with no draft (the
  // introduction, the reasoning chapter) is left as it is in book/.
  const ids = chapterIds(work)
  // A label may hold brackets of its own: each label stands as a mark while the brackets around it are looked for.
  const labels = [...new Set(Object.values(readJson(join(work, 'sources.json'), 'object')))].sort((a, b) => b.length - a.length)
  const mark = n => `\u0000${n}\u0001`
  const marked = text => labels.reduce((out, label, n) => out.replaceAll(label, mark(n)), text)
  const unmarked = text => text.replace(/\u0000(\d+)\u0001/g, (_, n) => labels[Number(n)])
  const strip = text => text.split(/(^```chart[ \t]*\n[\s\S]*?\n```[ \t]*$)/m)
    .map((part, n) => (n % 2 ? part : unmarked(marked(part.replace(ORIGINAL_WORDS, '')).replace(BRACKETED, group => (group.includes('\u0000') ? '' : group))))).join('')
  mkdirSync(join(work, 'book'), { recursive: true })
  const chapters = ids.slice(0, -1).filter(id => existsSync(join(work, 'drafts', `${id}.md`)))
  for (const id of chapters) write(join(work, 'book', `${id}.md`), strip(read(join(work, 'drafts', `${id}.md`))))
  const outlets = new Map() // outlet -> its links, in label order
  const named = cited(work)
  for (const [url, label] of Object.entries(named).sort(([, a], [, b]) => (a < b ? -1 : a > b ? 1 : 0))) {
    const year = label.search(YEAR)
    const outlet = year > 0 ? label.slice(0, year).trim() : ''
    const link = `[${year > 0 ? label.slice(year) : label}](<${url}>)`
    if (outlet) outlets.set(outlet, [...(outlets.get(outlet) || []), link])
    else outlets.set(label, [link])
  }
  const list = [...outlets].map(([outlet, links]) => (links.length === 1 && links[0].startsWith(`[${outlet}]`) ? `- ${links[0]}` : `- ${outlet}: ${links.join(', ')}`))
  write(join(work, 'book', `${ids.at(-1)}.md`), `# ${sourcesTitle}\n\n${list.join('\n')}\n`)
  return { chapters, sources: Object.keys(named).length }
}

export function check(work) {
  // Report what blocks the review (the chapters, sources.json) and what blocks rendering (the book text).
  const ids = chapterIds(work)
  // The introduction and the reasoning chapter are written from the other chapters, into book/ only.
  const bookOnly = [ids[0], ids.at(-2)]
  const { missing: absent, noLead, texts: draft } = layer(work, 'drafts', ids)
  const missing = absent.filter(id => !bookOnly.includes(id))
  const { missing: bookMissing, noLead: bookNoLeads, texts: book } = layer(work, 'book', ids)
  const bookNoLead = bookNoLeads.filter(id => id !== ids.at(-1)) // the closing list is a list
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
  const named = valid ? Object.keys(cited(work)) : []
  const unlinked = named.filter(url => listed && !ARCHIVE_HOSTS.has(host(url)) && !links.has(url))
  const strangers = sorted([...links].filter(link => !urls.includes(link)))
  // The closing sources list has no fixer; a chapter not written yet has no findings to apply.
  const unapplied = {}
  for (const id of ids.slice(0, -1)) {
    const left = draft.has(id) ? findings(work, id).length : 0
    if (left) unapplied[id] = left
  }
  const draftOk = valid && !missing.length && !noLead.length && !Object.keys(unapplied).length
  const bookOk = !(bookMissing.length || bookNoLead.length || citations.length || process_.length || unknown.length || series.length || unlinked.length || strangers.length)
  return { ok: draftOk && bookOk, draft_ok: draftOk, book_ok: bookOk,
    missing_chapters: missing, no_lead_paragraph: noLead, sources_valid: valid, findings_unapplied: unapplied,
    missing_book_chapters: bookMissing, book_no_lead_paragraph: bookNoLead,
    citations_in_book: citations, process_terms_in_book: process_, numbers_not_in_draft: unknown,
    series_in_prose: series, sources_not_linked: unlinked, links_not_in_sources: strangers,
    sources: hosts.length - snapshots, archive_snapshots: snapshots, sources_cited: named.length,
    sites: new Set(hosts.filter(h => !ARCHIVE_HOSTS.has(h))).size }
}

const OPTIONS = {
  init: { title: { type: 'string' }, cover: { type: 'string' }, source: { type: 'string' }, account: { type: 'string', multiple: true },
    out: { type: 'string' }, chapters: { type: 'string', default: '11' } },
  news: { since: { type: 'string' } },
  merge: {},
  cited: {},
  slice: {},
  book: { 'sources-title': { type: 'string', default: 'Sources' } },
  sources: {},
  figures: { worklist: { type: 'boolean', default: false } },
  quotes: {},
  findings: {},
  unread: {},
  bullets: {},
  codex: { model: { type: 'string', default: 'gpt-6-luna' } },
  check: { draft: { type: 'boolean', default: false } },
}
const USAGE = 'usage: case-study.mjs {init,merge,cited,slice,sources,figures,quotes,findings,unread,bullets,codex,book,check} ...'

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
    fail(`${USAGE}\ncase-study.mjs ${cmd}: ${error.message}`)
  }
  const { values, positionals } = parsed
  const need = (count, names) => {
    if (positionals.length !== count) fail(`case-study.mjs ${cmd}: expected ${names}, got ${positionals.length} argument(s)`)
  }
  if (cmd === 'news') {
    if (positionals.length < 2) fail('case-study.mjs news: expected the work directory and at least one name')
    news(positionals[0], values.since, positionals.slice(1)).then(result => {
      console.log(JSON.stringify(result))
      // A list that failed fails the command: the scouts are not to start on part of the news
      if (newsFailed(result)) process.exit(1)
    }, error => fail(`case-study.mjs news: ${error.message}`))
    return
  }
  if (cmd === 'cited') {
    need(1, 'work')
    console.log(JSON.stringify({ cited: Object.keys(cited(positionals[0])).length }))
    return
  }
  if (cmd === 'book') {
    need(1, 'work')
    console.log(JSON.stringify(book(positionals[0], values['sources-title'])))
    return
  }
  if (cmd === 'slice') {
    need(3, 'work, index, of')
    console.log(sliceSources(positionals[0], Number(positionals[1]), Number(positionals[2])).join('\n'))
    return
  }
  if (cmd === 'sources') {
    need(1, 'work')
    console.log(JSON.stringify(listSources(positionals[0])))
    return
  }
  if (cmd === 'figures') {
    need(2, 'work, chapter')
    const result = figures(positionals[0], positionals[1], values.worklist)
    console.log(JSON.stringify({ ...result, unmatched: result.unmatched.length }))
    return
  }
  if (cmd === 'quotes') {
    need(2, 'work, chapter')
    const { rows, wordless, ...result } = quotes(positionals[0], positionals[1])
    console.log(JSON.stringify(result))
    return
  }
  if (cmd === 'findings' || cmd === 'bullets') {
    need(2, 'work, chapter')
    console.log((cmd === 'findings' ? findings : bullets)(positionals[0], positionals[1]).join('\n'))
    return
  }
  if (cmd === 'codex') {
    need(2, 'work, chapter')
    const result = codex(positionals[0], positionals[1], values.model)
    console.log(dump(result))
    process.exit(result.status === 0 && result.remaining === 0 ? 0 : 1)
  }
  if (cmd === 'unread') {
    if (positionals.length < 3) fail('case-study.mjs unread: expected work, batch and the batch\'s urls')
    console.log(unread(positionals[0], positionals[1], positionals.slice(2)).join('\n'))
    return
  }
  let result
  let passed
  if (cmd === 'init') {
    need(1, 'slug')
    for (const name of ['title', 'cover', 'source', 'out']) if (values[name] === undefined) fail(`case-study.mjs init: --${name} is required`)
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

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) main(process.argv.slice(2))
