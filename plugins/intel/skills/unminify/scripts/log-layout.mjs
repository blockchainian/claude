import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const commands = new Set(['rename-files', 'restore-syntax', 'rename-bindings', 'sync-imports']);
const legacy = new Set(['syntax', 'merge-imports', 'chunks', 'syntax-preflight', 'imports']);

export function assertLogLayout(root, command) {
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
  if (command === 'rename-files') {
    if (['restore-syntax', 'rename-bindings', 'sync-imports'].some(name => existsSync(join(directory, name))))
      throw new Error('Run rename-files before restore-syntax, rename-bindings and sync-imports; later command logs already exist.');
  } else if (existsSync(join(directory, 'rename-files'))) {
    const marker = join(directory, 'rename-files', 'complete.json');
    if (!existsSync(marker) || JSON.parse(readFileSync(marker, 'utf8')).complete !== true)
      throw new Error('rename-files is incomplete; resume rename-files before running later commands.');
  }
}
