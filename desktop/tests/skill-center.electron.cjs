// Native host integration, isolated profile and owned daemon; no production replacement.
const {app,BrowserWindow,ipcMain}=require('electron');
const fs=require('node:fs/promises'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict');
const {spawn,execFile}=require('node:child_process');
const {pathToFileURL}=require('node:url');
const {createRuntime}=require('../electron/runtime.cjs'),{createExtensions}=require('../electron/extensions.cjs');
const root=path.join(os.tmpdir(),'velella-center-native-host-'+Date.now()),out=path.resolve(__dirname,'../../artifacts/skill-center-20261109');
app.setPath('userData',path.join(root,'electron-profile'));app.enableSandbox();
let win,daemon,extensions;
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function wait(expression){const end=Date.now()+25000;for(;;){if(await win.webContents.executeJavaScript(expression))return;if(Date.now()>end)throw Error('Native UI condition timed out: '+expression);await delay(100);}}
async function click(text){await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent.trim()===${JSON.stringify(text)}&&!b.disabled)`);await win.webContents.executeJavaScript(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${JSON.stringify(text)}&&!b.disabled).click()`);}
app.whenReady().then(async()=>{
 try{
  await fs.mkdir(root,{recursive:true});await fs.mkdir(out,{recursive:true});
  const exe=path.resolve(__dirname,'../../target/debug/velella-runtime.exe'),cli=path.resolve(__dirname,'../../target/debug/velella-cli.exe');
  daemon=spawn(exe,['daemon',root],{stdio:'ignore',windowsHide:true});const runtime=createRuntime(exe,root,{launch:()=>daemon});
  extensions=createExtensions({profileRoot:root,skillCli:cli,runtime});
  const available=await extensions.request({target:{kind:'local'},action:'list'});assert(available.result.plugins.some(p=>p.id==='skill-center'));
  ipcMain.handle('velella:command',async(_event,command,args)=>{
   if(command==='desktop_extensions')return extensions.request(args);
   if(command==='rpc')return runtime.request(args.request);
   if(command==='window')return args.action==='isMaximized'?false:undefined;
   if(command==='desktop_resource_space')return {ok:true,result:{spaces:[]}};
   return {ok:false,error:'Isolated native test: unsupported '+command};
  });
  win=new BrowserWindow({show:false,width:1440,height:960,webPreferences:{preload:path.resolve(__dirname,'../electron/preload.cjs'),contextIsolation:true,nodeIntegration:false,sandbox:true,backgroundThrottling:false}});
  await win.loadURL(pathToFileURL(path.resolve(__dirname,'../dist/index.html')).href);await click('插件');await click('技能中心');await click('启用技能中心');await click('打开技能中心');
  await wait(`!!document.querySelector('[aria-label="技能中心工作台"]')`);await wait(`document.body.textContent.includes('发现已有技能')`);
  assert.equal((await extensions.request({target:{kind:'local'},action:'list'})).result.plugins.find(p=>p.id==='skill-center').enabled,true);
  await fs.writeFile(path.join(out,'native-plugin-entry.png'),(await win.webContents.capturePage()).toPNG());
  await fs.writeFile(path.join(out,'native-host-result.json'),JSON.stringify({passed:true,entry:'built dist + Electron preload + actual extension host + actual Rust CLI/daemon',productionChanged:false},null,2));console.log('PASS: built desktop plugin registration, enable, open, real CLI and daemon');
 }catch(error){console.error(error);if(win)await fs.writeFile(path.join(out,'native-host-failure.png'),(await win.webContents.capturePage()).toPNG()).catch(()=>{});process.exitCode=1;}
 finally{
  await extensions?.dispose();win?.destroy();
  if(daemon?.pid)await new Promise(r=>execFile('taskkill.exe',['/PID',String(daemon.pid),'/T','/F'],{windowsHide:true},()=>r()));
  await fs.rm(root,{recursive:true,force:true}).catch(()=>{});app.exit(process.exitCode||0);
 }
});
