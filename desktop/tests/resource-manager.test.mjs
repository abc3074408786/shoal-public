import {test} from 'node:test';import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,symlink,rm} from 'node:fs/promises';import {tmpdir} from 'node:os';import path from 'node:path';
import manager from '../electron/resource-manager.cjs';import space from '../electron/resource-space.cjs';
const processRow=(pid,parent,extra={})=>({pid,parent,name:'velella-runtime.exe',created:100,cpuSeconds:4,workingSetBytes:10,privateBytes:20,...extra});
test('CPU uses elapsed time, core capacity and process identity; first sample unknown',()=>{
 const rows=[processRow(1,0,{cpuSeconds:6})],prev={time:1000,rows:new Map([[1,processRow(1,0)]])};
 assert.equal(manager.sampleProcesses(rows,null,2000,4)[0].cpu,null);
 assert.equal(manager.sampleProcesses(rows,prev,2000,4)[0].cpu,50);
 assert.equal(manager.sampleProcesses([processRow(1,0,{created:200})],prev,2000,4)[0].cpu,null);
});
test('tree includes host descendants, respects nested roots, prevents reused-parent attribution',()=>{
 const snapshot={projects:[{id:'p',name:'Project'}],workspaces:[{id:'w',project_id:'p',name:'Workspace'}]};
 const sessions=[1,3].map(pid=>({id:String(pid),title:'Shell',status:{pid,exit:null,state:'running'}}));
 const rows=[processRow(1,0),processRow(2,1,{name:'powershell.exe',created:110}),processRow(3,1,{created:120}),processRow(4,3,{created:130}),processRow(5,1,{created:50}),processRow(8,0,{name:'Velella.exe'})].map(p=>({...p,cpu:1}));
 const result=manager.assemble(snapshot,[{id:'w',sessions}],rows,[8],2000);
 assert.deepEqual(result.projects[0].workspaces[0].sessions.map(s=>s.processCount),[2,2]);assert.equal(result.privateBytes,100);assert.equal(result.cpu,5);
 const unavailable=manager.assemble(snapshot,[{id:'w',sessions:[],error:'offline'}],rows,[8],2000);assert.equal(unavailable.privateBytes,null);
 const wrongRoot=manager.assemble(snapshot,[{id:'w',sessions}],rows.map(r=>r.pid===1?{...r,name:'unrelated.exe'}:r),[8],2000);assert.equal(wrongRoot.projects[0].workspaces[0].sessions[0].privateBytes,null);
});
test('manager shares concurrent sample and cache; performs no scan at construction',async()=>{
 let calls=0,time=1000;const sample=manager.createResourceManager({getAppMetrics:()=>[]},{request:async()=>({ok:true,projects:[],workspaces:[]})},{readProcesses:async()=>{calls++;return []},now:()=>time,cores:1});
 assert.equal(calls,0);const [a,b]=await Promise.all([sample(),sample()]);assert.equal(a,b);assert.equal(calls,1);time+=100;await sample();assert.equal(calls,1);time+=5000;await sample();assert.equal(calls,2);
});
test('space scans are bounded, reject changed/active caches and only trash previewed paths',async()=>{
 const root=await mkdtemp(path.join(tmpdir(),'velella-resource-test-')),trashed=[];let active=false;
 try{
  await mkdir(path.join(root,'.vite'));await writeFile(path.join(root,'.vite','cache'),'hello');await writeFile(path.join(root,'source.ts'),'keep');
  const runtime={request:async q=>q.cmd==='workbench.snapshot'?{ok:true,workspaces:[{id:'w',name:'W',path:root}]}:{ok:true,sessions:active?[{status:{pid:42,exit:null,state:'starting'}}]:[]}};
  const api=space.createResourceSpace(runtime,async p=>trashed.push(p));
  let preview=await api.scan({workspace_id:'w'});assert.equal(preview.rows[0].bytes,9);assert.equal(preview.rows[0].caches[0].bytes,5);const args=()=>({token:preview.token,keys:[preview.rows[0].caches[0].key],confirmed:true});
  await assert.rejects(api.clean({...args(),keys:['forged']}),/预览/);
  active=true;assert.match((await api.clean(args())).error,/运行或启动/);assert.equal(trashed.length,0);
  active=false;await writeFile(path.join(root,'.vite','cache'),'changed');assert.match((await api.clean(args())).error,/已变更/);assert.equal(trashed.length,0);
  preview=await api.scan({workspace_id:'w'});const result=await api.clean(args());assert.equal(result.ok,true);assert.deepEqual(trashed,[path.join(root,'.vite')]);await assert.rejects(api.clean(args()),/过期/);
  assert.equal((await space.scanDirectory(root,{maxEntries:1})).partial,true);
 }finally{await rm(root,{recursive:true,force:true})}
});
test('space scan never follows directory junctions or offers them for cleanup',async()=>{
 const root=await mkdtemp(path.join(tmpdir(),'velella-resource-link-')),outside=await mkdtemp(path.join(tmpdir(),'velella-resource-outside-'));
 try{await writeFile(path.join(outside,'private'),'outside');await symlink(outside,path.join(root,'.vite'),'junction');const r=await space.scanDirectory(root);assert.equal(r.bytes,0);assert.equal(r.skipped,1);assert.equal(r.partial,true);await assert.rejects(space.safeCacheRoot(root,'.vite'),/普通目录/)}
 finally{await rm(path.join(root,'.vite'),{force:true});await rm(root,{recursive:true,force:true});await rm(outside,{recursive:true,force:true})}
});
