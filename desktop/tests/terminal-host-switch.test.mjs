import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const text=readFileSync(new URL('../src/Workbench.tsx',import.meta.url),'utf8');
const tree=ts.createSourceFile('Workbench.tsx',text,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
let terminal;
function find(node){if(ts.isFunctionDeclaration(node)&&node.name?.text==='terminal')terminal=node;ts.forEachChild(node,find)}find(tree);
assert.ok(terminal);
const js=ts.transpileModule(`module.exports=(${terminal.getText(tree)});`,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
function fixture(switchAfter){
 const a={kind:'local'},b={kind:'ssh',host_id:'host-b',server_id:'server-b'};let active=a,layouts=0;const calls=[];
 const context={module:{exports:{}},busy:false,task:undefined,w:{id:'workspace-a'},layout:{panes:[{id:'main',tabs:[],active:null}]},activePane:{id:'main'},sessions:[],crypto:{randomUUID:()=> 'tab-one'},structuredClone,
  currentTarget:()=>active,sameTarget:(x,y)=>x===y,setBusy:()=>{},setError:()=>{},setPaneId:()=>{},setLayout:()=>{layouts++},setSessions:()=>{},terminalSessionTitle:()=> 'Shell',
  rpc:async(request,target=active)=>{calls.push({cmd:request.cmd,target});if(request.cmd===switchAfter)active=b;return {session_id:'session-a',sessions:[{id:'session-a',kind:'terminal',program:'sh'}],layout:request.layout}},
 };
 context.refreshSessions=()=>context.rpc({cmd:'workbench.sessions'});
 vm.runInNewContext(js,context);return {run:context.module.exports,calls,a,layouts:()=>layouts};
}
for(const switchAfter of ['workbench.terminal.open','workbench.sessions','workbench.layout.save'])test(`blank terminal retains host A when switching after ${switchAfter}`,async()=>{
 const f=fixture(switchAfter);await f.run();assert.ok(f.calls.every(call=>call.target===f.a),'no request may be rebound to host B');assert.equal(f.layouts(),0,'do not apply host A layout after switching away');
});
test('blank terminal on the same host opens and saves its layout',async()=>{
 const f=fixture();await f.run();assert.ok(f.calls.every(call=>call.target===f.a));assert.equal(f.layouts(),1);
});
