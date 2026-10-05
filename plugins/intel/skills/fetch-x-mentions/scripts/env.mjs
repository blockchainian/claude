import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export let envPath = fileURLToPath(new URL('.env', import.meta.url));
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
