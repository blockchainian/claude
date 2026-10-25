import { readFileSync, readdirSync, openSync, closeSync, ftruncateSync, fsyncSync, writeFileSync } from 'node:fs';
import { join, isAbsolute, normalize, resolve, relative } from 'node:path';
import { hash, appendRecord, atomicWrite, validateResponse, responseValues } from './journal.mjs';

const stable = value => JSON.stringify(sort(value));
function sort(value) {
  if (Array.isArray(value)) return value.map(sort);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, sort(value[key])]));
  return value;
}
function validateFile(root, file) {
  if (typeof file !== 'string' || !file || isAbsolute(file) || normalize(file).split(/[\\/]/).includes('..') || relative(resolve(root), resolve(root, file)).split(/[\\/]/).includes('..')) {
    throw new Error('chunk journal requires a relative repository path');
  }
}
function sourceEntries(root, sources) {
  if (!(sources instanceof Map) || !sources.size) throw new Error('chunk journal requires source Map');
  return [...sources].map(([file, source]) => {
    validateFile(root, file);
    if (typeof source !== 'string') throw new Error(`invalid source: ${file}`);
    return [file, source];
  }).sort(([a], [b]) => a.localeCompare(b));
}
const fingerprints = entries => Object.fromEntries([...entries].map(([file, source]) => [file, hash(source)]));
function recordsAt(path) {
  let data;
  try { data = readFileSync(path); } catch (error) { if (error.code === 'ENOENT') return []; throw error; }
  const end = data.lastIndexOf(10) + 1;
  if (end !== data.length) {
    const fd = openSync(path, 'r+');
    try { ftruncateSync(fd, end); fsyncSync(fd); } finally { closeSync(fd); }
  }
  return data.subarray(0, end).toString('utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
}
function appendBatches(path, batches) {
  const text = [...batches].map(([id, names]) => `${JSON.stringify({ type: 'chunk-batch', id, names })}\n`).join('');
  // The header already made this directory entry durable; appends only change file data.
  const fd = openSync(path, 'a');
  try {
    writeFileSync(fd, text);
    fsyncSync(fd);
  } finally { closeSync(fd); }
}
function stateAt(path) {
  const records = recordsAt(path);
  if (!records.length) return null;
  const header = records[0];
  if (header.type !== 'chunk-source' || header.version !== 1 || typeof header.id !== 'string' || !header.hashes || typeof header.options !== 'string') throw new Error(`invalid chunk journal header: ${path}`);
  const batches = new Map(), replies = new Map();
  let done = null, intent = null, selection = null;
  for (const record of records.slice(1)) {
    if (record.type === 'chunk-selection') {
      sameMembership(header.hashes, record.hashes, header.id);
      header.hashes = record.hashes;
      selection = record.files;
      batches.clear(); replies.clear(); done = null; intent = null;
    }
    else if (record.type === 'response') { validateResponse(replies, record.request, record.names); replies.set(record.request, record.names); }
    else if (record.type === 'chunk-prepare') { intent = record; done = null; }
    else if (record.type === 'chunk-done') { done = record; intent = null; }
    else if (record.type === 'chunk-batch' && typeof record.id === 'string' && record.names && typeof record.names === 'object') {
      if (batches.has(record.id) && stable(batches.get(record.id)) !== stable(record.names)) throw new Error(`conflicting batch: ${record.id}`);
      batches.set(record.id, record.names);
    } else throw new Error(`invalid chunk journal record: ${path}`);
  }
  return { header, batches, replies, done, intent, selection };
}
function sameMembership(left, right, id) {
  if (stable(Object.keys(left).sort()) !== stable(Object.keys(right).sort())) throw new Error(`membership changed: ${id}`);
}
function diskHashes(root, expected, id) {
  const result = {};
  for (const file of Object.keys(expected)) {
    validateFile(root, file);
    try { result[file] = hash(readFileSync(join(root, file), 'utf8')); }
    catch (error) { if (error.code === 'ENOENT') throw new Error(`source changed: ${id}: ${file}`); throw error; }
  }
  return result;
}
function verifyHashes(actual, expected, id) {
  for (const file of Object.keys(expected)) if (actual[file] !== expected[file]) throw new Error(`source changed: ${id}: ${file}`);
}
function verifyIntent(root, state) {
  const { header, intent } = state;
  if (!intent || !intent.before || !intent.after || !intent.sources) throw new Error(`invalid chunk intent: ${header.id}`);
  sameMembership(header.hashes, intent.before, header.id);
  sameMembership(intent.before, intent.after, header.id);
  sameMembership(intent.before, intent.sources, header.id);
  const actual = diskHashes(root, intent.before, header.id);
  for (const file of Object.keys(intent.before)) {
    if (typeof intent.sources[file] !== 'string' || hash(intent.sources[file]) !== intent.after[file]) throw new Error(`invalid chunk intent source: ${header.id}: ${file}`);
    if (actual[file] !== intent.before[file] && actual[file] !== intent.after[file]) throw new Error(`source changed: ${header.id}: ${file}`);
  }
  return actual;
}
function recover(root, path, state, actual, write = atomicWrite) {
  for (const [file, source] of Object.entries(state.intent.sources)) {
    if (actual[file] !== state.intent.after[file]) write(join(root, file), source);
  }
  const done = { type: 'chunk-done', hashes: state.intent.after, status: state.intent.status };
  appendRecord(path, done);
  return done;
}

// Validate every pending transaction before completing any interrupted group write.
export function recoverChunkJournals(root, settings, journalDirectory = 'chunks') {
  const directory = join(root, '.unminify', journalDirectory);
  let files;
  try { files = readdirSync(directory).filter(file => file.endsWith('.jsonl')).sort(); }
  catch (error) { if (error.code === 'ENOENT') return 0; throw error; }
  const pending = [];
  for (const file of files) {
    const path = join(directory, file), state = stateAt(path);
    if (!state?.intent) continue;
    const options = typeof settings === 'function' ? settings(state.header.id) : settings;
    if (options !== undefined && state.header.options !== stable(options)) throw new Error(`options changed: ${state.header.id}`);
    pending.push({ path, state, actual: verifyIntent(root, state) });
  }
  for (const { path, state, actual } of pending) recover(root, path, state, actual);
  return pending.length;
}

export function openChunkJournal(root, id, sources, options = {}, journalDirectory = 'chunks') {
  if (typeof id !== 'string' || !id) throw new Error('chunk journal requires an id');
  let current = new Map(sourceEntries(root, sources));
  const hashes = fingerprints(current), config = stable(options);
  const path = join(root, '.unminify', journalDirectory, `${hash(id)}.jsonl`);
  let state = stateAt(path);
  if (!state) {
    const header = { type: 'chunk-source', version: 1, id, hashes, options: config };
    appendRecord(path, header);
    state = { header, batches: new Map(), replies: new Map(), done: null, intent: null };
  }
  if (state.header.id !== id) throw new Error(`chunk id changed: ${id}`);
  sameMembership(state.header.hashes, hashes, id);
  if (state.header.options !== config) throw new Error(`options changed: ${id}`);
  if (state.intent) {
    const actual = verifyIntent(root, state);
    state.done = recover(root, path, state, actual);
    current = new Map(Object.entries(state.intent.sources));
    state.intent = null;
  }
  const expected = state.done?.hashes ?? state.header.hashes;
  verifyHashes(fingerprints(current), expected, id);
  verifyHashes(diskHashes(root, expected, id), expected, id);
  const journal = {
    path, batches: state.batches, replies: state.replies,
    get selection() { return state.selection; },
    begin(files) {
      const selection = [...new Set([...(state.selection ?? []), ...files])].sort();
      if (state.selection && (!state.done || selection.length === state.selection.length)) return;
      const record = { type: 'chunk-selection', files: selection, hashes: fingerprints(current) };
      appendRecord(path, record);
      state.selection = selection;
      state.batches.clear(); state.replies.clear(); state.done = null;
    },
    response(request, names) {
      const values = responseValues(names);
      if (!validateResponse(state.replies, request, values)) return;
      appendRecord(path, { type: 'response', request, names: values });
      state.replies.set(request, values);
    },
    get done() { return state.done; },
    batch(batchId, names) {
      journal.batchMany([[batchId, names]]);
    },
    batchMany(entries) {
      const pending = new Map();
      for (const [batchId, names] of entries) {
        if (typeof batchId !== 'string') throw new Error('chunk batch requires a string id');
        const values = Object.fromEntries(names);
        const existing = pending.has(batchId) ? pending.get(batchId) : state.batches.get(batchId);
        if (pending.has(batchId) || state.batches.has(batchId)) {
          if (stable(existing) !== stable(values)) throw new Error(`conflicting batch: ${batchId}`);
          continue;
        }
        pending.set(batchId, values);
      }
      if (!pending.size) return;
      appendBatches(path, pending);
      for (const [batchId, values] of pending) state.batches.set(batchId, values);
    },
    prepare(before, after, status = 'changed') {
      if (state.intent) throw new Error(`chunk write already pending: ${id}`);
      const beforeHashes = fingerprints(sourceEntries(root, before));
      const afterEntries = sourceEntries(root, after), afterHashes = fingerprints(afterEntries);
      sameMembership(expected, beforeHashes, id);
      sameMembership(beforeHashes, afterHashes, id);
      verifyHashes(beforeHashes, fingerprints(current), id);
      verifyHashes(diskHashes(root, beforeHashes, id), beforeHashes, id);
      const intent = { type: 'chunk-prepare', before: beforeHashes, after: afterHashes, sources: Object.fromEntries(afterEntries), status };
      appendRecord(path, intent);
      state.intent = intent;
    },
    complete(completed, status, reason, details = {}) {
      const entries = sourceEntries(root, completed), completedHashes = fingerprints(entries);
      const target = state.intent?.after ?? fingerprints(current);
      sameMembership(target, completedHashes, id);
      verifyHashes(completedHashes, target, id);
      verifyHashes(diskHashes(root, target, id), target, id);
      const done = { type: 'chunk-done', hashes: completedHashes, status, ...(reason ? { reason } : {}), ...details };
      appendRecord(path, done);
      state.done = done;
      state.intent = null;
      current = new Map(entries);
    },
    commit(after, { write = atomicWrite, status = 'changed' } = {}) {
      journal.prepare(current, after, status);
      for (const [file, source] of sourceEntries(root, after)) if (source !== current.get(file)) write(join(root, file), source);
      journal.complete(after, status);
    },
  };
  return journal;
}
