import { knownGlobals } from './chunks.mjs';
import { toIdentifier, orderImportRestores, importCollisionName, sameImportedBinding } from './rename.mjs';
import { analyzeImportChunkFile, applyImportChunkFile, checkImportChunkFile } from './import-chunk-file.mjs';
async function mapFiles(entries, pool, run) {
  if (pool?.map) return pool.map(entries, run);
  // No pool is used by small API callers; keep parsing sequential there.
  const results = [];
  for (const entry of entries) results.push(await run(entry));
  return results;
}
function importedBinding(binding) {
  return { identifier: { name: binding.name }, path: {
    isImportSpecifier: () => binding.namedImport,
    parent: { source: { value: binding.source } },
    node: { imported: { name: binding.imported } },
  } };
}
function assemble(files, sources, options) {
  const declarations = new Map(), usages = new Map(), free = new Set();
  for (const [file, record] of files) {
    for (const binding of record.bindings) {
      if (!declarations.has(binding.name)) declarations.set(binding.name, []);
      declarations.get(binding.name).push({ file, binding });
    }
    for (const token of record.lexical) {
      if (token.top && token.reference) {
        if (!usages.has(token.name)) usages.set(token.name, []);
        usages.get(token.name).push({ file, ...token });
      }
    }
  }
  for (const record of files.values()) for (const token of record.lexical) if (token.free && !declarations.has(token.name)) free.add(token.name);
  const dynamicScope = [...files.values()].some(record => record.hasWith || record.directEval && !declarations.has('eval'));
  const selected = options.selected ?? new Set(sources.keys()), ignored = options.ignored ?? new Set();
  const bindings = new Map(), frozen = new Map(), candidates = [];
  const statistics = { duplicate: 0, ignored: 0, knownGlobal: 0, outsideSelection: 0, imports: 0, dynamicScope: 0 };
  for (const [name, owners] of declarations) {
    const owner = owners[0], references = usages.get(name) ?? [], touched = new Set([...owners.map(entry => entry.file), ...references.map(entry => entry.file)]);
    const record = { name, file: owner.file, key: owner.binding.key, kind: owner.binding.kind, files: touched, references, declarations: owners };
    bindings.set(name, record);
    let reason;
    if (dynamicScope) reason = 'dynamicScope';
    else if (owners.length > 1) reason = 'duplicate';
    else if ([...touched].some(file => ignored.has(file))) reason = 'ignored';
    else if (knownGlobals.has(name)) reason = 'knownGlobal';
    else if (!options.chunkSiblings && [...touched].some(file => !selected.has(file))) reason = 'outsideSelection';
    else if (!selected.has(owner.file) && !(options.allowImports && options.chunkSiblings && record.kind === 'module'
      && ([...touched].some(file => selected.has(file)) || owner.binding.namedImport || /^\.{1,2}\//.test(owner.binding.source)))) reason = 'outsideSelection';
    if (reason) { frozen.set(name, reason); statistics[reason]++; }
    else { if (record.kind === 'module') statistics.imports++; if (owner.binding.model && name.length < (options.minSymbolLength ?? 4)) candidates.push(record); }
  }
  return { files, bindings, declarations, free, frozen, candidates, statistics, dynamicScope, sources };
}

export async function analyzeImportChunk(sources, options = {}, pool) {
  const entries = await mapFiles([...sources], pool, async ([file, source]) => [file,
    pool ? await pool.run({ kind: 'import-chunk-analyze', source }) : analyzeImportChunkFile(source)]);
  return assemble(new Map(entries), sources, options);
}
export function resolveImportChunkNames(analysis, proposals, options = {}) {
  const occupied = new Map(analysis.bindings), result = new Map();
  const ordered = options.restoreExternalImports ? orderImportRestores(proposals,
    new Map([...analysis.bindings].filter(([name]) => !analysis.frozen.has(name)).map(([name, record]) => [name, { binding: importedBinding(record.declarations[0].binding), references: record.references.length }]))) : proposals;
  for (const [name, raw] of ordered) {
    const binding = analysis.bindings.get(name);
    if (!binding) throw new Error(`Unknown chunk binding: ${name}`);
    if (analysis.frozen.has(name) || !binding.declarations[0].binding.model && !options.allowImports) continue;
    let stem = toIdentifier(raw);
    if (binding.references.some(ref => ref.jsx) && /^[a-z]/.test(stem)) stem = stem[0].toUpperCase() + stem.slice(1);
    if (stem === name) continue;
    const collides = (candidate, differentImport = false) => candidate !== name && occupied.has(candidate)
      && (!differentImport || occupied.get(candidate).declarations.some(other => !sameImportedBinding(importedBinding(binding.declarations[0].binding), importedBinding(other.binding))))
      || analysis.free.has(candidate) || binding.references.some(ref => ref.nested.includes(candidate));
    if (options.restoreExternalImports && collides(stem, true)) stem = importCollisionName(importedBinding(binding.declarations[0].binding), stem);
    let candidate = stem, suffix = 2;
    if (collides(candidate)) options.onCollision?.({ key: name, proposed: stem, occupied: [...new Set([...occupied.keys(), ...analysis.free, ...binding.references.flatMap(ref => ref.nested)])].filter(candidate => collides(candidate)), context: binding.declarations[0].binding.context });
    while (collides(candidate)) candidate = `${stem}${suffix++}`;
    occupied.delete(name); occupied.set(candidate, binding);
    if (candidate !== name) result.set(name, candidate);
  }
  return result;
}
export async function applyImportChunkRenames(sources, names, analysis, pool) {
  for (const name of names.keys()) if (!analysis.bindings.has(name)) throw new Error(`Unknown chunk binding: ${name}`);
  if (!names.size) return new Map(sources);
  return new Map(await mapFiles([...sources], pool, async ([file, source]) => [file,
    pool ? await pool.run({ kind: 'import-chunk-apply', source, names: [...names], record: analysis.files.get(file) }) : applyImportChunkFile(source, names, analysis.files.get(file))]));
}
function identities(analysis) {
  return JSON.stringify([...analysis.files].map(([file, record]) => [file, record.lexical.filter(token => !token.exported).map(token => {
    if (token.identityLocal !== null) return `local:${token.identityLocal}`;
    const owners = analysis.declarations.get(token.name);
    if (owners?.length === 1) { const owner = owners[0]; return `chunk:${owner.file}:${owner.binding.index}`; }
    return owners?.length > 1 ? `ambiguous:${token.name}` : `global:${token.name}`;
  })]));
}
export async function checkImportChunkRewrite(before, after, analysis, pool) {
  let next;
  try {
    if (JSON.stringify([...before.keys()]) !== JSON.stringify([...after.keys()])) return 'CHUNK_FILES_CHANGED';
    if ([...before].every(([file, source]) => after.get(file) === source)) return null;
    const entries = await mapFiles([...after], pool, async ([file, source]) => [file, source === before.get(file) ? analysis.files.get(file)
      : pool ? await pool.run({ kind: 'import-chunk-analyze', source }) : analyzeImportChunkFile(source)]);
    next = assemble(new Map(entries), after, {});
    if (analysis.dynamicScope && JSON.stringify([...analysis.files].map(([file, record]) => [file, record.allBindings.map(binding => binding.name)])) !== JSON.stringify([...next.files].map(([file, record]) => [file, record.allBindings.map(binding => binding.name)]))) return 'CHUNK_DYNAMIC_SCOPE';
    if (identities(analysis) !== identities(next)) return 'CHUNK_BINDING_CHANGED';
    const inverse = new Map();
    for (const [file, record] of analysis.files) {
      const current = next.files.get(file);
      if (record.bindings.length !== current.bindings.length) return 'CHUNK_BINDINGS_CHANGED';
      for (let i = 0; i < record.bindings.length; i++) {
        const original = record.bindings[i].name, renamed = current.bindings[i].name;
        if (original === renamed) continue;
        if (analysis.declarations.get(original).length > 1) return 'CHUNK_DUPLICATE_CHANGED';
        if (inverse.has(renamed) && inverse.get(renamed) !== original) return 'CHUNK_DUPLICATE_ADDED';
        inverse.set(renamed, original);
      }
    }
    const reasons = await mapFiles([...before].filter(([file, source]) => after.get(file) !== source), pool, async ([file, source]) => {
      const task = { kind: 'import-chunk-check', before: source, after: after.get(file), original: analysis.files.get(file), current: next.files.get(file), inverse: [...inverse] };
      const reason = pool ? await pool.run(task) : checkImportChunkFile(task);
      return reason ? `${file}: ${reason}` : null;
    });
    return reasons.find(Boolean) ?? null;
  } catch (error) { return `CHUNK_CHECK_FAILED ${String(error.message).split(/[\r\n]/)[0]}`; }
  finally { if (next) releaseImportChunk(next); }
}
export function releaseImportChunk(analysis) {
  analysis.files.clear(); analysis.bindings.clear(); analysis.declarations.clear(); analysis.free.clear(); analysis.frozen.clear(); analysis.candidates.length = 0; analysis.sources = null;
}
