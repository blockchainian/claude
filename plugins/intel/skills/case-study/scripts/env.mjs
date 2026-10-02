// ABOUTME: Loads the skill's settings from a .env file into process.env, for the scripts that call outside services.
// ABOUTME: The file is scripts/.env next to the scripts, else ~/.cache/secrets-manager/profiles/case-study/.env.
import { existsSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

export const ENV_FILES = [join(dirname(fileURLToPath(import.meta.url)), '.env'), join(homedir(), '.cache', 'secrets-manager', 'profiles', 'case-study', '.env')]

// KEY=value lines; quotes around the value are dropped; a variable already in the environment wins.
export function parseEnv(text) {
  const values = {}
  for (const line of text.split('\n')) {
    const m = line.match(/^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/)
    if (!m || line.trim().startsWith('#')) continue
    values[m[1]] = m[2].replace(/^(["'])(.*)\1$/, '$2')
  }
  return values
}

export function loadEnv(files = ENV_FILES, env = process.env) {
  const file = files.find(f => existsSync(f))
  if (!file) return null
  for (const [key, value] of Object.entries(parseEnv(readFileSync(file, 'utf8')))) if (!(key in env)) env[key] = value
  return file
}
