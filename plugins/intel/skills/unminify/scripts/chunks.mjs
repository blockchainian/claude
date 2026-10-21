import { createRequire } from 'node:module';
import { parse } from '@babel/parser';
import traverseModule from '@babel/traverse';
import { isLexicalIdentifier, isModelBinding, restoredImportEdit } from './identifiers.mjs';
import { toIdentifier, applyRenames, bindingContext } from './rename.mjs';
import { checkRewrite } from './checks.mjs';
import { estimateInputTokens } from './prompt.mjs';

const traverse = traverseModule.default ?? traverseModule;
const require = createRequire(import.meta.url);
const knownGlobals = new Set([
  ...require('@babel/helper-globals/data/builtin-lower.json'),
  ...require('@babel/helper-globals/data/builtin-upper.json'),
  ...require('@babel/helper-globals/data/browser-upper.json'),
  ...('undefined Infinity NaN arguments require module exports __dirname __filename '
    + 'global process Buffer Bun Deno window document navigator self location history '
    + 'screen console performance crypto fetch atob btoa setTimeout clearTimeout '
    + 'setInterval clearInterval setImmediate clearImmediate queueMicrotask '
    + 'requestAnimationFrame cancelAnimationFrame requestIdleCallback cancelIdleCallback '
    + 'addEventListener removeEventListener dispatchEvent postMessage importScripts close '
    + 'alert confirm prompt matchMedia getComputedStyle localStorage sessionStorage '
    + 'indexedDB caches customElements define').split(' '),
]);

const inspections = new WeakMap();
function inspect(sources) {
  const cached = inspections.get(sources);
  if (cached && cached.sources.length === sources.size && cached.sources.every(([file, source]) => sources.get(file) === source)) return cached.analysis;
  const files = new Map(),
    declarations = new Map(),
    usages = new Map();
  const directEvalCalls = [];
  let hasWith = false;
  for (const [file, source] of sources) {
    const ast = parse(source, {
      sourceType: 'unambiguous',
      allowUndeclaredExports: true,
      plugins: ['jsx']
    });
    let program;
    const lexical = [];
    traverse(ast, {
      Program(path) {
        program = path;
      },
      WithStatement() {
        hasWith = true;
      },
      CallExpression(path) {
        if (path.get('callee').isIdentifier({
          name: 'eval'
        }) && !path.scope.getBinding('eval')) directEvalCalls.push(path);
      },
      'Identifier|JSXIdentifier'(path) {
        if (isLexicalIdentifier(path)) lexical.push(path);
      }
    });
    const bindings = Object.values(program.scope.bindings).sort((a, b) => a.identifier.start - b.identifier.start);
    for (const binding of bindings) {
      const name = binding.identifier.name;
      if (!declarations.has(name)) declarations.set(name, []);
      declarations.get(name).push({
        file,
        binding
      });
    }
    for (const path of lexical) {
      const name = path.node.name,
        binding = path.scope.getBinding(name);
      if (!binding || binding.scope === program.scope) {
        if (!usages.has(name)) usages.set(name, []);
        usages.get(name).push({
          file,
          path
        });
      }
    }
    files.set(file, {
      source,
      ast,
      program,
      lexical,
      bindings
    });
  }
  const dynamicScope = hasWith || directEvalCalls.length > 0 && !declarations.has('eval');
  const analysis = {
    files,
    declarations,
    usages,
    dynamicScope
  };
  inspections.set(sources, {
    sources: [...sources],
    analysis
  });
  return analysis;
}

