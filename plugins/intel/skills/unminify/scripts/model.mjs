import { instructions, promptText, estimateInputTokens } from './prompt.mjs';
import { createCodexClient } from './codex-api-client.mjs';

// Keep large strict-schema replies below the observed output truncation range.
const maxNamesPerRequest = 1000;

export function createNamer(options = {}) {
  const client = createCodexClient(options);
  async function namePart(payload, telemetry) {
    const keys = Object.keys(payload.placeholders);
    const schema = { type: 'object', properties: Object.fromEntries(keys.map(key => [key, { type: 'string' }])),
      required: keys, additionalProperties: false };
    const { output, usage } = await client.respond({ model: 'gpt-6-luna', effort: 'low',
      instructions,
      input: [{ type: 'message', role: 'user', content: [{ type: 'input_text',
        text: promptText(payload) }] }],
      schema, telemetry: { ...telemetry, estimated_input_tokens: estimateInputTokens(payload) },
      validate(names) {
        if (!names || typeof names !== 'object' || Array.isArray(names) || Object.keys(names).length !== keys.length
          || keys.some(key => !Object.hasOwn(names, key) || typeof names[key] !== 'string'))
          throw new Error('Naming schema mismatch.');
      },
    });
    return { names: output, usage };
  }
  return {
    async name(payload, telemetry) {
      const entries = Object.entries(payload.placeholders);
      if (entries.length <= maxNamesPerRequest) return namePart(payload, telemetry);
      const names = {}, usage = {};
      const count = Math.ceil(entries.length / maxNamesPerRequest);
      for (let index = 0; index < count; index++) {
        const placeholders = Object.fromEntries(entries.slice(index * maxNamesPerRequest, (index + 1) * maxNamesPerRequest));
        const reply = await namePart({ ...payload, placeholders }, { ...telemetry,
          name_count: Object.keys(placeholders).length, subrequest_index: index + 1, subrequest_count: count });
        Object.assign(names, reply.names);
        for (const [key, value] of Object.entries(reply.usage)) usage[key] = (usage[key] ?? 0) + value;
      }
      return { names, usage };
    },
    close: () => client.close(),
  };
}
