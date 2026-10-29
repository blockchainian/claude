import { readFile, mkdir, open, rename, unlink, lstat, realpath } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';

const hash = source => createHash('sha256').update(source).digest('hex');
const relative = (root, file) => path.relative(root, file).split(path.sep).join('/');
function resolveFile(root, file) {
  if (typeof file !== 'string' || !file || file.includes('\\') || path.isAbsolute(file) || /^[A-Za-z]:/.test(file)) throw new Error(`Expected relative repository path: ${file}`);
  const absolute = path.resolve(root, file), rel = relative(root, absolute);
  if (!rel || rel === '..' || rel.startsWith('../')) throw new Error(`Expected relative repository path: ${file}`);
  return absolute;
}
async function contents(file) {
  try {
    const info = await lstat(file);
    if (!info.isFile()) throw new Error(`Expected regular file: ${file}`);
    return await readFile(file, 'utf8');
  } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}
async function atomicWrite(file, source) {
  await mkdir(path.dirname(file), { recursive: true });
  const temporary = `${file}.unminify-${process.pid}.tmp`;
  const handle = await open(temporary, 'w');
  try { await handle.writeFile(source); await handle.sync(); } finally { await handle.close(); }
  await rename(temporary, file);
}
function rewrite(move, targets) {
  let output = move.source, previous = output.length;
  for (const item of [...(move.imports || [])].sort((a, b) => b.start - a.start)) {
    if (!Number.isInteger(item.start) || !Number.isInteger(item.end) || item.start < 0 || item.end > previous || item.start >= item.end) throw new Error(`Invalid import offsets: ${move.from}`);
    previous = item.start;
    if (!item.target) continue;
    const target = targets.get(item.target) || item.target;
    let specifier = path.posix.relative(path.posix.dirname(move.to), target);
    if (!specifier.startsWith('./') && !specifier.startsWith('../')) specifier = `./${specifier}`;
    const quote = move.source[item.start];
    if (quote !== '"' && quote !== "'") throw new Error(`Invalid import literal: ${move.from}`);
    const literal = quote === '"' ? JSON.stringify(specifier) : `'${specifier.replaceAll('\\', '\\\\').replaceAll("'", "\\'")}'`;
    output = output.slice(0, item.start) + literal + output.slice(item.end);
  }
  return output;
}
async function prepare(root, moves, chunks) {
  if (!Array.isArray(moves) || !moves.length) throw new Error('A file move plan is required');
  const targets = new Map(), destinations = new Set();
  for (const move of moves) {
    resolveFile(root, move.from); resolveFile(root, move.to);
    if (typeof move.source !== 'string') throw new Error(`Missing cached source: ${move.from}`);
    if (targets.has(move.from) || destinations.has(move.to)) throw new Error(`Duplicate move path: ${move.from} -> ${move.to}`);
    targets.set(move.from, move.to); destinations.add(move.to);
  }
  const records = [];
  for (const move of moves) {
    if (await contents(resolveFile(root, move.from)) !== move.source) throw new Error(`source changed: ${move.from}`);
    if (!targets.has(move.to) && await contents(resolveFile(root, move.to)) !== null) throw new Error(`Destination already exists: ${move.to}`);
    const output = rewrite(move, targets);
    records.push({ from: move.from, to: move.to, sourceHash: hash(move.source), outputHash: hash(output), output, status: 'pending' });
  }
  let manifest = null;
  if (chunks) {
    const absolute = await realpath(path.resolve(root, chunks));
    const file = relative(root, absolute);
    if (targets.has(file) || destinations.has(file)) throw new Error('Chunk manifest cannot be a moved file');
    const source = await contents(absolute);
    if (source === null) throw new Error(`Missing chunk manifest: ${file}`);
    const data = JSON.parse(source);
    if (!Array.isArray(data.chunks)) throw new Error('Invalid chunk manifest');
    for (const chunk of data.chunks) {
      if (!Array.isArray(chunk.files)) throw new Error('Invalid chunk manifest files');
      chunk.files = chunk.files.map(member => {
        const original = relative(root, path.resolve(path.dirname(absolute), member));
        return targets.has(original) ? relative(path.dirname(absolute), resolveFile(root, targets.get(original))) : member;
      });
    }
    const output = `${JSON.stringify(data, null, 2)}\n`;
    manifest = { file, sourceHash: hash(source), outputHash: hash(output), output, status: 'pending' };
  }
  return { version: 1, phase: 'prepared', moves: records, manifest };
}

