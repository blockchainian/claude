#!/usr/bin/env node
import { mkdtemp, mkdir, copyFile, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

async function main(args) {
  if (!args.length) throw new Error('Usage: node e2e.mjs <sample.js> [sample.mjs ...]');
  const samples = args.map(resolvePath => resolve(resolvePath));
  const names = samples.map(sample => basename(sample));
  if (new Set(names).size !== names.length) throw new Error('Sample filenames must be unique');
  if (names.some(name => !/\.m?js$/.test(name))) throw new Error('Samples must be .js or .mjs files');
  const temporary = await mkdtemp(join(tmpdir(), 'unminify-e2e-'));
  const repo = join(temporary, 'repo');
  await mkdir(repo);
  execFileSync('git', ['init', '--initial-branch=main', repo], { stdio: 'ignore' });
  const originals = new Map();
  for (const [index, source] of samples.entries()) {
    originals.set(names[index], await readFile(source, 'utf8'));
    await copyFile(source, join(repo, names[index]));
  }
  await writeFile(join(repo, 'package.json'), '{"type":"module"}\n');
  console.log(`Temporary repository: ${repo}`);
  console.log('Calling the real model using the current Codex login.');
  const child = spawn(process.execPath, [fileURLToPath(new URL('./cli.mjs', import.meta.url)), repo], { stdio: ['ignore', 'pipe', 'inherit'] });
  let transcript = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => { transcript += chunk; process.stdout.write(chunk); });
  const result = await new Promise((resolveResult, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolveResult({ code, signal }));
  });
  for (const name of names) {
    console.log(`\n--- ${name}: before ---\n${originals.get(name)}`);
    console.log(`--- ${name}: after ---\n${await readFile(join(repo, name), 'utf8')}`);
  }
  const summary = transcript.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line)).find(record => record.type === 'summary');
  if (summary) console.log(JSON.stringify({ requests: summary.requests, tokens: summary.tokens, quota: summary.quota }));
  else throw new Error(`CLI did not produce a summary (${result.signal ?? result.code})`);
  process.exitCode = result.code ?? 1;
}

try { await main(process.argv.slice(2)); }
catch (error) { console.error(error.message); process.exitCode = 1; }
