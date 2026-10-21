import { readdirSync, readFileSync, existsSync, statSync, realpathSync } from 'node:fs';
import { join, relative, resolve, dirname } from 'node:path';
import { parse } from '@babel/parser';
import { hash, readRecords, openJournal } from './journal.mjs';
import { openChunkJournal } from './chunk-journal.mjs';

/** Discover rename journals without repairing or resuming unfinished work. */
export function journalHistory(root) {
  const directory = join(root, '.unminify'), result = [];
  function walk(folder) {
    if (!existsSync(folder)) return;
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isDirectory()) {
        if (folder !== directory || !['syntax', 'merge-imports', 'imports'].includes(entry.name)) walk(path);
      }
      else if (entry.isFile() && (/\.m?js\.jsonl$/.test(entry.name)
        || dirname(path) === join(directory, 'chunks') && entry.name.endsWith('.jsonl'))) {
        const records = readRecords(path, false), header = records[0];
        if (header?.type === 'source') {
          result.push({ file: relative(directory, path).slice(0, -6), records, header });
        } else if (header?.type === 'chunk-source') result.push({ records, header });
      }
    }
  }
  walk(directory);
  return result;
}

export function loadHistory(root, { sources, journals, chunks, ignores, emit }) {
  for (const { file, records, header } of journalHistory(root)) {
    const members = file ? [file] : Object.keys(header.hashes);
    if (members.every(member => sources.has(member))) continue;
    const inputs = new Map();
    let missing = false;
    for (const member of members) {
      const absolute = resolve(root, member), rel = relative(root, absolute);
      if (!rel || rel.startsWith('../') || rel === '..') throw new Error(`invalid journal source: ${member}`);
      try {
        const actual = relative(root, realpathSync(absolute));
        if (actual.startsWith('../') || actual === '..') throw new Error(`journal source outside Git root: ${member}`);
        inputs.set(member, readFileSync(absolute, 'utf8'));
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        missing = true;
      }
    }
    const doneType = file ? 'done' : 'chunk-done';
    const lastState = records.findLast(record => [doneType, file ? 'prepare' : 'chunk-prepare', 'chunk-selection'].includes(record.type));
    const done = lastState?.type === doneType ? lastState : null;
    const mismatch = missing || done && [...inputs].some(([member, source]) => hash(source) !== (file ? done.hash : done.hashes[member]));
    if (mismatch) {
      emit({ ...(file ? { file } : { chunk: header.id }), status: 'imports-skipped', reason: 'source changed since journal completion' });
      continue;
    }
    for (const [member, source] of inputs) if (!sources.has(member)) sources.set(member, source);
    if (file) {
      if (done && !ignores(file)) journals.set(file, openJournal(root, file, inputs.get(file), JSON.parse(header.options)));
    } else {
      const options = JSON.parse(header.options);
      chunks.addHistory({ id: header.id, files: members }, inputs,
        done ? openChunkJournal(root, header.id, inputs, options) : null, ignores, options);
    }
  }
}

/** Internal modules outside the naming selection supply exports without gaining write access. */
export function loadImportContext(root, sources) {
  for (const [file, source] of sources) {
    const ast = parse(source, { sourceType: 'unambiguous', allowUndeclaredExports: true });
    for (const node of ast.program.body) {
      const specifier = node.source?.value;
      if (typeof specifier !== 'string' || !/^\.{1,2}\//.test(specifier)) continue;
      const base = resolve(root, dirname(file), specifier);
      for (const path of [base, `${base}.js`, `${base}.mjs`, join(base, 'index.js'), join(base, 'index.mjs')]) {
        if (!existsSync(path) || !statSync(path).isFile()) continue;
        const member = relative(root, realpathSync(path));
        if (member.startsWith('../') || member === '..' || !/\.m?js$/.test(member)) break;
        if (!sources.has(member)) sources.set(member, readFileSync(path, 'utf8'));
        break;
      }
    }
  }
}
