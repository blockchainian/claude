// ABOUTME: Loads the skill's settings from a .env file into process.env, for the scripts that call outside services.
// ABOUTME: Settings are shared by Intel at ~/.config/intel/.env.
import { existsSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { parseEnv as parseDotEnv } from 'node:util'
import { join } from 'node:path'
import { stateDir, limitsDir } from '../../fetch-x-mentions/scripts/env.mjs'

export const ENV_FILES = [join(homedir(), '.config', 'intel', '.env')]

// KEY=value lines; quotes around the value are dropped; a value that starts with ~/ is under the home directory;
// a variable already in the environment wins.
export function parseEnv(text) {
  return Object.fromEntries(Object.entries(parseDotEnv(text)).map(([key, value]) => [key, value.replace(/^~(?=\/)/, homedir())]))
}

export function loadEnv(files = ENV_FILES, env = process.env) {
  const file = files.find(f => existsSync(f))
  if (!file) return null
  for (const [key, value] of Object.entries(parseEnv(readFileSync(file, 'utf8')))) if (!(key in env)) env[key] = value
  return file
}

// Work directories and fetched news are costly to rebuild, so both live under the state root; the rate-limit
// state is shared with fetch-x-posts. The PDF goes wherever --out says.
export function caseStudyPaths(env = process.env) {
  return {
    work: join(stateDir(env), 'case-study', 'work'),
    data: join(stateDir(env), 'case-study', 'news'),
    state: limitsDir(env),
  }
}
