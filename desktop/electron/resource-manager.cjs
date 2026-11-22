const {execFile}=require('node:child_process');
const {join}=require('node:path');
const {cpus}=require('node:os');

// Fixed read-only query: never interpolate renderer input or expose command lines.
const QUERY=`$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $rows=@(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name,CreationDate,KernelModeTime,UserModeTime,WorkingSetSize,PrivatePageCount | ForEach-Object { [pscustomobject]@{pid=[int]$_.ProcessId;parent=[int]$_.ParentProcessId;name=$_.Name;created=if($_.CreationDate){[DateTimeOffset]::new($_.CreationDate.ToUniversalTime()).ToUnixTimeMilliseconds()}else{0};cpuSeconds=([double]$_.KernelModeTime+[double]$_.UserModeTime)/10000000;workingSetBytes=[double]$_.WorkingSetSize;privateBytes=[double]$_.PrivatePageCount} }); ConvertTo-Json -InputObject $rows -Compress`;
function readWindowsProcesses(){
 if(process.platform!=='win32')return Promise.reject(Error('工作区进程统计目前仅支持 Windows'));
 return new Promise((resolve,reject)=>execFile(join(process.env.SystemRoot||'C:\\Windows','System32/WindowsPowerShell/v1.0/powershell.exe'),['-NoLogo','-NoProfile','-NonInteractive','-Command',QUERY],{windowsHide:true,timeout:10000,maxBuffer:4*1024*1024},(error,out)=>{if(error)return reject(Error('无法读取 Windows 进程统计'));try{resolve(JSON.parse(out.replace(/^\uFEFF/,'')))}catch{reject(Error('Windows 进程统计格式无效'))}}));
}
const finite=value=>typeof value==='number'&&Number.isFinite(value)&&value>=0?value:null;
function sumMetrics(items){return Object.fromEntries(['cpu','workingSetBytes','privateBytes'].map(key=>[key,items.length&&items.every(p=>finite(p[key])!==null)?items.reduce((n,p)=>n+p[key],0):null]));}
function sampleProcesses(rows,previous,time,cores){
 const elapsed=previous?(time-previous.time)/1000:0;
 return rows.map(p=>{const old=previous?.rows.get(p.pid),delta=old&&old.created===p.created?p.cpuSeconds-old.cpuSeconds:-1;return {...p,cpu:elapsed>0&&delta>=0?Math.min(100,delta/elapsed/Math.max(1,cores)*100):null}});
}
function assemble(snapshot,sessionLists,processes,desktopPids,time){
 const byPid=new Map(processes.map(p=>[p.pid,p])),children=new Map();
 for(const p of processes){const parent=byPid.get(p.parent);if(parent&&parent.created>0&&p.created>=parent.created&&p.pid!==p.parent){if(!children.has(p.parent))children.set(p.parent,[]);children.get(p.parent).push(p.pid)}}
 const roots=new Set(sessionLists.flatMap(x=>x.sessions||[]).filter(s=>s.status?.exit===null&&s.status.pid>0&&!['failed','host_lost'].includes(s.status.state)).map(s=>s.status.pid));
 const assigned=new Set(desktopPids),listById=new Map(sessionLists.map(x=>[x.id,x]));
 const workspaces=snapshot.workspaces.map(w=>{
  const list=listById.get(w.id),sessions=(list?.sessions||[]).filter(s=>s.status?.exit===null&&s.status.pid>0&&!['failed','host_lost'].includes(s.status.state)).map(s=>{
   const root=byPid.get(s.status.pid),owned=[];
   // status.pid identifies a live Velella host; descendants include the shell/CLI.
   if(root&&/^velella-runtime(?:\.exe)?$/i.test(root.name)&&!assigned.has(root.pid)){
    const visit=pid=>{if(assigned.has(pid))return;assigned.add(pid);owned.push(byPid.get(pid));for(const child of children.get(pid)||[])if(!roots.has(child))visit(child)};visit(root.pid);
   }
   return {id:s.id,title:s.title||s.program||`pid ${s.status.pid}`,kind:s.kind,pid:s.status.pid,processCount:owned.length,...sumMetrics(owned)};
  });
  return {id:w.id,projectId:w.project_id,name:w.name,path:w.path,archived:w.archived,error:list?.error||null,sessions,...sumMetrics(sessions)};
 });
 const desktop=processes.filter(p=>desktopPids.includes(p.pid));
 const projects=snapshot.projects.map(p=>{const children=workspaces.filter(w=>w.projectId===p.id);return {id:p.id,name:p.name,workspaces:children,...sumMetrics(children.filter(w=>w.sessions.length||w.error))}});
 return {sampledAt:time,projects,desktop:{name:'Velella 桌面',processes:desktop.map(({pid,name,cpu,workingSetBytes,privateBytes})=>({pid,name,cpu,workingSetBytes,privateBytes})),...sumMetrics(desktop)},...sumMetrics([...workspaces.filter(w=>w.sessions.length||w.error),...(desktopPids.length?[{...sumMetrics(desktop.length===desktopPids.length?desktop:[])}]:[])])};
}
function createResourceManager(app,runtime,{readProcesses=readWindowsProcesses,now=Date.now,cores=cpus().length}={}){
 let previous,cached,pending;
 return function sample(){
  if(pending)return pending;if(cached&&now()-cached.sampledAt<4500)return Promise.resolve(cached);
  pending=(async()=>{
   const snapshot=await runtime.request({cmd:'workbench.snapshot'});if(!snapshot.ok)throw Error(snapshot.error||'工作区读取失败');
   const lists=[];
   // Bound concurrent runtime requests; do not contend with terminal input using a burst.
   for(const w of snapshot.workspaces){try{const r=await runtime.request({cmd:'workbench.sessions',workspace_id:w.id});if(!r.ok)throw Error(r.error);lists.push({id:w.id,sessions:r.sessions})}catch{lists.push({id:w.id,error:'会话暂时不可用',sessions:[]})}}
   const rows=await readProcesses(),time=now();const processes=sampleProcesses(rows,previous,time,cores);previous={time,rows:new Map(rows.map(p=>[p.pid,p]))};
   cached=assemble(snapshot,lists,processes,app.getAppMetrics().map(p=>p.pid),time);return cached;
  })().finally(()=>{pending=undefined});return pending;
 };
}
module.exports={createResourceManager,readWindowsProcesses,sampleProcesses,assemble,sumMetrics};
