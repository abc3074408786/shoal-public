import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/workspace-context.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {workspaceRows,workspaceTask,initialAgentSession,workspaceTreeRows,workspaceParentCandidates,workspaceTopPosition}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const task=(id,project_id='p')=>({id,project_id,direct:false,phase:'running',created_at:id,worktree:'/fixture/'+id,spec:{name:id},agent:{id:'agent-'+id}});
const workspace=(id,fields={})=>({id,project_id:'p',name:id,path:'/fixture/'+id,kind:'worktree',archived:false,favorite:false,position:0,...fields});
const tree=(rows,options={})=>workspaceTreeRows(rows,{projectId:'p',projectName:'Project',...options});
test('task workspace is a single row before and after registration',()=>{const t=task('a'),pending=workspaceRows([],[t]);assert.equal(pending.length,1);assert.equal(pending[0].id,'task:a');assert.equal(workspaceRows(pending,[t]).length,1)});
test('project ownership and old native session select the correct task context',()=>{const a=task('a'),b=task('b','other');assert.equal(workspaceTask({id:'task:a',project_id:'p'},[b,a]).id,'a');assert.equal(workspaceTask({id:'task:a',project_id:'p'},[a,b],{task_id:'b'}),undefined)});
test('direct folder tasks reuse one primary workspace and retain historical session context',()=>{const a={...task('a'),direct:true,phase:'review'},b={...task('b'),direct:true};const rows=workspaceRows([],[a,b]);assert.equal(rows.length,1);assert.equal(rows[0].id,'main:p');assert.equal(workspaceTask(rows[0],[a,b]).id,'b');assert.equal(workspaceTask(rows[0],[a,b],{task_id:'a'}).id,'a')});
test('initial Agent tab opens only on a never-used empty layout',()=>{const t=task('a'),s={id:'agent-a',kind:'agent'},layout={panes:[{tabs:[]}],closed:[]};assert.equal(initialAgentSession(layout,[s],t),s);assert.equal(initialAgentSession({...layout,closed:[{id:'closed'}]},[s],t),undefined);assert.equal(initialAgentSession({panes:[{tabs:[{kind:'files'}]}],closed:[]},[s],t),undefined)});

test('queued task rows retain organizational parents without overriding registered rows',()=>{
  const t={...task('child'),phase:'queued',parent_workspace_id:'parent'};
  const parent=workspace('parent'),pending=workspaceRows([parent],[t]);
  assert.equal(pending[1].parent_workspace_id,'parent');
  assert.deepEqual(tree(pending).map(row=>[row.workspace.id,row.depth]),[['parent',0],['task:child',1]]);
  const registered=workspace('task:child',{parent_workspace_id:'reparented'});
  assert.equal(workspaceRows([registered],[t])[0],registered);
  assert.equal(workspaceRows([],[{...t,direct:true}])[0].parent_workspace_id,null);
  assert.equal(workspaceRows([],[task('legacy')])[0].parent_workspace_id,null);
});

test('nested order compares favorites and positions only among siblings',()=>{
  const rows=[
    workspace('child-last',{parent_workspace_id:'root',position:9}),
    workspace('root',{position:0}),
    workspace('grandchild',{parent_workspace_id:'child-favorite',favorite:true,position:-999}),
    workspace('child-favorite',{parent_workspace_id:'root',favorite:true,position:10}),
    workspace('favorite-root',{favorite:true,position:99}),
    workspace('favorite-root-child',{parent_workspace_id:'favorite-root',position:-9999}),
    workspace('child-first',{parent_workspace_id:'root',position:1}),
  ];
  const before=JSON.stringify(rows);
  rows.forEach(Object.freeze);Object.freeze(rows);
  assert.deepEqual(tree(rows).map(row=>[row.workspace.id,row.depth]),[
    ['favorite-root',0],['favorite-root-child',1],['root',0],['child-favorite',1],['grandchild',2],['child-first',1],['child-last',1],
  ]);
  assert.equal(JSON.stringify(rows),before);
});

test('hidden archived ancestors remain clickable context for active descendants',()=>{
  const rows=[workspace('grandparent',{archived:true}),workspace('parent',{archived:true,parent_workspace_id:'grandparent'}),workspace('child',{parent_workspace_id:'parent'}),workspace('unrelated-archive',{archived:true})];
  assert.deepEqual(tree(rows).map(row=>[row.workspace.id,row.depth,row.contextOnly,row.archivedAncestor]),[
    ['grandparent',0,true,true],['parent',1,true,true],['child',2,false,false],
  ]);
  const all=tree(rows,{includeArchived:true});
  assert.equal(all.length,4);
  assert.ok(all.every(row=>!row.archivedAncestor&&!row.contextOnly));
});

test('search retains ancestor context but not unmatched siblings or unrelated archives',()=>{
  const rows=[workspace('root'),workspace('parent',{parent_workspace_id:'root',archived:true}),workspace('child',{parent_workspace_id:'parent',path:'/fixture/Needle'}),workspace('sibling',{parent_workspace_id:'root'}),workspace('needle-archive',{archived:true})];
  assert.deepEqual(tree(rows,{search:' NEEDLE '}).map(row=>[row.workspace.id,row.contextOnly]),[['root',true],['parent',true],['child',false]]);
  assert.deepEqual(tree(rows,{search:'does-not-exist'}),[]);
  assert.equal(tree(rows,{search:'pRoJeCt'}).length,4);
  assert.equal(tree(rows,{search:'needle',includeArchived:true}).length,4);
});

