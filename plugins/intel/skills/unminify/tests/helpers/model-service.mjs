import { createServer } from 'node:http2';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

// A local Responses transport; callers exercise the real authenticated model client.
export async function modelService(t, root, handler) {
  const codexHome = join(root, '.auth');
  await mkdir(codexHome);
  const token = `e30.${Buffer.from(JSON.stringify({ exp: Date.now() / 1000 + 3600 })).toString('base64url')}.local`;
  await writeFile(join(codexHome, 'auth.json'), JSON.stringify({ auth_mode: 'chatgpt',
    tokens: { access_token: token, refresh_token: 'local-refresh', account_id: 'local-account' } }));
  const server = createServer(handler);
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  return { codexHome, baseUrl: `http://127.0.0.1:${server.address().port}` };
}

export function complete(res, names, count = 1) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'x-codex-primary-used-percent': String(count) });
  res.end(`data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
    output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: JSON.stringify(names) }] }],
    usage: { input_tokens: 10, input_tokens_details: { cached_tokens: 2 }, output_tokens: 3,
      output_tokens_details: { reasoning_tokens: 1 } } } })}\n\n`);
}