/** Commit the cached move plan, or resume its durable write intent without parsing source. */
export async function executeMoves(root, { moves, chunks, journalDir = '.unminify/rename-files', onProgress } = {}) {
  root = await realpath(root);
  const directory = path.isAbsolute(journalDir) ? path.resolve(journalDir) : resolveFile(root, journalDir);
  resolveFile(root, relative(root, directory));
  const journalFile = path.join(directory, 'moves.json');
  let journal;
  const saved = await contents(journalFile);
  if (saved === null) {
    journal = await prepare(root, moves, chunks);
    await atomicWrite(journalFile, `${JSON.stringify(journal)}\n`);
  } else {
    journal = JSON.parse(saved);
    if (journal.version !== 1 || !Array.isArray(journal.moves) || !['prepared', 'clearing', 'writing', 'complete'].includes(journal.phase)) throw new Error('Invalid file move journal');
    if (moves && JSON.stringify(moves.map(({ from, to }) => ({ from, to }))) !== JSON.stringify(journal.moves.map(({ from, to }) => ({ from, to })))) throw new Error('File move plan changed');
    if (chunks && (!journal.manifest || relative(root, await realpath(path.resolve(root, chunks))) !== journal.manifest.file)) throw new Error('Chunk manifest changed');
  }
  const persist = async () => atomicWrite(journalFile, `${JSON.stringify(journal)}\n`);
  const progress = async event => { if (onProgress) await onProgress(event); };
  // Validate the entire transaction before resuming any destructive operation.
  for (const move of journal.moves) {
    resolveFile(root, move.from); resolveFile(root, move.to);
    if (hash(move.output) !== move.outputHash) throw new Error(`Corrupt move journal: ${move.to}`);
    if (journal.phase === 'prepared' || journal.phase === 'clearing') {
      const current = await contents(resolveFile(root, move.from));
      if ((current === null && journal.phase === 'prepared') || (current !== null && hash(current) !== move.sourceHash)) throw new Error(`source changed: ${move.from}`);
    } else {
      const current = await contents(resolveFile(root, move.to));
      if ((current === null && (journal.phase === 'complete' || move.status === 'written')) || (current !== null && hash(current) !== move.outputHash)) throw new Error(`source changed: ${move.to}`);
    }
  }
  if (journal.manifest) {
    const current = await contents(path.resolve(root, journal.manifest.file));
    const expected = journal.phase === 'complete' || journal.manifest.status === 'written' ? [journal.manifest.outputHash] : [journal.manifest.sourceHash, journal.manifest.outputHash];
    if (hash(journal.manifest.output) !== journal.manifest.outputHash) throw new Error('Corrupt manifest move journal');
    if (current === null || !expected.includes(hash(current))) throw new Error(`Chunk manifest changed: ${journal.manifest.file}`);
  }
  if (journal.phase === 'prepared' || journal.phase === 'clearing') {
    const sourcePaths = new Set(journal.moves.map(move => move.from));
    for (const move of journal.moves) {
      if (!sourcePaths.has(move.to) && await contents(resolveFile(root, move.to)) !== null) throw new Error(`Destination already exists: ${move.to}`);
    }
    journal.phase = 'clearing'; await persist();
    for (const move of journal.moves) {
      try { await unlink(resolveFile(root, move.from)); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      move.status = 'removed'; await persist();
      await progress({ phase: 'removed', from: move.from, to: move.to });
    }
    journal.phase = 'writing'; await persist();
  }
  if (journal.phase === 'writing') {
    for (const move of journal.moves) {
      const destination = resolveFile(root, move.to);
      if (await contents(destination) === null) await atomicWrite(destination, move.output);
      move.status = 'written'; await persist();
      await progress({ phase: 'written', from: move.from, to: move.to });
    }
    if (journal.manifest) {
      await atomicWrite(path.resolve(root, journal.manifest.file), journal.manifest.output);
      journal.manifest.status = 'written'; await persist();
      await progress({ phase: 'manifest', file: journal.manifest.file });
    }
    journal.phase = 'complete'; await persist();
  }
  return { files: journal.moves.length, moved: journal.moves.filter(move => move.status === 'written').length, complete: journal.phase === 'complete', manifest: journal.manifest?.file || null };
}