/** A manifest chunk owns only program bindings; nested bindings retain file scope. */
export function analyzeChunk(sources, {
  selected = new Set(sources.keys()),
  ignored = new Set(),
  chunkSiblings = false,
  minSymbolLength = 4,
  allowImports = false
} = {}) {
  const {
      files,
      declarations,
      usages,
      dynamicScope
    } = inspect(sources),
    bindings = new Map(),
    frozen = new Map(),
    candidates = [];
  const statistics = {
    duplicate: 0,
    ignored: 0,
    knownGlobal: 0,
    outsideSelection: 0,
    imports: 0,
    dynamicScope: 0
  };
  for (const [name, declarationsForName] of declarations) {
    const declaration = declarationsForName[0],
      references = [],
      touched = new Set(declarationsForName.map(entry => entry.file));
    for (const reference of usages.get(name) ?? []) {
      references.push(reference);
      touched.add(reference.file);
    }
    const record = {
      name,
      file: declaration.file,
      key: declaration.binding.identifier.start,
      kind: declaration.binding.kind,
      files: touched,
      references,
      declarations: declarationsForName
    };
    bindings.set(name, record);
    let reason;
    if (dynamicScope) reason = 'dynamicScope';else if (declarationsForName.length > 1) reason = 'duplicate';else if ([...touched].some(file => ignored.has(file))) reason = 'ignored';else if (knownGlobals.has(name)) reason = 'knownGlobal';else if (!chunkSiblings && [...touched].some(file => !selected.has(file))) reason = 'outsideSelection';
    // Model candidates start at selected declarations; import following can
    // include outside declarations when an authorized selected sibling uses them.
    else if (!selected.has(declaration.file) && !(allowImports && chunkSiblings && record.kind === 'module' && [...touched].some(file => selected.has(file)))) reason = 'outsideSelection';
    if (reason) {
      frozen.set(name, reason);
      statistics[reason]++;
    } else {
      if (record.kind === 'module') statistics.imports++;
      if (isModelBinding(declaration.binding) && name.length < minSymbolLength) candidates.push(record);
    }
  }
  return {
    files,
    bindings,
    candidates,
    frozen,
    statistics
  };
}
export function resolveChunkNames(sources, proposals, options = {}) {
  const analysis = analyzeChunk(sources, options),
    result = new Map();
  const occupied = new Set(analysis.bindings.keys());
  const free = new Set();
  for (const file of analysis.files.values()) for (const path of file.lexical) {
    if (!path.scope.getBinding(path.node.name) && !analysis.bindings.has(path.node.name)) free.add(path.node.name);
  }
  for (const [name, raw] of proposals) {
    const binding = analysis.bindings.get(name);
    if (!binding) throw new Error(`Unknown chunk binding: ${name}`);
    if (analysis.frozen.has(name) || !isModelBinding(binding.declarations[0].binding) && !options.allowImports) continue;
    let stem = toIdentifier(raw);
    if (binding.references.some(({
      path
    }) => path.isJSXIdentifier()) && /^[a-z]/.test(stem)) stem = stem[0].toUpperCase() + stem.slice(1);
    if (stem === name) continue;
    const collides = candidate => candidate !== name && occupied.has(candidate) || free.has(candidate) || knownGlobals.has(candidate) || binding.references.some(({
      path
    }) => {
      const other = path.scope.getBinding(candidate);
      return other && other !== binding.declarations[0].binding;
    });
    let candidate = stem,
      suffix = 2;
    if (collides(candidate)) options.onCollision?.({ key: name, proposed: stem, occupied: [...new Set([...occupied, ...free, ...(knownGlobals.has(stem) ? [stem] : []), ...binding.references.flatMap(({ path }) => Object.keys(path.scope.getAllBindings()))])].filter(collides), context: bindingContext(sources.get(binding.file), binding.declarations[0].binding) });
    while (collides(candidate)) candidate = `${stem}${suffix++}`;
    occupied.add(candidate);
    if (candidate !== name) result.set(name, candidate);
  }
  return result;
}
function shorthand(path) {
  let value = path;
  if (value.parentPath.isAssignmentPattern() && value.key === 'left') value = value.parentPath;
  return value.parentPath.isObjectProperty() && value.key === 'value' && value.parentPath.node.shorthand;
}

