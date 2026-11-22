import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const source=readFileSync(new URL('../src/task-navigation.ts',import.meta.url),'utf8');
const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {resolveTaskNavigation,tasksForProject,taskDisplayState}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const projects=[{id:'project-a'},{id:'project-b'}];
const tasks=[{id:'task-a',project_id:'project-a',created_at:'2026-10-13T08:00:00Z',phase:'running',spec:{name:'相同任务名',agent:'claude-code'},status:null},{id:'task-b',project_id:'project-b',created_at:'2026-10-13T09:00:00Z',phase:'review',spec:{name:'相同任务名',agent:'codex'},native_id:'original-native-session',agent:{id:'original-host'},status:null}];
test('project plus creates in the clicked project and never selects a same-named task',()=>{
  assert.deepEqual(resolveTaskNavigation({requestId:'new',kind:'create',projectId:'project-b'},projects,tasks),{projectId:'project-b',taskId:'',create:true});
  assert.equal(resolveTaskNavigation({requestId:'bad',kind:'open',projectId:'project-b',taskId:'task-a'},projects,tasks),null);
});
test('late snapshot can resolve the original task without mutating its native session',()=>{
  const route={requestId:'open',kind:'open',projectId:'project-b',taskId:'task-b'};
  const before=structuredClone(tasks);
  assert.equal(resolveTaskNavigation(route,projects,[]),null);
  assert.deepEqual(resolveTaskNavigation(route,projects,tasks),{projectId:'project-b',taskId:'task-b',create:false});
  assert.deepEqual(tasks,before);
});
test('list navigation clears a detail selection without creating a task',()=>{
  assert.deepEqual(resolveTaskNavigation({requestId:'list',kind:'list',projectId:'project-a'},projects,tasks),{projectId:'project-a',taskId:'',create:false});
  assert.equal(resolveTaskNavigation({requestId:'missing',kind:'create',projectId:'missing'},projects,tasks),null);
});
test('sidebar search remains project-scoped and does not reorder the shared snapshot',()=>{
  assert.deepEqual(tasksForProject(tasks,'project-b','相同').map(t=>t.id),['task-b']);
  assert.deepEqual(tasksForProject(tasks,'project-a','codex'),[]);
  assert.deepEqual(tasks.map(t=>t.id),['task-a','task-b']);
});
test('permission waiting is not confused with a stopped task',()=>{
  assert.equal(taskDisplayState({...tasks[0],status:{state:'waiting_permission'}}),'waiting_permission');
  assert.equal(taskDisplayState({...tasks[0],phase:'cancelled',status:{state:'waiting_permission'}}),'cancelled');
});
