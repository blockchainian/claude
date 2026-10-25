import { timed } from './timing.mjs';
import { estimateInputTokens } from './prompt.mjs';

/** Pack one file's pending stage, measuring the complete naming input. */
export function packBatches(batches, render, { maxInputTokens }) {
  return [...iterateBatches(batches, render, { maxInputTokens })];
}

/** Render only the next request, releasing search candidates before yielding. */
export function* iterateBatches(batches, render, { maxInputTokens }) {
  const placeholderCount = batches.reduce((sum, batch) => sum + batch.keys.length, 0);
  for (let start = 0; start < batches.length;) {
    const rendered = new Map(), candidates = new Map();
    function requestFor(length) {
      if (candidates.has(length)) return candidates.get(length);
      let placeholderOffset = 0;
      const parts = batches.slice(start, start + length).map(batch => {
        if (!rendered.has(batch)) rendered.set(batch, render(batch, { placeholderOffset, placeholderCount }));
        const payload = rendered.get(batch);
        placeholderOffset += batch.keys.length;
        return { batch, payload };
      });
      const payload = {
        context: parts.length === 1 ? parts[0].payload.context : parts
          .map(({ batch, payload }) => `// Batch ${batch.id}\n${payload.context}`).join('\n\n'),
        placeholders: Object.assign({}, ...parts.map(part => part.payload.placeholders)),
      };
      const request = { parts, payload, estimated_input_tokens: timed('estimateInputTokens', () => estimateInputTokens(payload)) };
      candidates.set(length, request);
      return request;
    }
    const remaining = batches.length - start;
    let length = 1;
    if (requestFor(1).estimated_input_tokens > maxInputTokens)
      throw new Error(`Batch ${batches[start].id} cannot fit the input token budget (${maxInputTokens}) after context truncation`);
    // Exponential growth avoids encoding every prefix of a large request.
    let upper = 2;
    while (upper <= remaining && requestFor(upper).estimated_input_tokens <= maxInputTokens) {
      length = upper;
      upper *= 2;
    }
    let high = Math.min(upper - 1, remaining);
    while (length < high) {
      const middle = Math.ceil((length + high) / 2);
      if (requestFor(middle).estimated_input_tokens <= maxInputTokens) length = middle;
      else high = middle - 1;
    }
    const request = requestFor(length);
    rendered.clear();
    candidates.clear();
    start += length;
    yield request;
  }
}
