// ABOUTME: Tests the .env loader the gate and wayback scripts read their settings from.
// ABOUTME: Covers the line format, quotes, comments, precedence of the environment and the file search order.
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, test } from 'node:test'
import { ENV_FILES, loadEnv, parseEnv } from '../scripts/env.mjs'

const tmp = mkdtempSync(join(tmpdir(), 'env-'))
after(() => rmSync(tmp, { recursive: true, force: true }))

test('parseEnv reads KEY=value lines, drops quotes, skips comments and blank lines', () => {
  assert.deepEqual(parseEnv('# proxy\nISP_PROXY_URL="http://u:p@h:8001"\n\nexport FETCH_X_POSTS=/x/y.mjs \nbad line\n'),
    { ISP_PROXY_URL: 'http://u:p@h:8001', FETCH_X_POSTS: '/x/y.mjs' })
})

test('loadEnv takes the first file that exists and never overrides a variable already set', () => {
  const a = join(tmp, 'a.env')
  const b = join(tmp, 'b.env')
  writeFileSync(b, 'ONE=b\nTWO=b\n')
  const env = { TWO: 'shell' }
  assert.equal(loadEnv([a, b], env), b)
  assert.deepEqual(env, { ONE: 'b', TWO: 'shell' })
  assert.equal(loadEnv([a], env), null, 'no file is fine')
  assert.ok(ENV_FILES[0].endsWith('/scripts/.env') && ENV_FILES[1].endsWith('/secrets-manager/profiles/case-study/.env'))
})
