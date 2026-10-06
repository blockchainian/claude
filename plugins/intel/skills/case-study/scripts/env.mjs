// ABOUTME: Loads the skill's settings from a .env file into process.env, for the scripts that call outside services.
// ABOUTME: Settings are shared by Intel at ~/.config/intel/.env.
import { existsSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { parseEnv as parseDotEnv } from 'node:util'
import { join } from 'node:path'

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

// A configured root groups working files, fetched data and rate-limit state. Existing defaults stay in place.
export function caseStudyPaths(env = process.env) {
  const root = env.INTEL_CASE_STUDY_DIR
  return {
    work: root || join(homedir(), 'Documents', 'case-studies'),
    data: root ? join(root, 'data') : join(homedir(), '.local', 'share', 'case-study'),
    state: root ? join(root, 'cache') : join(homedir(), '.cache', 'case-study-limits'),
  }
}
