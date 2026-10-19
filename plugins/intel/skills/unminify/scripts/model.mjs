import { createCodexClient } from './codex-api-client.mjs';

export function createNamer(options = {}) {
  const client = createCodexClient(options);
  return {
    async name({ context, placeholders }) {
      const keys = Object.keys(placeholders);
      const schema = { type: 'object', properties: Object.fromEntries(keys.map(key => [key, { type: 'string' }])),
        required: keys, additionalProperties: false };
      const { output, usage } = await client.respond({ model: 'gpt-6-luna', effort: 'low',
        instructions: 'You rename minified JavaScript identifiers. Reply only with JSON.',
        input: [{ type: 'message', role: 'user', content: [{ type: 'input_text',
          text: `Give descriptive names to these JavaScript placeholders: ${keys.join(', ')}. Use camelCase for variables and functions, and PascalCase for classes. Return a JSON object mapping every placeholder to its name.\n\n${context}` }] }],
        schema,
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
