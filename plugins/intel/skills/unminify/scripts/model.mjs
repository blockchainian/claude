import { instructions, promptText, estimateInputTokens } from './prompt.mjs';
import { createCodexClient } from './codex-api-client.mjs';

// Read only a flat object's complete JSON string pairs, stopping at a torn or invalid pair.
function completeNames(text, placeholders) {
  const names = new Map();
  let offset = 0;
  const whitespace = () => { while (/[\t\n\r ]/.test(text[offset] ?? '\0')) offset++; };
  function string() {
    whitespace();
    if (text[offset] !== '"') return;
    const start = offset++;
    while (offset < text.length) {
      const char = text[offset++];
      if (char === '\\') { offset++; continue; }
      if (char === '"') {
        try { return JSON.parse(text.slice(start, offset)); } catch { return; }
      }
    }
  }
  whitespace();
  if (text[offset++] !== '{') return {};
  for (;;) {
    const key = string();
    if (key === undefined) break;
    whitespace();
    if (text[offset++] !== ':') break;
    const value = string();
    if (value === undefined) break;
    if (Object.hasOwn(placeholders, key)) names.set(key, value);
    whitespace();
    if (text[offset++] !== ',') break;
  }
  return Object.fromEntries(names);
}

export function createNamer(options = {}) {
  const client = createCodexClient(options);
  async function request(payload, telemetry) {
    const keys = Object.keys(payload.placeholders);
    const schema = { type: 'object', properties: Object.fromEntries(keys.map(key => [key, { type: 'string' }])),
      required: keys, additionalProperties: false };
    const { output, usage } = await client.respond({ model: 'gpt-6-luna', effort: 'low',
      instructions,
      input: [{ type: 'message', role: 'user', content: [{ type: 'input_text', text: promptText(payload) }] }],
      schema, telemetry: { ...telemetry, name_count: keys.length, estimated_input_tokens: estimateInputTokens(payload) },
      recoverIncomplete: text => completeNames(text, payload.placeholders),
      validate(names, { incomplete }) {
        if (!names || typeof names !== 'object' || Array.isArray(names)
          || Object.entries(names).some(([key, name]) => !Object.hasOwn(payload.placeholders, key) || typeof name !== 'string')
          || !incomplete && Object.keys(names).length !== keys.length)
          throw new Error('Naming schema mismatch.');
      },
    });
    return { names: output, usage };
  }
  return {
    async name(payload, telemetry) {
      const reply = await request(payload, telemetry);
      const missing = Object.entries(payload.placeholders).filter(([key]) => !Object.hasOwn(reply.names, key));
      if (!missing.length) return reply;
      try {
        const supplement = await request({ ...payload, placeholders: Object.fromEntries(missing) }, telemetry);
        Object.assign(reply.names, supplement.names);
        for (const [key, value] of Object.entries(supplement.usage)) {
          if (value !== null) reply.usage[key] = (reply.usage[key] ?? 0) + value;
        }
      } catch (error) {
        // Exhausted transient/protocol retries leave accepted names usable; auth/quota still stop the run.
        if (!error.retryable || ['AUTH', 'USAGE_LIMIT'].includes(error.code)) throw error;
      }
      return reply;
    },
    close: () => client.close(),
  };
}
