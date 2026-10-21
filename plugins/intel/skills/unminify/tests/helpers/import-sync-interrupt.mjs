import { registerHooks } from 'node:module';

// Interrupt a real atomic replacement after the durable chunk intent was written.
registerHooks({ load(url, context, next) {
  const result = next(url, context);
  if (!url.endsWith('/journal.mjs')) return result;
  const source = String(result.source), needle = '    renameSync(temporary, path);';
  if (!source.includes(needle)) throw new Error('atomic write boundary was not found');
  return { ...result, source: source.replace(needle,
    `${needle}\n    if (path.endsWith('/first.mjs')) process.kill(process.pid, 'SIGKILL');`) };
} });