/** Edit lexical tokens only, preserving public properties and module interfaces. */
export function applyChunkRenames(sources, names) {
  const analysis = inspect(sources),
    output = new Map();
  for (const name of names.keys()) if (!analysis.declarations.has(name)) throw new Error(`Unknown chunk binding: ${name}`);
  for (const [file, {
    source,
    ast,
    program,
    lexical,
    bindings
  }] of analysis.files) {
    const edits = new Map();
    const add = (start, end, text) => edits.set(`${start}:${end}`, {
      start,
      end,
      text
    });
    for (const path of lexical) {
      const name = names.get(path.node.name);
      if (!name || name === path.node.name) continue;
      if (toIdentifier(name) !== name) throw new Error(`Invalid identifier: ${name}`);
      const binding = path.scope.getBinding(path.node.name);
      if (binding && binding.scope !== program.scope) continue;
      const restored = restoredImportEdit(path, source, name);
      if (restored) {
        add(restored.start, restored.end, restored.text);
        continue;
      }
      const {
          start,
          end
        } = path.node,
        raw = source.slice(start, end),
        parent = path.parentPath;
      let text = name;
      if (shorthand(path)) text = `${raw}: ${name}`;else if (parent.isImportSpecifier() && parent.node.imported.start === start) text = `${raw} as ${name}`;else if (parent.isExportSpecifier() && parent.node.exported.start === start) text = `${name} as ${raw}`;
      add(start, end, text);
    }
    traverse(ast, {
      ExportNamedDeclaration(path) {
        const declaration = path.node.declaration;
        if (!declaration) return;
        const exported = bindings.filter(binding => binding.identifier.start >= declaration.start && binding.identifier.end <= declaration.end);
        if (!exported.some(binding => names.has(binding.identifier.name))) return;
        add(path.node.start, path.node.start + /^export\s*/.exec(source.slice(path.node.start))[0].length, '');
        add(declaration.end, declaration.end, `\nexport { ${exported.map(binding => {
          const name = binding.identifier.name;
          return names.has(name) ? `${names.get(name)} as ${name}` : name;
        }).join(', ')} };`);
      }
    });
    let cursor = 0,
      text = '';
    for (const edit of [...edits.values()].sort((a, b) => a.start - b.start || a.end - b.end)) {
      if (edit.start < cursor) throw new Error('Overlapping chunk edits');
      text += source.slice(cursor, edit.start) + edit.text;
      cursor = edit.end;
    }
    output.set(file, text + source.slice(cursor));
  }
  return output;
}
function identityFacts(analysis) {
  const result = [];
  for (const [file, record] of analysis.files) {
    const local = [];
    traverse(record.ast, {
      Scope(path) {
        for (const binding of Object.values(path.scope.bindings)) if (!local.includes(binding)) local.push(binding);
      }
    });
    local.sort((a, b) => a.identifier.start - b.identifier.start);
    result.push([file, record.lexical.filter(path => !path.parentPath.isExportSpecifier()).map(path => {
      const name = path.node.name,
        binding = path.scope.getBinding(name),
        owners = analysis.declarations.get(name);
      if (binding && binding.scope !== record.program.scope) return `local:${local.indexOf(binding)}`;
      if (owners?.length === 1) {
        const owner = owners[0],
          index = analysis.files.get(owner.file).bindings.indexOf(owner.binding);
        return `chunk:${owner.file}:${index}`;
      }
      return owners?.length > 1 ? `ambiguous:${name}` : `global:${name}`;
    })]);
  }
  return JSON.stringify(result);
}

