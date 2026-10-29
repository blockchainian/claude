import { countTokens } from './prompt.mjs';

export function packFileTasks(tasks, instructions, maxInputTokens) {
  const batches = [];
  let batch = [];
  const fits = items => countTokens(instructions) + countTokens(JSON.stringify(items)) <= maxInputTokens;
  for (const task of tasks) {
    if (!fits([task])) throw new Error(`Task ${task.id} cannot fit the input token budget (${maxInputTokens})`);
    if (batch.length && !fits([...batch, task])) { batches.push(batch); batch = []; }
    batch.push(task);
  }
  if (batch.length) batches.push(batch);
  return batches;
}

export async function reserveName(proposed, occupied, retry) {
  function validate(name) {
    if (typeof name !== 'string' || !/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(name)
      || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i.test(name)) throw new Error(`Invalid file or directory name: ${name}`);
    return name;
  }
  let name = validate(proposed);
  if (occupied.has(name)) name = validate(await retry([...occupied].sort()));
  if (occupied.has(name)) {
    let suffix = 2;
    while (occupied.has(`${name}${suffix}`)) suffix++;
    name += suffix;
  }
  occupied.add(name);
  return name;
}
