import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';
import { join } from 'node:path';

export let envPath = join(homedir(), '.config', 'intel', '.env');
export function loadEnvFile(path = envPath) {
  envPath = path instanceof URL ? fileURLToPath(path) : path;
  if (!existsSync(envPath)) return;
  process.loadEnvFile(envPath);
}
export function requireEnv(key) {
  const value = process.env[key];
  if (!value) throw new Error(`${key} is required in ${envPath}`);
  return value;
}

// Data and final outputs use separate plugin roots; each skill adds its own subdirectory.
export function dataDir(env = process.env) {
  return expandUser(env.INTEL_DATA_DIR || join(homedir(), '.local', 'share', 'intel'));
}
export function outputDir(env = process.env) {
  return expandUser(env.INTEL_OUTPUT_DIR || join(homedir(), 'Documents'));
}
function expandUser(path) {
  return path.startsWith('~/') ? join(homedir(), path.slice(2)) : path;
}
export function caseStudyStateDir(env = process.env) {
  return env.INTEL_DATA_DIR ? join(dataDir(env), 'cache', 'case-study-limits') : join(homedir(), '.cache', 'case-study-limits');
}

// Python skills reuse this loader rather than implementing another dotenv parser.
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  loadEnvFile();
  console.log(JSON.stringify({data: dataDir(), output: outputDir()}));
}
