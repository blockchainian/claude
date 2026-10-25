import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';

// Authored fragments share an import and references across one program scope.
export function importSyncSources() {
  const files = {
    'first.mjs': 'import { readFile as a } from "node:fs";\nimport { helper as h } from "./helper.mjs";\n',
    'middle.mjs': 'function b(input) { return h(a(input)); }\n',
    'last.mjs': 'export { b as readInput };\nconsole.log(b("sample"));\n',
    'helper.mjs': 'export function helper(value) { return value; }\n',
  };
  for (let index = 0; index < 8; index++) files[`independent-${index}.mjs`] = 'import { basename as p } from "node:path";\nimport { readInput as c } from "./last.mjs";\nconsole.log(p("sample"), c);\n';
  return { files, groups: [{ id: 'shared-import', files: ['first.mjs', 'middle.mjs', 'last.mjs'] }] };
}

export async function writeImportSyncFixture(root) {
  const { files, groups } = importSyncSources();
  await mkdir(root, { recursive: true });
  execFileSync('git', ['init', '--initial-branch=main', root], { stdio: 'ignore' });
  for (const [file, source] of Object.entries(files)) await writeFile(join(root, file), source);
  await writeFile(join(root, 'chunks.json'), JSON.stringify({ version: 1, chunks: groups }));
}

export async function importSyncSnapshot(root) {
  const files = [], journals = [];
  for (const file of Object.keys(importSyncSources().files).sort()) files.push([file, await readFile(join(root, file), 'utf8')]);
  async function visit(path, relative = '') {
    for (const entry of await readdir(path, { withFileTypes: true })) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.isDirectory()) await visit(join(path, entry.name), name);
      else if (name !== 'performance.jsonl') journals.push([name, await readFile(join(path, entry.name), 'utf8')]);
    }
  }
  await visit(join(root, '.unminify'));
  journals.sort(([left], [right]) => left.localeCompare(right));
  return { files, journals };
}
