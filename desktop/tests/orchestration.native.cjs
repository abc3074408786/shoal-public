// Real renderer + runtime + installed native CLIs. Owns only this isolated profile.
const {app,BrowserWindow,ipcMain}=require('electron');
const fs=require('node:fs');const path=require('node:path');const {spawn,execFileSync}=require('node:child_process');const {createRuntime}=require('../electron/runtime.cjs');const {trustedSender}=require('../electron/security.cjs');const {pathToFileURL}=require('node:url');
const output=path.resolve(__dirname,'../../artifacts/agent-orchestration/native-'+(process.env.VELELLA_NATIVE_ATTEMPT||'2'));const data=path.join(output,'data');const project=process.env.VELELLA_NATIVE_PROJECT||path.join(output,'project');fs.mkdirSync(project,{recursive:true});fs.mkdirSync(data,{recursive:true});
app.setPath('userData',path.join(data,'electron-profile'));app.enableSandbox();
let win,runtime,daemon;const checks=[];const record=(name,value)=>{checks.push({name,value});fs.writeFileSync(path.join(output,'checks.json'),JSON.stringify(checks,null,2));};
const wait=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label,timeout=15000){const end=Date.now()+timeout;while(Date.now()<end){const r=await fn();if(r)return r;await wait(150)}throw Error('Timed out: '+label)}
const js=source=>win.webContents.executeJavaScript(source);
async function click(label){await js(`(()=>{const e=[...document.querySelectorAll('button')].find(e=>e.textContent.trim()===${JSON.stringify(label)}&&!e.disabled);if(!e)throw Error('Missing '+${JSON.stringify(label)});e.click()})()`)}
async function fill(selector,value){await js(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('Missing field');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}))})()`)}
async function capture(name){fs.writeFileSync(path.join(output,name+'.png'),(await win.webContents.capturePage()).toPNG());}
app.whenReady().then(async()=>{
  if(!fs.existsSync(path.join(project,'math_ops.py'))){fs.writeFileSync(path.join(project,'math_ops.py'),'def add(a, b):\n    return a - b\n');fs.writeFileSync(path.join(project,'test_math_ops.py'),'import unittest\nfrom math_ops import add\nclass TestAdd(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n        self.assertEqual(add(-2, 3), 1)\n');}
  if(!fs.existsSync(path.join(project,'.git')))execFileSync('git',['init'],{cwd:project,windowsHide:true});
  record('fixture_git_root',project);
  const executable=process.env.VELELLA_NATIVE_RUNTIME||path.resolve(__dirname,'../../target/release/velella-runtime.exe');
  runtime=createRuntime(executable,data,{launch(){daemon=spawn(executable,['daemon',data],{windowsHide:true,stdio:'ignore'});fs.writeFileSync(path.join(output,'ownership.json'),JSON.stringify({electron:process.pid,daemon:daemon.pid,data,project,executable},null,2));return daemon;}});
  const existing=await runtime.request({cmd:'workbench.snapshot'});let wid=existing.workspaces?.find(w=>w.path.toLowerCase()===project.toLowerCase())?.id;
  if(!wid){const added=await runtime.request({cmd:'project.add',path:project});if(!added.ok)throw Error(added.error);const snapshot=await runtime.request({cmd:'workbench.snapshot'});wid=snapshot.workspaces.find(w=>w.project_id===added.project.id).id;}
  const verified=await runtime.request({cmd:'workbench.snapshot'});if(verified.workspaces.find(w=>w.id===wid)?.path.toLowerCase()!==project.toLowerCase())throw Error('Fixture workspace escaped its isolated root');record('isolated_workspace',wid);
  const entry=pathToFileURL(path.resolve(__dirname,'../dist/index.html')).href;
  win=new BrowserWindow({width:1440,height:960,show:true,title:'Agent 编排 · 隔离验证',webPreferences:{preload:path.resolve(__dirname,'../electron/preload.cjs'),contextIsolation:true,nodeIntegration:false,sandbox:true}});
  ipcMain.handle('velella:command',async(event,command,args={})=>{if(!trustedSender(event,win.webContents,entry))throw Error('untrusted');if(command==='rpc')return runtime.request(args.request);if(command==='window'||command==='browser_layout')return {};if(command==='notify')return {};if(command==='dialog_confirm')return false;throw Error('unsupported native test command '+command)});
  win.on('page-title-updated',e=>e.preventDefault());
  win.webContents.on('console-message',event=>{if(event.level==='error')record('renderer_error',event.message)});
  win.webContents.on('did-fail-load',(_e,code,description)=>record('load_failed',{code,description}));win.webContents.on('render-process-gone',(_e,details)=>record('renderer_gone',details));
  win.loadURL(entry).then(()=>record('loaded',true)).catch(e=>record('load_error',String(e)));
  await until(()=>js(`document.readyState!=='loading'`).catch(()=>false),'page load');record('renderer_ready',true);await until(()=>js(`!![...document.querySelectorAll('.ide-nav')].find(e=>e.textContent==='Agent 编排')`),'console entry');await click('Agent 编排');await until(()=>js(`!!document.querySelector('.orch-page')`),'console mounted');record('console_mounted',true);let id;const saved=path.join(output,'run.json');if(fs.existsSync(saved)){id=JSON.parse(fs.readFileSync(saved)).id;record('resumed_existing_run',id);}else{await click('新建协作');await until(()=>js(`!!document.querySelector('.orch-create textarea')`),'new run');
  await fill('.orch-create textarea','修复当前测试项目 math_ops.py 的 add 函数，使 test_math_ops.py 通过。计划仅包含一个只读探子定位问题、一个主 Agent 实现任务，最多并发1；不修改测试，不安装依赖，不提交发布。');
  await fill('.orch-create label:nth-child(3) textarea','python -m unittest test_math_ops.py 通过；改动仅 math_ops.py；独立审查确认。');
  await js(`(()=>{const e=document.querySelector('.orch-create .orch-form-row label:nth-child(2) select');Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set.call(e,'claude-code');e.dispatchEvent(new Event('change',{bubbles:true}))})()`);
  await click('生成协作方案');let runs=await until(async()=>{const r=await runtime.request({cmd:'orchestration.list'});return r.runs?.length?r.runs:null},'created run');id=runs[0].id;fs.writeFileSync(path.join(output,'run.json'),JSON.stringify({id,wid},null,2));record('created_through_real_ui',true);}
  let opened=false,last='';
  for(let i=0;i<600;i++){
    const response=await runtime.request({cmd:'orchestration.snapshot',id});const run=response.run;fs.writeFileSync(path.join(output,'snapshot.json'),JSON.stringify(run,null,2));
    const state=run.state+' / '+(run.blocker||'');if(state!==last){console.log(state);last=state;}
    if(run.blocker&&run.lead_session&&!opened){await until(()=>js(`!![...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='主 Agent')`),'lead button');await click('主 Agent');await until(()=>js(`!!document.querySelector('.wb-terminal')`),'lead terminal');await wait(800);await capture('native-lead');win.show();opened=true;console.log('Native prerequisite requires user attention in isolated candidate window.');}
    // The operator may drive confirmation in the visible UI. The test does not approve native trust.
    if(run.state==='accepted'||run.state==='stopped'){await capture('native-final');record('terminal_state',run.state);console.log('Native run terminal; close candidate window to end test.');break;}
    await wait(1000);
  }
  record('observation_finished',true);console.log('Observation finished. Native UAT remains separate from fixture checks.');
}).catch(async error=>{console.error(error);record('error',String(error));if(win&&!win.isDestroyed()){try{record('failure_body',await js('document.body.innerText'));await capture('native-failure')}catch{}}app.quit()});
app.on('window-all-closed',()=>app.quit());
