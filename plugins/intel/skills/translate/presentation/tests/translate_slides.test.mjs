import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { problem, parseArgs } from '../scripts/translate_slides.mjs';

test('slide defaults retain the model, effort and priority tier', () => {
  const opt = parseArgs(['work']);
  assert.equal(opt.model, 'gpt-6-luna');
  assert.equal(opt.effort, 'low');
  assert.equal(opt.serviceTier, 'priority');
});
test('slide translations preserve style, code, line breaks and picture tokens', () => {
  const src = '<s1>Read</s1><br><c2>return x;</c2> ⟦E1⟧';
  const zh = '<s1>读取</s1><br><c2>return x;</c2> ⟦E1⟧';
  assert.equal(problem(src, zh), null);
  assert.match(problem(src, zh.replace('return x;', '返回 x;')), /code/);
  assert.match(problem(src, zh.replace('⟦E1⟧', '')), /tags/);
  assert.match(problem(src, zh.replace('<br>', '')), /tags/);
});
test('a completed slide deck resumes without any model request', t => {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'slides-resume-'));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  fs.writeFileSync(path.join(work, 'blocks.json'), JSON.stringify([{ text: 'Computer Programming as an Art' }]));
  const translations = { 'Computer Programming as an Art': '计算机程序设计的艺术' };
  fs.writeFileSync(path.join(work, 'zh.json'), JSON.stringify(translations));
  const r = spawnSync(process.execPath, [new URL('../scripts/translate_slides.mjs', import.meta.url).pathname, work],
    { encoding: 'utf8', env: { ...process.env, CODEX_HOME: path.join(work, 'no-login'), PATH: '' } });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /0 texts to translate/);
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(work, 'zh.json'))), translations);
});
