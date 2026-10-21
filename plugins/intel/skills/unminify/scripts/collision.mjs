import { estimateInputTokens } from './prompt.mjs';

/** Reconsider each colliding proposal once; durable replies survive interrupted rewrites. */
export async function reconsiderNames({ proposals, resolve, journal, id, namer, maxInputTokens, telemetry }) {
  const attempted = new Set();
  for (;;) {
    const collisions = [];
    resolve(proposals, collision => collisions.push(collision));
    const collision = collisions.find(({ key }) => !attempted.has(key));
    if (!collision) return resolve(proposals);
    attempted.add(collision.key);
    const retryId = `collision:${id}:${collision.key}`;
    let names = journal.replies.get(retryId);
    if (!names) {
      const prefix = `The name ${collision.proposed} is occupied. Choose a meaningful distinct name for v1. Occupied names: ${collision.occupied.join(', ')}.\nBinding context:\n`;
      const payload = { placeholders: { v1: collision.key }, context: prefix + collision.context };
      if (estimateInputTokens(payload) > maxInputTokens) throw new Error(`Collision batch ${retryId} cannot fit the input token budget (${maxInputTokens})`);
      const response = await namer.name(payload, { ...telemetry, batch_id: retryId, batch_ids: [retryId], name_count: 1, phase: 'model', queued_at: Date.now() });
      if (!response.names || Object.keys(response.names).length !== 1 || typeof response.names.v1 !== 'string') throw new Error(`invalid names for ${retryId}`);
      journal.response(retryId, new Map([[collision.key, response.names.v1]]));
      names = journal.replies.get(retryId);
    }
    proposals.set(collision.key, names[collision.key]);
  }
}
