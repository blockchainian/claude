import { readFile, realpath, stat } from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import Ajv2020 from 'ajv/dist/2020.js';
import ignore from 'ignore';

const schema = JSON.parse(await readFile(new URL('./chunks.schema.json', import.meta.url), 'utf8'));
const validate = new Ajv2020({ allErrors: true }).compile(schema);
const relativePath = (root, file) => path.relative(root, file).split(path.sep).join('/');
const outside = relative => relative === '..' || relative.startsWith('../') || path.isAbsolute(relative);

/** Load explicit shared program scopes without inspecting file contents. */
export async function loadChunks(manifestPath, root) {
  root = await realpath(root);
  manifestPath = await realpath(manifestPath);
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  if (!validate(manifest)) {
    throw new Error(`Invalid chunk manifest: ${validate.errors.map(error => `${error.instancePath || '/'} ${error.message}`).join('; ')}`);
  }
  const ids = new Set(), owners = new Map(), chunks = [];
  for (const chunk of manifest.chunks) {
    if (ids.has(chunk.id)) throw new Error(`Duplicate chunk id: ${chunk.id}`);
    ids.add(chunk.id);
  }
  for (const chunk of manifest.chunks) {
    const files = [];
    for (const file of chunk.files) {
      // The manifest uses portable forward-slash relative paths on every host.
      if (file.includes('\\') || path.win32.isAbsolute(file) || /^[A-Za-z]:/.test(file)) {
        throw new Error(`Invalid chunk manifest path: ${file}`);
      }
      const absolute = await realpath(path.resolve(path.dirname(manifestPath), file));
      const relative = relativePath(root, absolute);
      if (outside(relative)) throw new Error(`Chunk file outside Git root: ${file}`);
      if (!(await stat(absolute)).isFile()) throw new Error(`Chunk path must be a regular file: ${file}`);
      if (owners.has(absolute)) {
        throw new Error(`Duplicate chunk file: ${file} belongs to ${owners.get(absolute)} and ${chunk.id}`);
      }
      owners.set(absolute, chunk.id);
      files.push(relative);
    }
    chunks.push({ id: chunk.id, files: files.sort() });
  }
  chunks.sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const fingerprint = createHash('sha256').update(JSON.stringify({ version: manifest.version, chunks })).digest('hex');
  return { chunks, files: new Set(chunks.flatMap(chunk => chunk.files).sort()), fingerprint };
}

/** Match exclusions relative to the ignore file, exactly as file selection does. */
export async function createChunkIgnore(root, ignoreFile) {
  const requestedRoot = path.resolve(root);
  root = await realpath(root);
  const rulesFile = ignoreFile ? await realpath(ignoreFile) : path.join(root, '.ignore');
  let rules = '';
  try { rules = await readFile(rulesFile, 'utf8'); }
  catch (error) { if (error.code !== 'ENOENT' || ignoreFile) throw error; }
  const matcher = ignore().add(rules);
  return file => {
    let absolute = path.isAbsolute(file) ? file : path.resolve(root, file);
    const requestedRelative = path.relative(requestedRoot, absolute);
    if (path.isAbsolute(file) && !outside(requestedRelative)) absolute = path.resolve(root, requestedRelative);
    const relative = relativePath(path.dirname(rulesFile), absolute);
    return relative !== '' && !outside(relative) && matcher.ignores(relative);
  };
}
