import { mkdtemp, mkdir, writeFile, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { run } from '../../scripts/cli.mjs';
import * as select from '../../scripts/select.mjs';
import * as syntax from '../../scripts/syntax/stage.mjs';
import { runMergeFile } from '../../scripts/merge-imports.mjs';
import { createChunkRun } from '../../scripts/chunk-run.mjs';
import { loadFragmentContext } from '../../scripts/fragments.mjs';

// Authored here: these modules and slices contain no upstream application code.
export function authoredSources(count = 8, chunkCount = 3) {
  const files = {}, groups = [];
  for (let index = 0; index < count; index++) {
    files[`standalone-${index}.js`] = `import { basename as pathName } from "node:path";\nimport { basename as otherName } from "node:path";\nexport const flag${index} = !0;\nexport function read${index}(input) { return (console.log(pathName(input), otherName(input)), void 0); }\n`;
  }
  for (let index = 0; index < chunkCount; index++) {
    const names = ['first', 'middle', 'last'].map(part => `chunk-${index}/${part}.js`);
    files[names[0]] = `import { basename as firstPath${index} } from "node:path";\nconst sharedFlag${index} = !0;\n`;
    files[names[1]] = `import { basename as secondPath${index} } from "node:path";\nfunction readChunk${index}(input) { return (console.log(firstPath${index}(input), secondPath${index}(input)), void 0); }\n`;
    files[names[2]] = `console.log(sharedFlag${index}, readChunk${index}("sample"), !1); export { sharedFlag${index} };\n`;
    groups.push({ id: `chunk-${index}`, files: names });
  }
  return { files, groups };
}

export async function parallelFixture(t, sources = authoredSources()) {
  const root = await mkdtemp(join(tmpdir(), 'unminify-parallel-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, source] of Object.entries(sources.files)) {
    await mkdir(join(root, file, '..'), { recursive: true });
    await writeFile(join(root, file), source);
  }
  const chunks = join(root, 'chunks.json');
  await writeFile(chunks, JSON.stringify({ version: 1, chunks: sources.groups }));
  const lines = [];
  return { root, chunks, files: sources.files, groups: sources.groups, lines,
    async contents() {
      return new Map(await Promise.all(Object.keys(sources.files).sort().map(async file => [file, await readFile(join(root, file))])));
    },
    run(command = 'restore-syntax', syntaxPool = { size: 3 }, output = line => lines.push(line)) {
      return run({ command, paths: [root], chunks, minSymbolLength: 1 }, { ...select, ...syntax, syntaxPool, output });
    },
    async serial() {
      const files = Object.keys(sources.files).sort();
      const driver = await createChunkRun(root, { command: 'restore-syntax', chunks, minSymbolLength: 1, maxInputTokens: 100000, chunkSiblings: true });
      const context = loadFragmentContext(root, files);
      driver.select(files, context.fragments);
      for (const file of files) if (!driver.files.has(file)) {
        syntax.runSyntaxFile(root, file, { fragment: context.fragments.has(file), fragmentContext: context });
      }
      for (const file of files) if (!driver.files.has(file)) runMergeFile(root, file);
      const summary = { rejected: 0 };
      await driver.restore({ emit() {}, summary, sources: new Map(), phase: () => () => {}, fragmentContext: context });
      if (summary.rejected) throw new Error('authored serial fixture was rejected');
    },
    async journals() {
      const result = new Map();
      async function visit(directory, relative = '') {
        for (const entry of await readdir(directory, { withFileTypes: true })) {
          const name = join(relative, entry.name), path = join(directory, entry.name);
          if (entry.isDirectory()) await visit(path, name);
          else result.set(name, await readFile(path));
        }
      }
      for (const directory of ['syntax', 'imports']) await visit(join(root, '.unminify', 'restore-syntax', directory), directory);
      return new Map([...result].sort(([left], [right]) => left.localeCompare(right)));
    },
  };
}
