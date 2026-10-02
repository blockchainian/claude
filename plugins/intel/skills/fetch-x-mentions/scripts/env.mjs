import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
export const envPath = join(homedir(), '.config/intel/.env');
export function loadEnvFile(path = envPath) {
  if (!existsSync(path)) return;
  process.loadEnvFile(path);
}
loadEnvFile();
export function requireEnv(key) {
  const value=process.env[key];
  if (!value) throw new Error(`${key} is required in ${envPath}`);
  return value;
}
