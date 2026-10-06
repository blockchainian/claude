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
  assert.deepEqual(parseEnv('# proxy\nINTEL_ISP_PROXY_URL="http://u:p@h:8001"\n\nexport INTEL_CASE_STUDY_DIR=/x/y \nbad line\n'),
    { INTEL_ISP_PROXY_URL: 'http://u:p@h:8001', INTEL_CASE_STUDY_DIR: '/x/y' })
})

test('parseEnv puts a value that starts with ~/ under the home directory', () => {
  assert.deepEqual(parseEnv('INTEL_CASE_STUDY_DIR=~/x/y\nQUOTED="~/z"\nINTEL_ISP_PROXY_URL=http://h/~/a\nBARE=~\n'),
    { INTEL_CASE_STUDY_DIR: join(homedir(), 'x/y'), QUOTED: join(homedir(), 'z'), INTEL_ISP_PROXY_URL: 'http://h/~/a', BARE: '~' })
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

test('one root override groups work, fetched data and limit state without changing default locations', () => {
  assert.deepEqual(caseStudyPaths({INTEL_CASE_STUDY_DIR:'/tmp/research'}), {work:'/tmp/research',data:'/tmp/research/data',state:'/tmp/research/cache'})
  assert.deepEqual(caseStudyPaths({}), {work:join(homedir(),'Documents','case-studies'),data:join(homedir(),'.local','share','case-study'),state:join(homedir(),'.cache','case-study-limits')})
})
