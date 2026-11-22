import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/split-layout.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {leaf,leafIds,splitLeaf,moveLeaf,removeLeaf,moveTab,normalizeLayout,resizeSplit}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
test('nested terminal moves retain each session exactly once and close collapses its sibling',()=>{
  let tree=splitLeaf(leaf('a'),'a',leaf('b'),'right');tree=splitLeaf(tree,'b',leaf('c'),'bottom');
  tree=moveLeaf(tree,'a','c','bottom');assert.deepEqual(leafIds(tree),['b','c','a']);
  assert.equal(tree.second.axis,'vertical');assert.deepEqual(leafIds(removeLeaf(tree,'c')),['b','a']);
  assert.deepEqual(moveLeaf(tree,'a','a','right'),tree);assert.deepEqual(moveLeaf(tree,'external','a','left'),tree);
});
test('moving tabs to nested edges preserves tabs, removes empty source groups and ignores external drops',()=>{
  const tab=id=>({id,kind:'files',target:id,title:id});
  const original={revision:5,axis:'horizontal',ratio:.65,closed:[],panes:[{id:'main',active:'a',tabs:[tab('a'),tab('b')]},{id:'other',active:'c',tabs:[tab('c')]}]};
  let result=moveTab(original,'b','other','bottom');assert.equal(result.layout.panes.length,3);assert.equal(result.layout.tree.ratio,.65);
  result=moveTab(result.layout,'c','main');assert.equal(result.layout.panes.length,2);assert.deepEqual(result.layout.panes.flatMap(p=>p.tabs.map(t=>t.id)).sort(),['a','b','c']);
  assert.equal(moveTab(original,'a','main'),undefined);assert.equal(moveTab(original,'unknown','main','right'),undefined);
  assert.equal(original.panes[0].tabs.length,2,'caller layout is not mutated');
});
test('legacy producer appends a pane without losing saved boundaries',()=>{
  let tree=splitLeaf(leaf('main'),'main',leaf('other'),'bottom');tree=resizeSplit(tree,tree.id,.7);
  const layout=normalizeLayout({revision:1,axis:'vertical',ratio:.7,closed:[],tree,panes:['main','other','new'].map(id=>({id,active:null,tabs:[]}))});
  assert.equal(layout.tree.ratio,.7);assert.equal(layout.tree.axis,'vertical');assert.deepEqual(leafIds(layout.tree),['main','other','new']);
});
