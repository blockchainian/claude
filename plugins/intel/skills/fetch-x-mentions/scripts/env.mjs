import { existsSync, mkdirSync } from 'node:fs';
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

// Final outputs, state that must survive and freely deletable data use separate plugin roots;
// each skill adds its own subdirectory.
export function dataDir(env = process.env) {
  return expandUser(env.INTEL_DATA_DIR || join(homedir(), '.local', 'share', 'intel'));
}
export function stateDir(env = process.env) {
  return expandUser(env.INTEL_STATE_DIR || join(homedir(), '.local', 'state', 'intel'));
}
export function outputDir(env = process.env) {
  return expandUser(env.INTEL_OUTPUT_DIR || join(homedir(), 'Documents'));
}
// Scratch files of one skill's runs, under the deletable data root.
export function tmpDir(skill, env = process.env) {
  const dir = join(dataDir(env), 'tmp', skill);
  mkdirSync(dir, { recursive: true });
  return dir;
}
// Pace files, slot locks, account cooldowns and the gate log, shared by case-study and fetch-x-posts.
export function limitsDir(env = process.env) {
  return join(stateDir(env), 'limits');
}
function expandUser(path) {
  return path.startsWith('~/') ? join(homedir(), path.slice(2)) : path;
}

// Python skills reuse this loader rather than implementing another dotenv parser.
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  loadEnvFile();
  console.log(JSON.stringify({data: dataDir(), state: stateDir(), output: outputDir()}));
}