test('cross-project parents are links while children remain in their own project',()=>{
  const parent=workspace('foreign-parent',{project_id:'other',archived:true});
  const rows=[parent,workspace('child',{parent_workspace_id:parent.id}),workspace('grandchild',{parent_workspace_id:'child'})];
  const visible=tree(rows,{search:'grandchild'});
  assert.deepEqual(visible.map(row=>[row.workspace.id,row.depth]),[['child',0],['grandchild',1]]);
  assert.equal(visible[0].parent,parent);
  assert.equal(visible[0].parent.archived,true);
  assert.ok(visible.every(row=>row.workspace.project_id==='p'));
  assert.deepEqual(tree(rows,{projectId:'other'}),[]);
  assert.equal(tree(rows,{projectId:'other',includeArchived:true})[0].workspace,parent);
});

test('malformed cycles, missing parents, and host boundaries never hide rows or mutate IDs',()=>{
  const rows=[workspace('a',{parent_workspace_id:'b'}),workspace('b',{parent_workspace_id:'a'}),workspace('child',{parent_workspace_id:'a'}),workspace('self',{parent_workspace_id:'self'}),workspace('orphan',{parent_workspace_id:'missing'}),workspace('main:p',{parent_workspace_id:'a'}),workspace('primary',{kind:'primary',parent_workspace_id:'a'}),workspace('remote',{host_id:'remote'}),workspace('local-child',{parent_workspace_id:'remote'})];
  const before=JSON.stringify(rows),visible=tree(rows);
  assert.equal(visible.length,rows.length);
  assert.equal(new Set(visible.map(row=>row.workspace.id)).size,rows.length);
  for(const id of ['self','orphan','main:p','primary','local-child'])assert.equal(visible.find(row=>row.workspace.id===id).depth,0);
  for(const row of visible){
    if(row.parent){const parentIndex=visible.findIndex(candidate=>candidate.workspace.id===row.parent.id);assert.ok(parentIndex<visible.indexOf(row));assert.equal(row.depth,visible[parentIndex].depth+1)}
  }
  assert.equal(JSON.stringify(rows),before);
});

test('cross-project cycles are bounded without moving workspace ownership',()=>{
  const rows=[workspace('a',{parent_workspace_id:'b'}),workspace('b',{project_id:'other',parent_workspace_id:'a'})];
  assert.deepEqual(tree(rows).map(row=>[row.workspace.id,row.depth]),[['a',0]]);
  assert.deepEqual(tree(rows,{projectId:'other'}).map(row=>[row.workspace.id,row.depth]),[['b',0]]);
  assert.deepEqual(workspaceParentCandidates(rows[0],rows),[]);
});

test('reparent choices exclude self, descendants across projects, other hosts, and invalid chains',()=>{
  const current=workspace('current',{parent_workspace_id:'main:p'});
  const rows=[workspace('main:p',{kind:'primary'}),current,workspace('child',{parent_workspace_id:'current'}),workspace('grandchild',{parent_workspace_id:'child',project_id:'other'}),workspace('sibling',{parent_workspace_id:'main:p'}),workspace('other-project',{project_id:'other'}),workspace('remote',{host_id:'remote'}),workspace('cycle-a',{parent_workspace_id:'cycle-b'}),workspace('cycle-b',{parent_workspace_id:'cycle-a'}),workspace('orphan',{parent_workspace_id:'missing'}),workspace('archived',{archived:true}),workspace('explicit-local',{host_id:'local'})];
  assert.deepEqual(workspaceParentCandidates(current,rows).map(row=>row.id),['main:p','sibling','other-project','archived','explicit-local']);
  assert.deepEqual(workspaceParentCandidates(rows[0],rows),[]);
  assert.deepEqual(workspaceParentCandidates(workspace('primary',{kind:'primary'}),rows),[]);
  assert.deepEqual(workspaceParentCandidates(workspace('folder',{kind:'folder'}),rows),[]);
});

test('top position uses the sibling minimum, including hidden archived siblings only',()=>{
  const current=workspace('current',{parent_workspace_id:'parent',position:5});
  const rows=[workspace('parent',{position:-10000}),current,workspace('sibling',{parent_workspace_id:'parent',position:1}),workspace('archived-sibling',{parent_workspace_id:'parent',position:-2,archived:true}),workspace('child',{parent_workspace_id:'current',position:-100}),workspace('unrelated',{position:-200}),workspace('foreign-project',{project_id:'other',parent_workspace_id:'parent',position:-500}),workspace('foreign-host',{host_id:'remote',parent_workspace_id:'parent',position:-999})];
  assert.equal(workspaceTopPosition(current,rows),-3);
  assert.equal(workspaceTopPosition(workspace('only',{position:3}),[]),2);
  assert.equal(workspaceTopPosition(rows[0],rows),-10001);
});

test('deep hierarchies retain every level without recursive traversal limits',()=>{
  const rows=Array.from({length:12000},(_,index)=>workspace('node-'+index,{parent_workspace_id:index?'node-'+(index-1):null})).reverse();
  const visible=tree(rows,{search:'node-11999'});
  assert.equal(visible.length,12000);
  assert.equal(visible[0].workspace.id,'node-0');
  assert.equal(visible.at(-1).workspace.id,'node-11999');
  assert.equal(visible.at(-1).depth,11999);
});
