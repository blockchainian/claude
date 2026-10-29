import { readFileSync, readdirSync, statSync, realpathSync, existsSync } from 'node:fs';
import { join, dirname, relative, resolve, extname, isAbsolute } from 'node:path';
import { randomUUID } from 'node:crypto';
import { createChunkIgnore, loadChunks } from './chunk-manifest.mjs';
import { appendRecord, readRecords, hash } from './journal.mjs';
import { buildDependencyGraph } from './dependency-graph.mjs';
import { clusterGraph } from './communities.mjs';
import { executeMoves } from './file-moves.mjs';
import { packFileTasks, reserveName } from './file-naming.mjs';
import { countTokens } from './prompt.mjs';
import { createCodexClient } from './codex-api-client.mjs';
import { requestStats } from './timing.mjs';

const usageKeys = ['input_tokens', 'cached_input_tokens', 'output_tokens', 'reasoning_output_tokens'];
const instructions = {
  summaries: 'Describe each JavaScript file in one concise line: its purpose, followed by a few keywords. Read only the supplied code. Return a JSON object mapping each task id to its description.',
  directories: 'Name each code directory from its member summaries and child directories. Return a JSON object mapping each task id to a short descriptive lowercase kebab-case basename without extension. Avoid occupied names. No paths.',
  files: 'Name each JavaScript file from its summary and full directory path. Return a JSON object mapping each task id to a short descriptive lowercase kebab-case basename without extension. Avoid occupied names. No paths.',
};

