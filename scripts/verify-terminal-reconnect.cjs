// Isolated real Electron + PowerShell lifecycle verification. No production data.
const { _electron:electron }=require('playwright');
const {spawn,execFileSync}=require('node:child_process');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {socketRequest}=require('../desktop/electron/runtime.cjs');
const root=path.resolve(__dirname,'..');
const evidence=path.join(root,'artifacts','terminal-reconnect-'+Date.now());
const data=path.join(evidence,'data'),project=path.join(evidence,'project');
fs.mkdirSync(data,{recursive:true});fs.mkdirSync(project);
execFileSync('git',['init','--quiet',project],{windowsHide:true});
const runtime=path.join(root,'target/release/velella-runtime.exe');
const endpoint=JSON.parse(execFileSync(runtime,['ipc-endpoint',data,'daemon'],{encoding:'utf8',windowsHide:true}));
const env={...process.env,VELELLA_DATA_DIR:data,VELELLA_RUNTIME_PATH:runtime,VELELLA_DESKTOP_BACKGROUND:'1'};delete env.ELECTRON_RUN_AS_NODE;
const result={evidence,checks:[],started:new Date().toISOString()};
let app,daemon,workspace,sid,page;
const wait=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,timeout=15000){const end=Date.now()+timeout;let error;while(Date.now()<end){try{const value=await fn();if(value)return value}catch(e){error=e}await wait(80)}throw error||Error('verification timed out')}
const rpc=async q=>{const r=await socketRequest(endpoint,q,10000);if(!r.ok)throw Error(r.error);return r};
const row=async()=>(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions.find(s=>s.id===sid);
const liveRow=()=>until(async()=>{const s=await row();return s?.status?.input_protocol===1&&s.status.exit===null?s:null});
const log=()=>fs.readFileSync(path.join(project,'.velella/sessions',sid,'terminal.bin'));
async function desktop(){
 app=await electron.launch({executablePath:path.join(root,'desktop/node_modules/electron/dist/electron.exe'),args:[path.join(root,'desktop')],env,timeout:30000});
 page=await app.firstWindow();await page.waitForLoadState('domcontentloaded');
 await page.evaluate(w=>{localStorage.setItem('velella:workspace',w);localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false}));},workspace);
 await page.reload();await page.locator('.xterm-helper-textarea').waitFor();
 await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.show();w.focus()});
}
async function type(command,marker){await page.locator('.xterm-helper-textarea').focus();await page.keyboard.type(command);await page.keyboard.press('Enter');await until(()=>log().includes(Buffer.from(marker)),10000)}
const startDaemon=()=>spawn(runtime,['daemon',data],{env,windowsHide:true,stdio:'ignore'});
(async()=>{
 try{
  daemon=startDaemon();await until(()=>rpc({cmd:'snapshot'}));
  const p=(await rpc({cmd:'project.add',path:project})).project;workspace='main:'+p.id;
  sid=(await rpc({cmd:'workbench.terminal.open',workspace_id:workspace})).session_id;
  const layout=(await rpc({cmd:'workbench.layout',workspace_id:workspace})).layout;
  layout.panes=[{id:'main',active:'t',tabs:[{id:'t',kind:'terminal',target:sid,title:'PowerShell'}]}];
  await rpc({cmd:'workbench.layout.save',workspace_id:workspace,layout});
  await desktop();const original=(await liveRow()).status.pid;
  // Markers are assembled by PowerShell, so input echo alone cannot pass.
  await type("$reconnectProof='kept'; Write-Output ('BEFORE_'+'CLOSE')",'BEFORE_CLOSE');
  await app.close();app=null;assert.equal((await liveRow()).status.pid,original);
  await desktop();await type("Write-Output ('AFTER_CLOSE_'+$reconnectProof)",'AFTER_CLOSE_kept');
  assert.equal((await liveRow()).status.pid,original);result.checks.push('desktop quit and reopen preserves the same live PowerShell and its variable');
  daemon.kill();await new Promise(r=>daemon.once('exit',r));daemon=startDaemon();await until(()=>rpc({cmd:'snapshot'}));
  assert.equal((await liveRow()).status.pid,original);await type("Write-Output ('AFTER_DAEMON_'+$reconnectProof)",'AFTER_DAEMON_kept');
  result.checks.push('daemon restart reconnects to the same PowerShell without resetting state');
  const previous=log();process.kill(original);
  const recovered=await until(async()=>{const s=await row();return s.status?.pid!==original&&s.status?.input_protocol===1&&s.status?.exit===null?s:null});
  await until(async()=>!(await page.locator('.terminal-status').innerText().catch(()=>'' )).includes('正在恢复'));
  await wait(2300);await type("Write-Output ('AFTER_'+'RECOVERY')",'AFTER_RECOVERY');
  assert.ok(log().subarray(0,previous.length).equals(previous));result.checks.push('host crash automatically restarts one shell, retains exact transcript prefix and accepts keyboard input');
  result.originalPid=original;result.recoveredPid=recovered.status.pid;
  process.kill(recovered.status.pid);await until(async()=>(await row()).status?.state==='host_lost');await wait(3500);
  assert.equal((await row()).auto_recoverable,false);await page.getByRole('button',{name:'重新启动终端',exact:true}).waitFor();
  assert.ok(!(await page.locator('body').innerText()).includes('输入适配不可用'));result.checks.push('second host crash stops automatic attempts and exposes restart directly');
  await page.screenshot({path:path.join(evidence,'recovery-action.png')});
  await page.getByRole('button',{name:'重新启动终端',exact:true}).click();await until(async()=>(await row()).status?.input_protocol===1);
  await wait(2300);await type("Write-Output ('MANUAL_'+'RECOVERY')",'MANUAL_RECOVERY');
  await page.keyboard.type('exit');await page.keyboard.press('Enter');await until(async()=>(await row()).status?.exit===0);await wait(3500);
  assert.equal((await row()).auto_recoverable,false);result.checks.push('manual recovery works, and an explicit exit remains stopped');
  result.ok=true;
 }catch(e){result.ok=false;result.error=e.stack;process.exitCode=1;}
 finally{
  if(app)await app.close().catch(()=>{});
  if(workspace&&sid)await rpc({cmd:'workbench.terminal.stop',workspace_id:workspace,session_id:sid,confirmed:true}).catch(()=>{});
  if(daemon&&!daemon.killed)daemon.kill();
  result.finished=new Date().toISOString();fs.writeFileSync(path.join(evidence,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
 }
})();
