import { realpathSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { parseArgs, run } from '../../scripts/cli.mjs';

export function parseRunArgs(args) {
  const { command, ...options } = parseArgs(['rename-bindings', ...args]);
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  try {
    process.exitCode = (await run(parseRunArgs(process.argv.slice(2)))).exitCode;
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