export async function runRenameFiles(root, options, dependencies) {
  const directory = join(root, '.unminify', 'rename-files');
  const runId = randomUUID(), startedAt = Date.now(), attempts = [], pauses = [], phases = {};
  const output = dependencies?.output ?? console.log;
  const event = record => appendRecord(join(directory, 'performance.jsonl'), { ...record, run_id: runId });
  const emit = record => output(JSON.stringify(record));
  const summary = { type: 'summary', run_id: runId, files: 0, completed: 0, skipped: 0, rejected: 0,
    failedBatches: 0, requests: 0, tokens: Object.fromEntries(usageKeys.map(key => [key, 0])), quota: {}, exitCode: 0 };
  const statePath = join(directory, 'stages.jsonl');
  const saved = new Map(readRecords(statePath).map(record => [record.stage, record.value]));
  const save = (stage, value) => { appendRecord(statePath, { stage, value }); saved.set(stage, value); return value; };
  const repliesPath = join(directory, 'replies.jsonl');
  const replies = new Map(readRecords(repliesPath).map(record => [record.id, record.value]));
  let client;
  event({ type: 'run_start', at: startedAt, command: 'rename-files' });
  async function stage(name, task) {
    if (saved.has(name)) return saved.get(name);
    const start = Date.now(); event({ type: 'phase_start', phase: name, at: start });
    try { return save(name, await task()); }
    finally { const end = Date.now(); phases[name] = end - start; event({ type: 'phase_end', phase: name, at: end, started_at: start }); }
  }
  function log(record) {
    record = { ...record, run_id: runId }; attempts.push(record);
    appendRecord(join(directory, 'requests.jsonl'), record);
    if (record.kind !== 'model') return;
    summary.requests++;
    for (const key of usageKeys) summary.tokens[key] += record.usage?.[key] ?? 0;
    for (const [header, value] of Object.entries(record.rate_headers ?? {})) {
      if (!header.endsWith('-used-percent') || !Number.isFinite(Number(value))) continue;
      const entry = summary.quota[header] ?? { before: Number(value) };
      entry.after = Number(value); entry.change = entry.after - entry.before; summary.quota[header] = entry;
    }
  }
  async function ask(kind, tasks) {
    const pending = tasks.filter(task => !replies.has(task.id));
    const batches = packFileTasks(pending, instructions[kind], options.maxInputTokens);
    let cursor = 0, failure;
    async function worker() {
      while (!failure && cursor < batches.length) {
        const batch = batches[cursor++], text = JSON.stringify(batch);
        try {
          client ??= createCodexClient({ concurrency: options.concurrency, codexHome: options.codexHome, log, runId,
            event(record) { event(record); if (record.type === 'pause_end') pauses.push([record.started_at, record.at]); } });
          const ids = batch.map(task => task.id);
          const schema = { type: 'object', properties: Object.fromEntries(ids.map(id => [id, { type: 'string' }])), required: ids, additionalProperties: false };
          const { output: values } = await client.respond({ model: 'gpt-6-luna', effort: 'low', instructions: instructions[kind],
            input: [{ type: 'message', role: 'user', content: [{ type: 'input_text', text }] }], schema,
            telemetry: { phase: kind, file: batch.length === 1 ? batch[0].file : null, batch_ids: ids, name_count: ids.length,
              estimated_input_tokens: countTokens(instructions[kind]) + countTokens(text) },
            validate(values) {
              if (!values || typeof values !== 'object' || Array.isArray(values) || Object.keys(values).length !== ids.length
                || ids.some(id => typeof values[id] !== 'string' || !values[id].trim() || /[\r\n]/.test(values[id])))
                throw new Error('File naming schema mismatch.');
            } });
          for (const task of batch) {
            appendRecord(repliesPath, { id: task.id, value: values[task.id] });
            replies.set(task.id, values[task.id]);
            emit({ file: task.file, stage: kind, status: 'completed' });
          }
        } catch (error) { failure ??= error; }
      }
    }
    await Promise.all(Array.from({ length: Math.min(options.concurrency, batches.length) }, worker));
    if (failure) throw failure;
    return Object.fromEntries(tasks.map(task => [task.id, replies.get(task.id)]));
  }
  try {
    const config = { paths: options.paths.map(path => relative(root, path)).sort(), chunks: options.chunks ?? null,
      ignoreFile: options.ignoreFile ?? null, maxInputTokens: options.maxInputTokens };
    if (saved.has('config') && JSON.stringify(saved.get('config')) !== JSON.stringify(config)) throw new Error('rename-files options changed; resume with the saved selection and token budget.');
    if (!saved.has('config')) save('config', config);
    const snapshot = await stage('graph', async () => {
      const ignores = await createChunkIgnore(root, options.ignoreFile), sources = new Map(), visited = new Set();
      function walk(target) {
        const file = realpathSync(target), local = relative(root, file).split('\\').join('/');
        if (local === '..' || local.startsWith('../') || isAbsolute(local)) throw new Error(`File outside Git root: ${target}`);
        if (visited.has(file) || ignores(statSync(file).isDirectory() ? file + '/' : file)) return;
        visited.add(file);
        if (statSync(file).isDirectory()) {
          for (const entry of readdirSync(file).sort()) if (!['.git', '.unminify', 'node_modules'].includes(entry)) walk(join(file, entry));
        } else if (/\.m?js$/.test(file)) sources.set(local, readFileSync(file, 'utf8'));
      }
      options.paths.forEach(walk);
      if (!sources.size) throw new Error('No JavaScript files selected.');
      if (options.chunks) await loadChunks(options.chunks, root);
      const files = [...sources.keys()].sort();
      const graph = buildDependencyGraph(files, { root, sources });
      return { graph, sources: Object.fromEntries(sources), chunksHash: options.chunks ? hash(readFileSync(options.chunks)) : null, ignoreHash: hash(await readIgnore(root, options.ignoreFile)) };
    });
    if (snapshot.ignoreHash !== hash(await readIgnore(root, options.ignoreFile))) throw new Error('rename-files ignore rules changed.');
    if (options.chunks && !existsSync(join(directory, 'moves.json')) && snapshot.chunksHash !== hash(readFileSync(options.chunks))) throw new Error('rename-files chunk manifest changed.');
    if (!existsSync(join(directory, 'moves.json'))) {
      for (const [file, source] of Object.entries(snapshot.sources)) {
        if (!existsSync(join(root, file)) || hash(readFileSync(join(root, file))) !== hash(source)) throw new Error(`source changed: ${file}`);
      }
    }
    summary.files = Object.keys(snapshot.sources).length;
    const summaries = await stage('summaries', () => ask('summaries', Object.entries(snapshot.sources)
      .map(([file, text], index) => ({ id: `summary-${index}`, file, text }))));
    const fileSummaries = Object.fromEntries(Object.keys(snapshot.sources).map((file, index) => [file, summaries[`summary-${index}`]]));
    const tree = await stage('communities', async () => clusterGraph(snapshot.graph));
    let counter = 0;
    function index(node) { node.id = `directory-${counter++}`; node.children.forEach(index); }
    index(tree);
    const descendants = node => [...node.files, ...node.children.flatMap(descendants)];
    const directories = await stage('directories', async () => {
      const names = {};
      async function nameLevel(nodes) {
        const children = nodes.flatMap(node => node.children);
        if (children.length) await nameLevel(children);
        for (const node of nodes) {
          const occupied = new Set();
          for (const child of node.children) names[child.id] = await reserveName(names[child.id], occupied, async occupiedNames =>
            (await ask('directories', [{ id: `${child.id}-collision`, summaries: descendants(child).map(file => fileSummaries[file]), occupied: occupiedNames }]))[`${child.id}-collision`]);
        }
        const tasks = nodes.map(node => ({ id: node.id, summaries: descendants(node).map(file => fileSummaries[file]),
          children: node.children.map(child => names[child.id]) }));
        const proposed = await ask('directories', tasks);
        // Each sibling set reserves names in deterministic node order.
        for (const node of nodes) names[node.id] = proposed[node.id];
      }
      await nameLevel([tree]);
      let base = dirname(Object.keys(snapshot.sources)[0]);
      while (Object.keys(snapshot.sources).some(file => relative(base, file).startsWith('..'))) base = dirname(base);
      const occupied = new Set(readdirSync(join(root, base)));
      names[tree.id] = await reserveName(names[tree.id], occupied, async occupiedNames =>
        (await ask('directories', [{ id: `${tree.id}-collision`, summaries: descendants(tree).map(file => fileSummaries[file]), occupied: occupiedNames }]))[`${tree.id}-collision`]);
      return names;
    });
    const plan = await stage('files', async () => {
      let base = dirname(Object.keys(snapshot.sources)[0]);
      while (Object.keys(snapshot.sources).some(file => relative(base, file).startsWith('..'))) base = dirname(base);
      const nodes = [], folders = new Map();
      function paths(node, parent) {
        const path = join(parent, directories[node.id]); folders.set(node.id, path); nodes.push(node);
        node.children.forEach(child => paths(child, path));
      }
      paths(tree, base);
      const taskList = nodes.flatMap(node => node.files.map(file => ({ id: `file-${Object.keys(snapshot.sources).indexOf(file)}`, file,
        summary: fileSummaries[file], directory: folders.get(node.id) })));
      const proposed = await ask('files', taskList), moves = [];
      for (const node of nodes) {
        const folder = folders.get(node.id), occupied = new Set(node.children.map(child => directories[child.id]));
        if (existsSync(join(root, folder))) for (const entry of readdirSync(join(root, folder))) occupied.add(entry.replace(/\.m?js$/, ''));
        for (const file of node.files) {
          const id = taskList.find(task => task.file === file).id;
          const name = await reserveName(proposed[id], occupied, async occupiedNames => (await ask('files', [{ id: `${id}-collision`, file,
            summary: fileSummaries[file], directory: folder, occupied: occupiedNames }]))[`${id}-collision`]);
          const imports = snapshot.graph.nodes.find(node => node.id === file).imports;
          moves.push({ from: file, to: join(folder, name + extname(file)).split('\\').join('/'), source: snapshot.sources[file], imports });
        }
      }
      return { moves };
    });
    await stage('moves', async () => executeMoves(root, { moves: plan.moves, chunks: options.chunks, journalDir: directory,
      onProgress: record => emit({ stage: 'moves', ...record }) }));
    const maxDepth = node => 1 + Math.max(0, ...node.children.map(maxDepth));
    const count = node => 1 + node.children.reduce((sum, child) => sum + count(child), 0);
    const reportTree = node => ({ name: directories[node.id], files: node.files.map(file => plan.moves.find(move => move.from === file).to), children: node.children.map(reportTree) });
    Object.assign(summary, { completed: summary.files, directories: count(tree), max_depth: maxDepth(tree), tree: reportTree(tree) });
    // A durable completion marker is the authorization boundary for subsequent stages.
    const { writeFileSync, openSync, fsyncSync, closeSync, renameSync } = await import('node:fs');
    const marker = join(directory, 'complete.json'), temporary = join(directory, 'complete.tmp');
    writeFileSync(temporary, JSON.stringify({ complete: true, tree: summary.tree }));
    const fd = openSync(temporary, 'r'); try { fsyncSync(fd); } finally { closeSync(fd); }
    renameSync(temporary, marker);
    const directoryFd = openSync(directory, 'r'); try { fsyncSync(directoryFd); } finally { closeSync(directoryFd); }
    return summary;
  } catch (error) {
    summary.exitCode = 1; summary.reason = error.message;
    emit({ status: 'interrupted', reason: error.message }); return summary;
  } finally {
    await client?.close();
    summary.total_ms = Date.now() - startedAt; summary.phase_ms = phases;
    Object.assign(summary, requestStats(attempts, pauses, summary.total_ms));
    event({ type: 'run_end', at: Date.now(), total_ms: summary.total_ms }); event({ ...summary, at: Date.now() }); emit(summary);
  }
}

async function readIgnore(root, file) {
  try { return readFileSync(file ?? join(root, '.ignore'), 'utf8'); }
  catch (error) { if (error.code === 'ENOENT' && !file) return ''; throw error; }
}
