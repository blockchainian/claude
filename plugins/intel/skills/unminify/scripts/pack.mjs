/** Pack one file's pending stage in order, using rendered UTF-8 context sizes. */
export function packBatches(batches, render, { maxIds, contextBytes }) {
  const requests = [];
  let current = null, count = 0;
  function requestFor(items, placeholderCount) {
    let placeholderOffset = 0;
    const parts = items.map(batch => {
      const payload = render(batch, { placeholderOffset, placeholderCount });
      placeholderOffset += batch.keys.length;
      return { batch, payload };
    });
    return { parts, payload: {
      context: parts.length === 1 ? parts[0].payload.context : parts
        .map(({ batch, payload }) => `// Batch ${batch.id}\n${payload.context}`).join('\n\n'),
      placeholders: Object.assign({}, ...parts.map(part => part.payload.placeholders)),
    } };
  }
  function flush() {
    if (current) requests.push(current);
    current = null; count = 0;
  }
  for (const batch of batches) {
    if (current && count + batch.keys.length > maxIds) flush();
    const items = [...(current?.parts.map(part => part.batch) ?? []), batch];
    let candidate = requestFor(items, count + batch.keys.length);
    if (current && Buffer.byteLength(candidate.payload.context, 'utf8') > contextBytes) {
      flush();
      candidate = requestFor([batch], batch.keys.length);
    }
    current = candidate;
    count += batch.keys.length;
    // An oversized original batch keeps its own request.
    if (count >= maxIds || Buffer.byteLength(current.payload.context, 'utf8') > contextBytes) flush();
  }
  flush();
  return requests;
}
