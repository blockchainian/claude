import test from 'node:test';
import assert from 'node:assert/strict';
import { clusterGraph } from '../scripts/communities.mjs';
const node = id => ({ id });
const leaves = tree => tree.children.length ? tree.children.flatMap(leaves) : tree.files;
const graph = {
  nodes: ['a','b','c','d','e','f'].map(node),
  edges: [['a','b'],['b','c'],['c','a'],['d','e'],['e','f'],['f','d']].map(([source,target]) => ({source,target})),
};
test('Leiden finds disconnected dense groups and terminates without duplicate members', () => {
  const tree = clusterGraph(graph);
  assert.deepEqual(tree, {files: [], children: [
    {files: ['a','b','c'], children: []}, {files: ['d','e','f'], children: []},
  ]});
  assert.deepEqual(leaves(tree).sort(), ['a','b','c','d','e','f']);
});
test('seeded communities are independent of node and edge insertion order', () => {
  assert.deepEqual(clusterGraph(graph), clusterGraph({nodes: [...graph.nodes].reverse(), edges:[...graph.edges].reverse()}));
});
test('empty and indivisible graphs are leaves', () => {
  assert.deepEqual(clusterGraph({nodes:[],edges:[]}), {files:[],children:[]});
  assert.deepEqual(clusterGraph({nodes:[node('a')],edges:[]}), {files:['a'],children:[]});
});
test('IQR incoming hubs become their own directory only when removal improves modularity', () => {
  const nodes = [...graph.nodes, node('hub')];
  const edges = [...graph.edges, ...graph.nodes.map(({id}) => ({source:id,target:'hub'}))];
  assert.deepEqual(clusterGraph({nodes,edges}), {files:[],children:[
    {files:['a','b','c'],children:[]},
    {files:['d','e','f'],children:[]},
    {files:['hub'],children:[]},
  ]});
  const star = { nodes, edges: graph.nodes.map(({id}) => ({source:id,target:'hub'})) };
  assert.deepEqual(clusterGraph(star), {files:['a','b','c','d','e','f','hub'],children:[]});
});
test('a graph with several nodes and no dependency structure stays in one leaf directory', () => {
  assert.deepEqual(clusterGraph({nodes:['c','a','b'].map(node),edges:[]}), {files:['a','b','c'],children:[]});
});
