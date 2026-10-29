import createGraph from 'ngraph.graph';
import { detectClusters } from 'ngraph.leiden';

const seed = 42;

function quartile(sorted, fraction) {
  const index = (sorted.length - 1) * fraction;
  const lower = Math.floor(index);
  return sorted[lower] + (sorted[Math.ceil(index)] - sorted[lower]) * (index - lower);
}

/** Recursively partition dependency structure with seeded Leiden and IQR hub extraction. */
export function clusterGraph(graph) {
  const ids = graph.nodes.map(node => typeof node === 'string' ? node : node.id).sort();
  const all = new Set(ids);
  const edges = graph.edges.filter(edge => all.has(edge.source) && all.has(edge.target))
    .sort((a, b) => a.source.localeCompare(b.source) || a.target.localeCompare(b.target));
  function partition(members, links) {
    if (members.length < 2 || links.length === 0) {
      return { groups: members.length ? [members] : [], quality: 0 };
    }
    const network = createGraph();
    for (const id of members) network.addNode(id);
    for (const { source, target } of links) network.addLink(source, target);
    const result = detectClusters(network, { randomSeed: seed, refine: true, maxLevels: members.length + 1 });
    const groups = [...result.getCommunities().values()].map(group => group.sort())
      .sort((a, b) => a[0].localeCompare(b[0]));
    return { groups, quality: result.quality() };
  }
  function split(members, links) {
    if (members.length <= 1) return { files: members, children: [] };
    const baseline = partition(members, links);
    const incoming = new Map(members.map(id => [id, 0]));
    for (const { target } of links) incoming.set(target, incoming.get(target) + 1);
    const degrees = [...incoming.values()].sort((a, b) => a - b);
    const q1 = quartile(degrees, 0.25), q3 = quartile(degrees, 0.75);
    const hubs = members.filter(id => incoming.get(id) > q3 + 1.5 * (q3 - q1));
    if (hubs.length && hubs.length < members.length) {
      const common = new Set(hubs);
      const rest = members.filter(id => !common.has(id));
      const restEdges = links.filter(edge => !common.has(edge.source) && !common.has(edge.target));
      const remainder = partition(rest, restEdges);
      if (remainder.groups.length >= 2 && remainder.quality > baseline.quality) {
        const groups = [hubs, ...remainder.groups].sort((a, b) => a[0].localeCompare(b[0]));
        return { files: [], children: groups.map(group => descend(group, links)) };
      }
    }
    if (baseline.groups.length < 2) return { files: members, children: [] };
    return { files: [], children: baseline.groups.map(group => descend(group, links)) };
  }
  function descend(group, links) {
    const included = new Set(group);
    return split(group, links.filter(edge => included.has(edge.source) && included.has(edge.target)));
  }
  return split(ids, edges);
}

export const buildCommunityTree = clusterGraph;
