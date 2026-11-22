import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/shell-navigation.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {recordLocation,moveLocation}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const home={page:'home'},project={page:'project'};
const initial=()=>({entries:[home],index:0});
test('project and workspace changes stay on the same page history entry',()=>{
  const h=recordLocation(initial(),{...project,workspaceId:'a'});
  assert.equal(recordLocation(h,{...project,workspaceId:'b'}),h);
  assert.equal(moveLocation(h,-1).entries[0].page,'home');
});
test('selecting a project after back preserves the forward page',()=>{
  let h=recordLocation(initial(),project);h=recordLocation(h,{page:'settings'});
  const back=moveLocation(h,-1);
  assert.equal(recordLocation(back,{...project,workspaceId:'new-selection'}),back);
  assert.equal(moveLocation(back,1).entries[2].page,'settings');
});
test('opening a different page after back discards the old forward path',()=>{
  let h=recordLocation(initial(),project);h=recordLocation(h,{page:'settings'});
  h=recordLocation(moveLocation(h,-1),{page:'automation'});
  assert.deepEqual(h.entries,[home,project,{page:'automation'}]);assert.equal(moveLocation(h,1),h);
});
test('snapshot changes on home do not create duplicate navigation entries',()=>{const h=initial();assert.equal(recordLocation(h,{...home,workspaceId:'a'}),h);assert.equal(moveLocation(h,-1),h)});
test('all major pages participate in back and forward',()=>{
  let h=initial();const pages=['project','orchestration','automation','sources','plugins','settings'];
  for(const page of pages)h=recordLocation(h,{page});
  for(const page of [...pages.slice(0,-1)].reverse().concat('home')){h=moveLocation(h,-1);assert.equal(h.entries[h.index].page,page)}
  for(const page of pages){h=moveLocation(h,1);assert.equal(h.entries[h.index].page,page)}
});
test('history remains bounded and clamps at both ends',()=>{
  let h=initial();for(let i=0;i<70;i++)h=recordLocation(h,{page:i%2?'settings':'project'});
  assert.equal(h.entries.length,50);assert.equal(h.index,49);assert.equal(moveLocation(h,1),h);
  h=moveLocation(h,-100);assert.equal(h.index,0);assert.equal(moveLocation(h,-1),h);
});
