const {app,BrowserWindow,ipcMain,shell}=require('electron');
const fs=require('node:fs'),fsp=require('node:fs/promises'),path=require('node:path'),assert=require('node:assert/strict');
const {createResourceManager}=require('../electron/resource-manager.cjs');
const {createResourceSpace}=require('../electron/resource-space.cjs');
const {createRuntime}=require('../electron/runtime.cjs');
const {COMMANDS,trustedSender}=require('../electron/security.cjs');
const out=path.resolve(__dirname,'../../artifacts/resource-manager-20261028');fs.mkdirSync(out,{recursive:true});app.setPath('userData',path.join(out,'native-profile'));
app.whenReady().then(async()=>{
 try{
  const plan=JSON.parse(fs.readFileSync(path.resolve(__dirname,'../../artifacts/resource-status-20261028/plan.json')));
  const runtime=createRuntime(plan.runtime,path.join(process.env.LOCALAPPDATA,'Velella'));
  const win=new BrowserWindow({show:false,webPreferences:{preload:path.resolve(__dirname,'../electron/preload.cjs'),sandbox:true,contextIsolation:true,nodeIntegration:false}});
  const file=path.join(out,'native.html');fs.writeFileSync(file,'<!doctype html><title>Resource manager IPC fixture</title>');
  const entry=require('node:url').pathToFileURL(file).href,sample=createResourceManager(app,runtime);
  const root=await fsp.mkdtemp(path.join(out,'cleanup-fixture-'));assert.ok(root.startsWith(out+path.sep));await fsp.mkdir(path.join(root,'.vite'));await fsp.writeFile(path.join(root,'.vite','cache'),'test cache');await fsp.writeFile(path.join(root,'source.ts'),'preserved source');
  const space=createResourceSpace({request:async q=>q.cmd==='workbench.snapshot'?{ok:true,workspaces:[{id:'fixture',name:'Fixture',path:root}]}:{ok:true,sessions:[]}},target=>{assert.equal(target,path.join(root,'.vite'));return shell.trashItem(target)});
  ipcMain.handle('velella:command',(event,command,args)=>{assert.ok(COMMANDS.has(command)&&trustedSender(event,win.webContents,entry));if(command==='resource_manager')return sample();if(command==='resource_space_scan')return space.scan(args);if(command==='resource_space_clean')return space.clean(args);throw Error('Unexpected command')});
  await win.loadFile(file);const first=await win.webContents.executeJavaScript("window.velellaDesktop.invoke('resource_manager')");assert.ok(first.desktop.processes.some(p=>p.pid===process.pid));assert.ok(first.desktop.privateBytes>0);assert.equal(first.cpu,null);
  await new Promise(resolve=>setTimeout(resolve,4700));const second=await win.webContents.executeJavaScript("window.velellaDesktop.invoke('resource_manager')");assert.ok(second.desktop.cpu>=0);const sessions=second.projects.flatMap(p=>p.workspaces.flatMap(w=>w.sessions));assert.ok(sessions.some(s=>s.processCount>1),'must observe real host descendants');
  const preview=await win.webContents.executeJavaScript("window.velellaDesktop.invoke('resource_space_scan')");const cache=preview.rows[0].caches.find(c=>c.relative==='.vite');assert.equal(cache.bytes,10);
  const result=await win.webContents.executeJavaScript(`window.velellaDesktop.invoke('resource_space_clean',${JSON.stringify({token:preview.token,keys:[cache.key],confirmed:true})})`);assert.equal(result.ok,true);assert.equal(fs.existsSync(path.join(root,'.vite')),false);assert.equal(fs.readFileSync(path.join(root,'source.ts'),'utf8'),'preserved source');
  fs.writeFileSync(path.join(out,'native-result.json'),JSON.stringify({status:'passed',first,second,recycleTest:{status:'passed',fixtureRoot:root,onlyFixtureCacheTrashed:true,sourcePreserved:true}},null,2));app.exit(0);
 }catch(error){fs.writeFileSync(path.join(out,'native-error.txt'),String(error.stack));app.exit(1)}
});
