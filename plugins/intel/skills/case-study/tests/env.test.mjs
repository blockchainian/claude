// ABOUTME: Tests the .env loader the gate and wayback scripts read their settings from.
// ABOUTME: Covers the line format, quotes, comments, precedence of the environment and the file search order.
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, test } from 'node:test'
import { ENV_FILES, loadEnv, parseEnv, caseStudyPaths } from '../scripts/env.mjs'

const tmp = mkdtempSync(join(tmpdir(), 'env-'))
after(() => rmSync(tmp, { recursive: true, force: true }))

test('parseEnv reads KEY=value lines, drops quotes, skips comments and blank lines', () => {
  assert.deepEqual(parseEnv('# proxy\nISP_PROXY_URL="http://u:p@h:8001"\n\nexport INTEL_OUTPUT_DIR=/x/y \nbad line\n'),
    { ISP_PROXY_URL: 'http://u:p@h:8001', INTEL_OUTPUT_DIR: '/x/y' })
})

test('parseEnv puts a value that starts with ~/ under the home directory', () => {
  assert.deepEqual(parseEnv('INTEL_OUTPUT_DIR=~/x/y\nQUOTED="~/z"\nISP_PROXY_URL=http://h/~/a\nBARE=~\n'),
    { INTEL_OUTPUT_DIR: join(homedir(), 'x/y'), QUOTED: join(homedir(), 'z'), ISP_PROXY_URL: 'http://h/~/a', BARE: '~' })
})

test('loadEnv takes the first file that exists and never overrides a variable already set', () => {
  const a = join(tmp, 'a.env')
  const b = join(tmp, 'b.env')
  writeFileSync(b, 'ONE=b\nTWO=b\n')
  const env = { TWO: 'shell' }
  assert.equal(loadEnv([a, b], env), b)
  assert.deepEqual(env, { ONE: 'b', TWO: 'shell' })
  assert.equal(loadEnv([a], env), null, 'no file is fine')
  assert.deepEqual(ENV_FILES, [join(homedir(), '.config', 'intel', '.env')])
})

test('work, fetched news and rate limits live under the state root, the PDFs under the output root', () => {
  assert.deepEqual(caseStudyPaths({INTEL_STATE_DIR:'/tmp/state',INTEL_DATA_DIR:'/tmp/data',INTEL_OUTPUT_DIR:'/tmp/output'}), {work:'/tmp/state/case-study/work',data:'/tmp/state/case-study/news',state:'/tmp/state/limits',output:'/tmp/output/case-studies'})
  const root = join(homedir(),'.local','state','intel')
  assert.deepEqual(caseStudyPaths({}), {work:join(root,'case-study','work'),data:join(root,'case-study','news'),state:join(root,'limits'),output:join(homedir(),'Documents','case-studies')})
})
