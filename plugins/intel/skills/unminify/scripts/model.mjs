import { instructions, promptText, estimateInputTokens } from './prompt.mjs';
import { createCodexClient } from './codex-api-client.mjs';

export function createNamer(options = {}) {
  const client = createCodexClient(options);
  return {
    async name(payload, telemetry) {
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
    },
    close: () => client.close(),
  };
}
