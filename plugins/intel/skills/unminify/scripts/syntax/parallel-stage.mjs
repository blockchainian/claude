import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { openJournal, atomicWrite } from '../journal.mjs';
import { syntaxFingerprint } from './stage.mjs';
import { rules } from './registry.mjs';
const globals = [...new Set(rules.flatMap(rule => rule.globals ?? []))].sort();

// Keep stage.mjs unchanged: its bytes are part of existing checkpoint fingerprints.
// Workers compute source only; durable intents and atomic writes stay on this thread.
export async function runSyntaxFileParallel(root, file, { fragment = false, fragmentContext } = {}, pool) {
  const path = join(root, file), source = readFileSync(path, 'utf8');
  if (existsSync(join(root, '.unminify', `${file}.jsonl`))) return { status: 'syntax-skipped', file };
  const options = { pipeline: syntaxFingerprint, fragment,
    ...(fragmentContext === undefined ? {} : { shadowedGlobals: globals.filter(name => fragmentContext.topLevelBindings.has(name)) }) };
  const journal = openJournal(root, join('syntax', `${file}.syntax`), source, options);
  const header = JSON.parse(readFileSync(journal.path, 'utf8').split('\n')[0]);
  if (header.options !== JSON.stringify(options)) throw new Error(`syntax options changed: ${file}`);
  if (journal.done) return { status: 'syntax-skipped', file };
  const result = await pool.run({ kind: 'transform', source, options: { fragment, fragmentContext } });
  journal.prepare(source, result.source, 'syntax');
  if (source !== result.source) atomicWrite(path, result.source);
  journal.complete(result.source, 'syntax');
  return { status: 'syntax', file, changed: source !== result.source, steps: result.steps };
}
