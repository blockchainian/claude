import { createHash, randomUUID } from 'node:crypto';
import { readFileSync, mkdirSync, writeFileSync, renameSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';

const hash = value => createHash('sha256').update(value).digest('hex');
const sorted = values => [...values].sort();

function fingerprint(root, options) {
  const { files, fragmentContext, syntaxFingerprint, importMergeVersion } = options;
  if (!Array.isArray(files) || !(fragmentContext?.fragments instanceof Set)
    || !(fragmentContext?.topLevelBindings instanceof Set) || typeof syntaxFingerprint !== 'string'
    || !Number.isSafeInteger(importMergeVersion)) throw new Error('invalid syntax check inputs');
  return hash(JSON.stringify({
    version: 1, scope: options.scope ?? 'all',
    sources: sorted(new Set(files)).map(file => [file, hash(readFileSync(join(root, file)))]),
    // Member order can decide which duplicate import declaration is retained.
    chunks: (options.chunks ?? []).map(chunk => ({ id: chunk.id, files: chunk.files })),
    ignored: sorted(options.ignored ?? []),
    fragments: sorted(new Set(files)).filter(file => fragmentContext.fragments.has(file)),
    // FlipComparisons reads these intrinsics; RemoveVoid and UnInfinity introduce them.
    shadowedGlobals: ['Infinity', 'NaN', 'undefined'].filter(name => fragmentContext.topLevelBindings.has(name)),
    syntax: syntaxFingerprint, mergeImports: importMergeVersion,
  }));
}

/** A successful check authorizes only its exact source bytes and syntax context. */
export async function runCheckSyntax(root, options, check) {
  const key = fingerprint(root, options);
  const directory = join(root, '.unminify', 'rename-bindings', 'check-syntax');
  const path = join(directory, `${key}.json`);
  let record;
  try { record = JSON.parse(readFileSync(path, 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; }
  if (record?.version === 1 && record.fingerprint === key && record.status === 'passed') {
    return { cached: true, pending: [] };
  }
  const pending = await check();
  if (!Array.isArray(pending) || pending.some(file => typeof file !== 'string')) throw new Error('invalid syntax check result');
  // Never bless newly written text using the verdict on the text read earlier.
  if (!pending.length && fingerprint(root, options) === key) {
    mkdirSync(directory, { recursive: true });
    const temporary = join(directory, `.${randomUUID()}.tmp`);
    try {
      writeFileSync(temporary, `${JSON.stringify({ version: 1, fingerprint: key, status: 'passed' })}\n`, { flag: 'wx', mode: 0o600 });
      renameSync(temporary, path);
    } finally {
      try { unlinkSync(temporary); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    }
  }
  return { cached: false, pending };
}
