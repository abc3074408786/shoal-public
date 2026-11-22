const {app,BrowserWindow,ipcMain}=require('electron');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {createDesktopResources}=require('../electron/desktop-resources.cjs');
const {COMMANDS,trustedSender}=require('../electron/security.cjs');
const out=process.env.RESOURCE_STATUS_OUTPUT||path.resolve(__dirname,'../../artifacts/resource-status-20261028');fs.mkdirSync(out,{recursive:true});app.setPath('userData',path.join(out,'native-profile'));
app.whenReady().then(async()=>{
 try{
  const win=new BrowserWindow({show:false,webPreferences:{preload:path.resolve(__dirname,'../electron/preload.cjs'),sandbox:true,contextIsolation:true,nodeIntegration:false}});
  const file=path.join(out,'native.html');fs.writeFileSync(file,'<!doctype html><title>Resource IPC fixture</title>');
  const entry=require('node:url').pathToFileURL(file).href,sample=createDesktopResources(app);
  ipcMain.handle('velella:command',(event,command)=>{assert.ok(COMMANDS.has(command)&&trustedSender(event,win.webContents,entry));assert.equal(command,'desktop_resources');return sample()});
  await win.loadFile(file);const result=await win.webContents.executeJavaScript("window.velellaDesktop.invoke('desktop_resources')");
  assert.ok(result.processes.some(p=>p.pid===process.pid));assert.ok(result.privateBytes>0);assert.equal(result.privateBytes,result.processes.reduce((n,p)=>n+p.privateBytes,0));
  fs.writeFileSync(path.join(out,'native-metrics.json'),JSON.stringify({status:'passed',...result},null,2));console.log(JSON.stringify({status:'passed',processes:result.processes.length,privateBytes:result.privateBytes}));app.exit(0);
 }catch(error){fs.writeFileSync(path.join(out,'native-error.txt'),String(error));console.error(error);app.exit(1)}
});
