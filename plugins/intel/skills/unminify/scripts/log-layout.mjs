import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const commands = new Set(['restore-syntax', 'rename-bindings', 'sync-imports']);
const legacy = new Set(['syntax', 'merge-imports', 'chunks', 'syntax-preflight', 'imports']);

export function assertLogLayout(root) {
  const directory = join(root, '.unminify');
  if (!existsSync(directory)) return;
  function hasJournal(path) {
    return readdirSync(path, { withFileTypes: true }).some(entry =>
      entry.isDirectory() ? hasJournal(join(path, entry.name)) : entry.name.endsWith('.jsonl'));
  }
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (commands.has(entry.name)) continue;
    if (legacy.has(entry.name) || entry.name.endsWith('.jsonl')
      || entry.isDirectory() && hasJournal(join(directory, entry.name))) {
      throw new Error(`Legacy .unminify layout: ${entry.name}; migrate logs before running unminify.`);
    }
  }
}
