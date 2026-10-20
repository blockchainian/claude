// Ported from wakaru v1.14.0 pipeline (Apache-2.0); independent syntax checkpoint.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { openJournal, atomicWrite, hash } from '../journal.mjs';
import { transformSyntax } from './runner.mjs';
import { registry, rules } from './registry.mjs';
const globals = [...new Set(rules.flatMap(rule => rule.globals ?? []))].sort();
const pipeline = hash(JSON.stringify(registry)
  + ['runner.mjs', 'guards.mjs', 'checks.mjs', 'stage.mjs', ...registry.map(rule => rule.module)]
    .map(file => readFileSync(new URL(file, import.meta.url), 'utf8')).join(''));
export function runSyntaxFile(root, file, {fragment=false, fragmentContext} = {}) {
  const path=join(root,file), source=readFileSync(path,'utf8');
  // Once rename owns a file, its offsets and crash recovery remain authoritative.
  // This also preserves resumability of runs started before the syntax stage existed.
  if (existsSync(join(root,'.unminify',`${file}.jsonl`))) {
    return {status:'syntax-skipped',file};
  }
  const syntaxFile=join('syntax',`${file}.syntax`);
  const options={pipeline,fragment, ...(fragmentContext === undefined ? {} : {shadowedGlobals:globals.filter(name => fragmentContext.topLevelBindings.has(name))})};
  const journal=openJournal(root,syntaxFile,source,options);
  // Journal recovery truncates torn appends before inspecting the durable header.
  const header=JSON.parse(readFileSync(journal.path,'utf8').split('\n')[0]);
  if(header.options!==JSON.stringify(options)) throw new Error(`syntax options changed: ${file}`);
  if(journal.done) return {status:'syntax-skipped',file};
  const result=transformSyntax(source,{fragment,fragmentContext});
  journal.prepare(source,result.source,'syntax');
  if(source!==result.source) atomicWrite(path,result.source);
  journal.complete(result.source,'syntax');
  return {status:'syntax',file,changed:source!==result.source,steps:result.steps.map(({name,edits})=>({name,edits:edits.length}))};
}
