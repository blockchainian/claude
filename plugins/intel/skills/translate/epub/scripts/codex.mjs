// Direct, tool-free Responses calls shared by translation and page transcription.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createCodexClient } from '../../../unminify/scripts/codex-api-client.mjs';

export function textInput(prompt) {
  return [{ role: 'user', content: [{ type: 'input_text', text: prompt }] }];
}

export function requestInput(input, rules) {
  return [{ role: 'developer', content: [{ type: 'input_text', text: rules.trim() }] },
    ...(typeof input === 'string' ? textInput(input) : input)];
}

export function imageInput(prompt, images, bytes = images.map(file => fs.readFileSync(file))) {
  if (!images.length) throw new Error('At least one image is required.');
  const content = images.flatMap((image, index) => {
    const file = path.resolve(image instanceof URL ? fileURLToPath(image) : image);
    const data = bytes[index];
    let mime;
    if (data.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) mime = 'image/png';
    else if (data[0] === 255 && data[1] === 216 && data[2] === 255) mime = 'image/jpeg';
    else if (data.toString('ascii', 0, 4) === 'RIFF' && data.toString('ascii', 8, 12) === 'WEBP') mime = 'image/webp';
    else throw new Error(`Unsupported image: ${file}`);
    return [{ type: 'input_text', text: `<image name=[Image #${index + 1}] path="${file}">` },
      { type: 'input_image', image_url: `data:${mime};base64,${data.toString('base64')}` },
      { type: 'input_text', text: '</image>' }];
  });
  return [{ role: 'user', content: [...content, { type: 'input_text', text: prompt }] }];
}

export async function requestCodex(input, model, effort, serviceTier,
  { events = null, timeout = 3600, rules, schema = null } = {}) {
  const started = performance.now();
  const client = createCodexClient({ concurrency: 1, timeoutMs: timeout * 1000,
    log: event => { if (events) fs.appendFileSync(events, JSON.stringify(event) + '\n'); } });
  try {
    const { output, usage } = await client.respond({ model, effort, serviceTier: serviceTier === 'standard' ? 'default' : serviceTier,
      input: requestInput(input, rules), reasoningContext: 'all_turns', verbosity: 'low', responsesLite: true, ...(schema === null ? {} : { schema }) });
    return [schema === null ? output : JSON.stringify(output), usage, Math.round((performance.now() - started) / 1000)];
  } finally { await client.close(); }
}
