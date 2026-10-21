import { authoredSources } from './parallel-fixture.mjs';

// Authored fragments exercise cross-file reads, aliases, boundaries and invalid input.
export function fragmentSources() {
  const sources = authoredSources(12, 3);
  Object.assign(sources.files, {
    'alias.js': 'import { basename as sharedAlias } from "node:path";\n',
    'declaration.js': 'function sharedAlias(value) { return value + 1; }\n',
    'export.js': 'export { sharedAlias };\n',
    'intrinsic.js': 'const undefined = 7;\n',
    'ordinary.cjs': 'const fs = require("node:fs"); module.exports = fs;\n',
    'invalid.js': 'const = ;\n',
  });
  return sources;
}
