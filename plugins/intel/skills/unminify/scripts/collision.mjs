import { createHash } from 'node:crypto';
import { estimateInputTokens } from './prompt.mjs';

/** Connected collisions share a request so their replacements can be chosen together. */
function collisionGroups(collisions) {
  const parents = collisions.map((_, index) => index);
  function root(index) {
    while (parents[index] !== index) {
      parents[index] = parents[parents[index]];
      index = parents[index];
    }
    return index;
  }
  const proposed = new Map();
  for (const [index, collision] of collisions.entries()) {
    if (proposed.has(collision.proposed)) parents[root(index)] = root(proposed.get(collision.proposed));
    else proposed.set(collision.proposed, index);
  }
  for (const [index, collision] of collisions.entries()) for (const name of collision.occupied) {
    if (proposed.has(name)) parents[root(index)] = root(proposed.get(name));
  }
  const groups = new Map();
  for (const [index, collision] of collisions.entries()) {
    const key = root(index);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(collision);
  }
  return [...groups.values()];
}

/** Reconsider each colliding proposal once; durable replies survive interrupted rewrites. */
export async function reconsiderNames({ proposals, resolve, journal, id, namer, maxInputTokens, telemetry }) {
  const attempted = new Set();
  function requestId(group) {
    const keys = group.map(collision => collision.key);
    return group.length === 1 ? `collision:${id}:${keys[0]}`
      : `collision-group:${id}:${createHash('sha256').update(JSON.stringify(keys)).digest('hex')}`;
  }
  for (;;) {
    const collisions = [];
    resolve(proposals, collision => { if (!attempted.has(collision.key)) collisions.push(collision); });
    if (!collisions.length) return resolve(proposals);
    // Old journals were produced one binding at a time, resolving again after every reply.
    const saved = collisions.find(collision => journal.replies.has(`collision:${id}:${collision.key}`));
    if (saved) {
      attempted.add(saved.key);
      proposals.set(saved.key, journal.replies.get(`collision:${id}:${saved.key}`)[saved.key]);
      continue;
    }
    const savedWhole = collisionGroups(collisions).find(group => journal.replies.has(requestId(group)));
    if (savedWhole) {
      const names = journal.replies.get(requestId(savedWhole));
      for (const { key } of savedWhole) {
        attempted.add(key);
        if (Object.hasOwn(names, key)) proposals.set(key, names[key]);
        else proposals.delete(key);
      }
      continue;
    }
    // Split replies remain reusable when the budget or remaining collision groups change.
    const keysByText = new Map([...proposals.keys()].map(key => [String(key), key]));
    const collidingKeys = new Set(collisions.map(collision => String(collision.key)));
    const savedGroup = [...journal.replies].find(([request, names]) =>
      request.startsWith(`collision-group:${id}:`) && Object.keys(names).some(key => collidingKeys.has(key))
      && Object.keys(names).every(key => keysByText.has(key)));
    if (savedGroup) {
      for (const [text, name] of Object.entries(savedGroup[1])) {
        const key = keysByText.get(text);
        if (attempted.has(key)) continue;
        attempted.add(key);
        proposals.set(key, name);
      }
      continue;
    }
    function requestsFor(group) {
      const keys = group.map(collision => collision.key);
      const retryId = requestId(group);
      const occupied = [...new Set(group.flatMap(collision => collision.occupied))];
      const payload = {
        placeholders: Object.fromEntries(keys.map((key, index) => [`v${index + 1}`, key])),
        context: `Choose meaningful names for the placeholders that are distinct from one another and not in occupied names. Occupied names: ${occupied.join(', ')}.\n`
          + group.map((collision, index) => `The name ${collision.proposed} is occupied. Choose a meaningful distinct name for v${index + 1}.\nBinding context:\n${collision.context}`).join('\n\n'),
      };
      if (!journal.replies.has(retryId) && estimateInputTokens(payload) > maxInputTokens) {
        if (group.length === 1)
          throw new Error(`Collision batch ${retryId} cannot fit the input token budget (${maxInputTokens})`);
        const middle = Math.floor(group.length / 2);
        return [...requestsFor(group.slice(0, middle)), ...requestsFor(group.slice(middle))];
      }
      return [{ group, retryId, payload }];
    }
    const requests = collisionGroups(collisions).flatMap(requestsFor);
    // The caller's shared slot limits requests and stops queued work on AUTH/USAGE_LIMIT.
    const replies = await Promise.allSettled(requests.map(async ({ retryId, payload }) => {
      if (journal.replies.has(retryId)) return journal.replies.get(retryId);
      const response = await namer.name(payload, { ...telemetry, batch_id: retryId, batch_ids: [retryId],
        name_count: Object.keys(payload.placeholders).length, phase: 'model', queued_at: Date.now() });
      const entries = Object.entries(payload.placeholders);
      if (!response.names || Object.entries(response.names).some(([placeholder, name]) => !Object.hasOwn(payload.placeholders, placeholder) || typeof name !== 'string'))
        throw new Error(`invalid names for ${retryId}`);
      return Object.fromEntries(entries.filter(([placeholder]) => Object.hasOwn(response.names, placeholder))
        .map(([placeholder, key]) => [key, response.names[placeholder]]));
    }));
    let failure;
    // Drain accepted replies even when a peer fails; arrival order never changes the journal.
    for (const [index, { group, retryId }] of requests.entries()) {
      const reply = replies[index];
      if (reply.status === 'rejected') { failure ??= reply.reason; continue; }
      const names = new Map(group.filter(({ key }) => Object.hasOwn(reply.value, key)).map(({ key }) => [key, reply.value[key]]));
      if (names.size) journal.response(retryId, names);
      for (const { key } of group) {
        attempted.add(key);
        if (Object.hasOwn(reply.value, key)) proposals.set(key, reply.value[key]);
        else proposals.delete(key);
      }
    }
    if (failure) throw failure;
  }
}
