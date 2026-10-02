import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
export const envPath = join(homedir(), '.config/intel/.env');
export function loadEnvFile(path = envPath) {
  if (!existsSync(path)) return;
  for (const raw of readFileSync(path,'utf8').split(/\r?\n/)) {
    const line=raw.trim();
    if (!line || line.startsWith('#') || !line.includes('=')) continue;
    const idx=line.indexOf('='), key=line.slice(0,idx).trim();
    if (!(key in process.env)) process.env[key]=line.slice(idx+1).trim();
  }
}
loadEnvFile();
export function requireEnv(key) {
  const value=process.env[key];
  if (!value) throw new Error(`${key} is required in ${envPath}`);
  return value;
}