/** Invert only proven program renames, then reuse the strict single-file guard. */
export function checkChunkRewrite(before, after) {
  try {
    if (JSON.stringify([...before.keys()]) !== JSON.stringify([...after.keys()])) return 'CHUNK_FILES_CHANGED';
    if ([...before].every(([file, source]) => after.get(file) === source)) return null;
    const a = inspect(before),
      b = inspect(after),
      inverse = new Map();
    if (a.dynamicScope) {
      const bindingNames = analysis => [...analysis.files].map(([file, record]) => {
        const bindings = new Set();
        traverse(record.ast, {
          Scope(path) {
            for (const binding of Object.values(path.scope.bindings)) bindings.add(binding);
          }
        });
        return [file, [...bindings].sort((left, right) => left.identifier.start - right.identifier.start).map(binding => binding.identifier.name)];
      });
      if (JSON.stringify(bindingNames(a)) !== JSON.stringify(bindingNames(b))) return 'CHUNK_DYNAMIC_SCOPE';
    }
    if (identityFacts(a) !== identityFacts(b)) return 'CHUNK_BINDING_CHANGED';
    for (const [file, record] of a.files) {
      const next = b.files.get(file);
      if (record.bindings.length !== next.bindings.length) return 'CHUNK_BINDINGS_CHANGED';
      for (let i = 0; i < record.bindings.length; i++) {
        const original = record.bindings[i].identifier.name,
          renamed = next.bindings[i].identifier.name;
        if (original !== renamed) {
          if ((a.declarations.get(original)?.length ?? 0) > 1) return 'CHUNK_DUPLICATE_CHANGED';
          if (inverse.has(renamed) && inverse.get(renamed) !== original) return 'CHUNK_DUPLICATE_ADDED';
          inverse.set(renamed, original);
        }
      }
    }
    // Restore nested names first so inversion cannot capture a legal local
    // rename that happens to reuse a program binding's original spelling.
    const localsRestored = new Map(after);
    for (const [file, original] of a.files) {
      const current = b.files.get(file),
        bindingsOf = record => {
          const entries = new Set();
          traverse(record.ast, {
            Scope(path) {
              for (const binding of Object.values(path.scope.bindings)) if (binding.scope !== record.program.scope) entries.add(binding);
            }
          });
          return [...entries].sort((x, y) => x.identifier.start - y.identifier.start);
        };
      const oldBindings = bindingsOf(original),
        newBindings = bindingsOf(current),
        names = new Map();
      if (oldBindings.length !== newBindings.length) return 'CHUNK_BINDINGS_CHANGED';
      for (let i = 0; i < oldBindings.length; i++) if (oldBindings[i].identifier.name !== newBindings[i].identifier.name) names.set(newBindings[i].identifier.start, oldBindings[i].identifier.name);
      if (names.size) localsRestored.set(file, applyRenames(after.get(file), names));
    }
    const restored = applyChunkRenames(localsRestored, inverse);
    for (const [file, source] of before) {
      const reason = checkRewrite(source, restored.get(file), {
        fragment: false, sharedProgram: true
      });
      if (reason) return `${file}: ${reason}`;
    }
    return null;
  } catch (error) {
    return `CHUNK_CHECK_FAILED ${String(error.message).split(/[\r\n]/)[0]}`;
  }
}
export function planChunk(sources, options = {}) {
  const budget = options.maxInputTokens ?? 100000;
  const analysis = analyzeChunk(sources, options);
  return analysis.candidates.map(({
    name
  }) => ({
    id: `chunk:${budget}:${name}`,
    keys: [name]
  }));
}
export function renderChunkBatch(sources, batch, renames = new Map(), {
  placeholderOffset = 0,
  placeholderCount = batch.keys.length
} = {}) {
  const budget = Number(batch.id.split(':')[1]);
  if (!Number.isSafeInteger(budget) || budget < 1) throw new Error('Invalid chunk batch budget');
  const placeholders = {},
    targets = new Map(renames);
  batch.keys.forEach((key, index) => {
    const placeholder = `v${placeholderOffset + index + 1}`;
    placeholders[placeholder] = key;
    targets.set(key, placeholder);
  });
  const analysis = analyzeChunk(sources, {
    minSymbolLength: Number.MAX_SAFE_INTEGER
  });
  // Context-only collisions must not confuse the model's unique placeholders.
  let contextIndex = 0;
  for (const name of analysis.bindings.keys()) if (!targets.has(name) && /^v[1-9]\d*$/.test(name) && Number(name.slice(1)) <= placeholderCount) targets.set(name, `_context${++contextIndex}`);
  const snippets = [];
  for (const key of batch.keys) {
    const record = analysis.bindings.get(key);
    if (!record) throw new Error(`Unknown chunk binding: ${key}`);
    for (const file of record.files) {
      const centers = record.references.filter(ref => ref.file === file).map(ref => ref.path.node.start);
      if (file === record.file) centers.unshift(record.key);
      snippets.push({
        file,
        centers: [...new Set(centers)].slice(0, 3)
      });
    }
  }
  function excerpt(file, center, radius) {
    const record = analysis.files.get(file),
      start = Math.max(0, center - radius),
      end = Math.min(record.source.length, center + radius);
    let cursor = start,
      text = '';
    for (const path of record.lexical) {
      const node = path.node;
      if (node.start < cursor || node.end > end) continue;
      const binding = path.scope.getBinding(node.name);
      const top = !binding || binding.scope === record.program.scope;
      let replacement = top ? targets.get(node.name) : null;
      if (!replacement && /^v[1-9]\d*$/.test(node.name) && Number(node.name.slice(1)) <= placeholderCount) replacement = `_context_${node.name}`;
      if (!replacement) continue;
      text += record.source.slice(cursor, node.start) + replacement;
      cursor = node.end;
    }
    return text + record.source.slice(cursor, end);
  }
  let radius = 500;
  function payload() {
    return {
      context: snippets.map(({
        file,
        centers
      }) => `// ${file}\n${centers.map(center => excerpt(file, center, radius)).join('\n// ...\n')}`).join('\n\n'),
      placeholders
    };
  }
  let result = payload();
  while (estimateInputTokens(result) > budget && radius > 16) {
    radius = Math.max(16, Math.floor(radius / 2));
    result = payload();
  }
  if (estimateInputTokens(result) > budget) throw new Error(`Chunk batch cannot fit maxInputTokens=${budget}`);
  return result;
}
