import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
async function load(name){const compiled=ts.transpileModule(readFileSync(new URL('../src/'+name+'.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'))}
const {viewWorkspaces,compareWorkspaceView,sortViewProjects,workspaceViewGroups,linkedWorkspacePR}=await load('workspace-view');
const {workspaceTreeRows}=await load('workspace-context');const {defaultWorkspaceView}=await load('project-library');
const row=(id,extra={})=>({id,project_id:'p',name:id,path:'/p/'+id,kind:'worktree',favorite:false,archived:false,position:0,...extra});
const projects=[{id:'p',name:'P10'},{id:'q',name:'P2'}],library={version:1,groups:[],projects:{},workspaces:{sleep:{sleeping:true},done:{status:'done'}}};
test('hide sleeping and archived ancestors without losing visible children or mixing projects',()=>{
  const rows=[row('sleep'),row('child',{parent_workspace_id:'sleep'}),row('archive',{archived:true}),row('live',{parent_workspace_id:'archive'}),row('other',{project_id:'q'}),row('foreign',{project_id:'missing'})];
  const filtered=viewWorkspaces(rows,projects,library,{...defaultWorkspaceView(),project:'p',hideSleeping:true});
  assert.deepEqual(filtered.map(w=>w.id),['child','live']);
  const tree=workspaceTreeRows(filtered,{projectId:'p',search:'child'});assert.deepEqual(tree.map(r=>[r.workspace.id,r.depth]),[['child',0]]);
  assert.equal(rows[0].parent_workspace_id,undefined);
});
test('name sorting is natural, immutable, and keeps parent before children in the tree',()=>{
  const rows=[row('z-parent'),row('b10'),row('b2'),row('a-child',{parent_workspace_id:'z-parent'}),row('pinned',{favorite:true,position:50})];
  assert.deepEqual(workspaceTreeRows(rows,{projectId:'p',compare:compareWorkspaceView('name')}).map(r=>r.workspace.id),['b2','b10','pinned','z-parent','a-child']);
  assert.equal([...rows].sort(compareWorkspaceView('manual'))[0].id,'pinned');assert.equal([...rows].sort(compareWorkspaceView('name_desc'))[0].id,'z-parent');assert.equal(rows[0].id,'z-parent');
  assert.deepEqual(sortViewProjects(projects,'name').map(p=>p.id),['q','p']);assert.equal(projects[0].id,'p');
});
test('status and PR groups preserve each row exactly once and unknown PRs stay unknown',()=>{
  const rows=[row('todo'),row('done'),row('task:with-pr'),row('task:no-url')];
  const tasks=[{id:'with-pr',project_id:'p',created_at:'2026-11-04',pr_url:'https://github.com/example/repo/pull/1'}, {id:'no-url',project_id:'p',created_at:'2026-11-04',pr_url:null,delivery:{pull_request:false}}, {id:'todo',project_id:'other',direct:true,created_at:'2026-11-04',pr_url:'https://github.com/other/repo/pull/2'}];
  const groups=workspaceViewGroups(rows,{...defaultWorkspaceView(),groupBy:'status'},library,tasks);assert.deepEqual(groups.map(g=>[g.id,g.rows.length]),[['todo',3],['done',1]]);
  const pr=workspaceViewGroups(rows,{...defaultWorkspaceView(),groupBy:'pr'},library,tasks);assert.deepEqual(pr.map(g=>[g.id,g.rows.length]),[['linked',1],['unknown',3]]);
  assert.equal(linkedWorkspacePR(row('main:p'),tasks),undefined);assert.equal(new Set(pr.flatMap(g=>g.rows.map(w=>w.id))).size,rows.length);
  assert.equal(viewWorkspaces(rows,projects,library,{...defaultWorkspaceView(),project:'deleted-project'}).length,0);
});
