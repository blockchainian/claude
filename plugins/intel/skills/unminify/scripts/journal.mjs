import { createHash, randomUUID } from 'node:crypto';
import { openSync, closeSync, fsyncSync, writeFileSync, readFileSync, mkdirSync, renameSync, unlinkSync, statSync, ftruncateSync } from 'node:fs';
import { dirname, join, isAbsolute, normalize } from 'node:path';

export const hash = source => createHash('sha256').update(source).digest('hex');

function syncDirectory(path) {
  const fd = openSync(path, 'r');
  try { fsyncSync(fd); } finally { closeSync(fd); }
}

export function appendRecord(path, record) {
  mkdirSync(dirname(path), { recursive: true });
  const fd = openSync(path, 'a', 0o600);
  try {
    writeFileSync(fd, `${JSON.stringify(record)}\n`);
    fsyncSync(fd);
  } finally { closeSync(fd); }
  syncDirectory(dirname(path));
}

export function atomicWrite(path, source) {
  const temporary = join(dirname(path), `.${randomUUID()}.unminify`);
  const fd = openSync(temporary, 'wx', statSync(path).mode & 0o777);
  try {
    writeFileSync(fd, source);
    fsyncSync(fd);
  } finally { closeSync(fd); }
  try {
    renameSync(temporary, path);
    syncDirectory(dirname(path));
  } finally {
    try { unlinkSync(temporary); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
}

function readRecords(path) {
  let data;
  try { data = readFileSync(path); } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
  const end = data.lastIndexOf(10) + 1;
  if (end !== data.length) {
    const fd = openSync(path, 'r+');
    try { ftruncateSync(fd, end); fsyncSync(fd); } finally { closeSync(fd); }
  }
  return data.subarray(0, end).toString('utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
}

export function validateResponse(replies, request, names) {
  if (typeof request !== 'string' || !request || !names || typeof names !== 'object' || Array.isArray(names)
    || !Object.keys(names).length || Object.entries(names).some(([key, value]) => !key || typeof value !== 'string')) throw new Error('invalid response record');
  if (!replies.has(request)) return true;
  const previous = replies.get(request);
  if (Object.keys(previous).length !== Object.keys(names).length || Object.entries(previous).some(([key, value]) => !Object.hasOwn(names, key) || names[key] !== value)) throw new Error(`conflicting response: ${request}`);
  return false;
}

export function responseValues(names) {
  if (!(names instanceof Map) || [...names.keys()].some(key => typeof key !== 'string' && !(Number.isSafeInteger(key) && key >= 0))) throw new Error('invalid response names');
  return Object.fromEntries(names);
}

export function openJournal(root, file, source, options) {
  if (isAbsolute(file) || normalize(file).split(/[\\/]/).includes('..')) throw new Error('journal requires a relative repository path');
  const path = join(root, '.unminify', `${file}.jsonl`);
  const records = readRecords(path);
  const fingerprint = hash(source);
  const config = JSON.stringify(options);
  if (!records.length) {
    const header = { type: 'source', hash: fingerprint, options: config };
    appendRecord(path, header);
    records.push(header);
  }
  const header = records[0];
  if (header.type !== 'source') throw new Error(`invalid journal header: ${file}`);
  const batches = new Map(), replies = new Map();
  let done = null, intent = null;
  for (const record of records.slice(1)) {
    if (record.type === 'response') { validateResponse(replies, record.request, record.names); replies.set(record.request, record.names); }
    else if (record.type === 'prepare') intent = record;
    else if (record.type === 'done') { done = record; intent = null; }
    else if (record.type === 'imports-rejected') { /* Retain the accepted source state. */ }
    else if (typeof record.id === 'string' && record.names && typeof record.names === 'object') {
      if (batches.has(record.id) && JSON.stringify(batches.get(record.id)) !== JSON.stringify(record.names)) throw new Error(`conflicting batch: ${record.id}`);
      batches.set(record.id, record.names);
    } else throw new Error(`invalid journal record: ${file}`);
  }
  const append = record => appendRecord(path, record);
  // A durable intent bridges the rename/complete crash window without saving source copies.
  if (intent && fingerprint === intent.after) {
    done = { type: 'done', hash: fingerprint, status: intent.status };
    append(done);
    intent = null;
  }
  if (intent && fingerprint !== intent.before) throw new Error(`source changed: ${file}`);
  const expected = intent ? intent.before : done ? done.hash : header.hash;
  if (fingerprint !== expected) throw new Error(`source changed: ${file}`);
  if (!done && header.options !== config) throw new Error(`options changed: ${file}`);
  return {
    path, batches, replies,
    response(request, names) {
      const values = responseValues(names);
      if (!validateResponse(replies, request, values)) return;
      append({ type: 'response', request, names: values });
      replies.set(request, values);
    },
    get done() { return done; },
    batch(id, names) {
      const values = Object.fromEntries(names);
      if (batches.has(id)) {
        if (JSON.stringify(batches.get(id)) !== JSON.stringify(values)) throw new Error(`conflicting batch: ${id}`);
        return;
      }
      append({ id, names: values });
      batches.set(id, values);
    },
    prepare(before, after, status) { append({ type: 'prepare', before: hash(before), after: hash(after), status }); },
    complete(source, status, reason) {
      done = { type: 'done', hash: hash(source), status, ...(reason ? { reason } : {}) };
      append(done);
    },
    rejectImports(reason) { append({ type: 'imports-rejected', reason }); },
  };
}
