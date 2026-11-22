// Replay a captured native planning snapshot without launching models or a daemon.
const {app, BrowserWindow, ipcMain}=require('electron');
const fs=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const {pathToFileURL}=require('node:url');
const output=path.resolve(__dirname,'../../artifacts/agent-orchestration/native-replay');
const captured=JSON.parse(fs.readFileSync(process.env.VELELLA_REPLAY_SNAPSHOT||path.resolve(__dirname,'../../artifacts/agent-orchestration/native-4/snapshot.json'),'utf8'));
const initial=process.env.VELELLA_REPLAY_INITIAL==='1';
if(initial&&!process.env.VELELLA_REPLAY_SNAPSHOT){delete captured.collab_id;delete captured.lead_session;captured.status={members:[],work:[],reviews:[]};}
const workspace={id:captured.workspace_id,project_id:'replay-project',name:'原生快照回放',path:'D:\\isolated-replay',kind:'folder',position:0,archived:false,favorite:false};
const errors=[],requests=[];
let win;
fs.mkdirSync(output,{recursive:true});
app.setPath('userData',path.join(output,'profile'));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const evaluate=source=>win.webContents.executeJavaScript(source);
async function until(fn,label){const deadline=Date.now()+8000;while(Date.now()<deadline){if(errors.length)throw Error(errors.join('\n'));if(await fn())return;await pause(100)}throw Error('Timed out: '+label)}
app.whenReady().then(async()=>{
  ipcMain.handle('velella:command',async(_event,command,args={})=>{
    if(command!=='rpc')return {};
    const r=args.request;requests.push(r.cmd);
    switch(r.cmd){
      case 'workbench.snapshot':return {ok:true,projects:[{id:workspace.project_id,name:'回放项目',path:workspace.path,setup:''}],workspaces:[workspace],tasks:[],source_links:[]};
      case 'workbench.layout':return {ok:true,layout:{revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]}};
      case 'workbench.files':return {ok:true,files:[]};
      case 'workbench.sessions':return {ok:true,sessions:[]};
      case 'workbench.git':return {ok:true,branch:'main',status:'',diff:'',staged:''};
      case 'orchestration.list':return {ok:true,runs:[captured]};
      case 'orchestration.snapshot':return {ok:true,run:captured};
      default:throw Error('Replay disallows '+r.cmd);
    }
  });
  win=new BrowserWindow({show:false,webPreferences:{preload:path.resolve(__dirname,'../electron/preload.cjs'),contextIsolation:true,nodeIntegration:false,sandbox:true}});
  win.webContents.on('console-message',event=>{if(event.level==='error')errors.push(event.message)});
  await win.loadURL(pathToFileURL(path.resolve(__dirname,'../dist/index.html')).href);
  await evaluate(`localStorage.setItem('velella:orchestration:selected',${JSON.stringify(captured.id)})`);
  await until(()=>evaluate(`!![...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Agent 编排')`),'entry');
  await evaluate(`([...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Agent 编排')).click()`);
  await until(()=>evaluate(`!!document.querySelector('.orch-run-heading')`),'native planning detail');
  assert.equal(await evaluate(`!![...document.querySelectorAll('.orch-actions button')].find(e=>e.textContent.trim()==='主 Agent')`),!initial);
  fs.writeFileSync(path.join(output,initial?'initial-result.json':'result.json'),JSON.stringify({status:'passed',errors,requests},null,2));
  console.log('PASS: native '+(initial?'initial':'lead-ready')+' planning snapshot renders');
  app.quit();
}).catch(async error=>{
  let body='';try{body=await evaluate('document.body.innerText')}catch{}
  fs.writeFileSync(path.join(output,initial?'initial-result.json':'result.json'),JSON.stringify({status:'failed',error:String(error),errors,requests,body},null,2));
  console.error(error);app.exit(1);
});
