// Same existing workbench suite; current chat entry and isolated evidence path.
// Explicit browser regression: node tests/terminal-execution.browser.mjs
// Uses a disposable Chrome profile and Vite without the real RPC bridge.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp,readFile,writeFile,mkdir,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';

const root=fileURLToPath(new URL('..',import.meta.url));
const output=process.env.VELELLA_UI_EVIDENCE_DIR||fileURLToPath(new URL('../../artifacts/agent-orchestration/ui/',import.meta.url));
const profile=await mkdtemp(join(tmpdir(),'velella-interface-'));
const checks={},snapshots={},exceptions=[],network=[];
let chrome,socket,server,escapedRpc=0,error;
let sequence=0,documentLoads=0;
const pending=new Map();
async function until(fn,label,timeout=10000){
  const deadline=Date.now()+timeout;
  while(Date.now()<deadline){const value=await fn();if(value)return value;await delay(40)}
  throw new Error(`Timed out: ${label}`);
}
function cdp(method,params={}){
  return new Promise((resolve,reject)=>{
    const id=++sequence;
    const timer=setTimeout(()=>{pending.delete(id);reject(new Error(`CDP timeout: ${method}`))},10000);
    pending.set(id,{resolve:value=>{clearTimeout(timer);resolve(value)},reject:value=>{clearTimeout(timer);reject(value)}});
    socket.send(JSON.stringify({id,method,params}));
  });
}
async function evaluate(expression){
  const result=await cdp('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  if(result.exceptionDetails)throw new Error(result.exceptionDetails.text+': '+result.exceptionDetails.exception?.description);
  return result.result.value;
}
try{
  await mkdir(output,{recursive:true});
  server=await createServer({
    root,configFile:false,logLevel:'error',plugins:[react(),{
      name:'deny-real-rpc',
      configureServer(s){s.middlewares.use('/_velella',(_req,res)=>{
        escapedRpc++;res.statusCode=500;res.end('No real RPC bridge exists in this test');
      })},
    }],server:{host:'127.0.0.1',port:0},
  });
  await server.listen();
  chrome=spawn(process.env.CHROME_BIN||'C:/Program Files/Google/Chrome/Application/chrome.exe',[
    '--headless=new','--no-sandbox','--disable-gpu','--no-first-run','--disable-background-networking',
    '--disable-component-update','--disable-sync','--disable-default-apps','--metrics-recording-only',
    '--disable-features=MediaRouter','--remote-debugging-address=127.0.0.1','--remote-debugging-port=0',
    `--user-data-dir=${profile}`,'about:blank',
  ],{stdio:['ignore','ignore','pipe']});
  chrome.stderr.on('data',()=>{});
  chrome.on('error',failure=>{error=failure});
  const port=await until(async()=>{
    if(error)throw error;
    try{return (await readFile(join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0]}catch{return null}
  },'Chrome debug port');
  const targets=await fetch(`http://127.0.0.1:${port}/json/list`).then(r=>r.json());
  socket=new WebSocket(targets.find(t=>t.type==='page').webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true})});
  socket.addEventListener('message',event=>{
    const message=JSON.parse(event.data);
    if(message.id){const item=pending.get(message.id);if(!item)return;pending.delete(message.id);message.error?item.reject(new Error(JSON.stringify(message.error))):item.resolve(message.result);return}
    if(message.method==='Page.loadEventFired')documentLoads++;
    if(message.method==='Runtime.exceptionThrown')exceptions.push(message.params.exceptionDetails);
    if(message.method==='Network.requestWillBeSent')network.push(message.params.request.url);
  });
  await cdp('Runtime.enable');
  await cdp('Network.enable');
  await cdp('Page.enable');
  await cdp('Emulation.setDeviceMetricsOverride',{width:1280,height:940,deviceScaleFactor:1,mobile:false});


  const base=server.resolvedUrls.local[0];
  async function shot(name){const image=await cdp('Page.captureScreenshot',{format:'png'});await writeFile(join(output,name+'.png'),Buffer.from(image.data,'base64'))}
  async function click(selector){const point=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e||e.disabled)throw Error('Missing enabled '+${JSON.stringify(selector)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await cdp('Input.dispatchMouseEvent',{type,...point,button:'left',clickCount:1});}
  async function key(key,code=key){const keyCode={Escape:27,ArrowDown:40,ArrowUp:38,Enter:13,Tab:9}[key];await cdp('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode:keyCode,nativeVirtualKeyCode:keyCode});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:keyCode,nativeVirtualKeyCode:keyCode});}
  async function loaded(){await until(()=>evaluate('!!document.querySelector(".wb-content") && !document.querySelector("button[aria-label=新建标签]")?.disabled'),'workbench ready')}

  async function textClick(text,scope='document'){
    const point=await evaluate(`(()=>{const e=[...${scope}.querySelectorAll('button')].find(e=>e.textContent.trim()===${JSON.stringify(text)}&&!e.disabled);if(!e)throw Error('Missing button '+${JSON.stringify(text)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await cdp('Input.dispatchMouseEvent',{type,...point,button:'left',clickCount:1});
  }
  async function fill(selector,value){await click(selector);await evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});await cdp('Input.insertText',{text:value});}
  await cdp('Page.navigate',{url:base+'tests/orchestration.html'});
  await until(()=>evaluate(`!![...document.querySelectorAll('.ide-nav')].find(e=>e.textContent==='Agent 编排')`),'navigation');
  await textClick('Agent 编排');await until(()=>evaluate(`!!document.querySelector('.orch-page')`),'console');
  checks.entry=true;checks.installed_a_shell=await evaluate(`document.querySelector('.ide-shell').classList.contains('workbench-a-shell')&&!!document.querySelector('.workbench-wordmark')`);await until(()=>evaluate(`!![...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='新建协作'&&!e.disabled)`),'orchestration capability fallback');await textClick('新建协作');await until(()=>evaluate(`!!document.querySelector('.orch-create textarea')`),'create dialog');
  await fill('.orch-create textarea','修复终端输入延迟');await fill('.orch-create label:nth-child(3) textarea','输入回归通过，原有会话保持可用');await textClick('生成协作方案');
  await until(()=>evaluate(`document.querySelectorAll('.orch-node-editor').length===2`),'draft');
  checks.generated_draft=true;
  await fill('.orch-node-editor input:not([type=checkbox])','定位并说明输入延迟');
  checks.unsaved_confirmation_disabled=await evaluate(`[...document.querySelectorAll('button')].find(e=>e.textContent==='确认并启动').disabled`);
  await textClick('保存方案');await until(()=>evaluate(`![...document.querySelectorAll('button')].find(e=>e.textContent==='确认并启动').disabled`),'saved draft');
  await shot('plan-light');await textClick('确认并启动');await until(()=>evaluate(`!!document.querySelector('dialog[open]')`),'confirmation');
  checks.no_dispatch_before_confirmation=await evaluate(`!orchestrationFixture.requests.some(r=>r.cmd==='orchestration.plan.confirm')`);
  await textClick('确认',`document.querySelector('dialog[open]')`);await until(()=>evaluate(`!!document.querySelector('.orch-graph-node')`),'dependency graph');
  checks.three_nodes=await evaluate(`document.querySelectorAll('.orch-graph-node').length===3`);
  await click('.orch-graph-node[aria-label^="修复并验证"]');
  checks.graph_selects_task=await evaluate(`document.querySelector('.orch-task-detail').textContent.includes('修复并验证')`);
  await textClick('暂停派发');await until(()=>evaluate(`document.querySelector('.orch-run-heading').textContent.includes('已暂停派发')`),'paused');checks.pause=true;
  await textClick('继续核对');await until(()=>evaluate(`document.querySelector('.orch-run-heading').textContent.includes('执行中')`),'resumed');checks.resume=true;
  await evaluate('orchestrationFixture.block()');await until(()=>evaluate(`document.querySelector('.orch-alert')?.textContent.includes('原生授权')`),'permission blocker');checks.permission_blocker=true;await shot('run-light');
  await click('button[aria-label="切换深色主题"]');await shot('run-dark');
  for(const width of [1440,1100,900]){await cdp('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await delay(120);checks['fits_'+width]=await evaluate('document.documentElement.scrollWidth<=innerWidth');}
  await evaluate('orchestrationFixture.finish()');await textClick('交付与验收');await until(()=>evaluate(`!!document.querySelector('.orch-accept textarea')`),'acceptance');
  checks.explicit_final_gate=await evaluate(`[...document.querySelectorAll('button')].find(e=>e.textContent==='确认总体验收').disabled`);
  await fill('.orch-accept textarea','已核对目标、变更及验证证据');await textClick('确认总体验收');await until(()=>evaluate(`document.querySelector('.orch-delivery h3').textContent==='本次协作已验收'`),'accepted');checks.final_acceptance=true;await shot('delivery-dark');
  await textClick('工作台');await until(()=>evaluate(`!!document.querySelector('.wb-content')`),'workbench preserved');checks.workbench_navigation=true;
  await textClick('Agent 编排');await until(()=>evaluate(`!!document.querySelector('.orch-run-heading')`),'restored history');checks.history_restored=true;
  await textClick('新建协作');await fill('.orch-create textarea','停止测试');await fill('.orch-create label:nth-child(3) textarea','停止创建的会话');await textClick('生成协作方案');await until(()=>evaluate(`document.querySelector('.orch-run-heading h2')?.textContent==='停止测试'`),'second run');
  await textClick('停止');await until(()=>evaluate(`!!document.querySelector('dialog[open]')`),'stop confirmation');await textClick('确认',`document.querySelector('dialog[open]')`);await until(()=>evaluate(`document.querySelector('.orch-run-heading').textContent.includes('已停止')`),'stopped');checks.stop=true;
  checks.no_real_rpc=escapedRpc===0;checks.no_browser_exceptions=exceptions.length===0;
  for(const [name,pass] of Object.entries(checks))assert.equal(pass,true,name);
 }catch(failure){error=failure;console.error(failure);try{await writeFile(join(output,'failure-context.json'),JSON.stringify(await evaluate(`({body:document.body.innerText,requests:orchestrationFixture.requests.slice(-12),fields:[...document.querySelectorAll('textarea')].map(e=>({value:e.value,valid:e.validity.valid})),dialogs:document.querySelectorAll('[role=dialog]').length})`),null,2));const snap=await cdp('Page.captureScreenshot',{format:'png'});await writeFile(join(output,'failure.png'),Buffer.from(snap.data,'base64'));}catch{}}
finally{
  socket?.close();
  for(const item of pending.values())item.reject(new Error('Browser test closed'));
  pending.clear();
  if(chrome&&chrome.exitCode===null&&chrome.signalCode===null){
    chrome.kill('SIGTERM');
    await Promise.race([new Promise(resolve=>chrome.once('exit',resolve)),delay(2000)]);
    if(chrome.exitCode===null&&chrome.signalCode===null){chrome.kill('SIGKILL');await new Promise(resolve=>chrome.once('exit',resolve))}
  }
  await server?.close();
  if(!profile.startsWith(join(tmpdir(),'velella-interface-')))throw Error('Unexpected test profile path');
  await rm(profile,{recursive:true,force:true});
  await mkdir(output,{recursive:true});
  await writeFile(join(output,'result.json'),JSON.stringify({
    status:error?'failed':'passed',scope:'Actual WorkspaceApp and orchestration console with in-memory RPC fixture; no real models or runtime',
    checks,snapshots,exceptions,escapedRpc,temporaryProfileRemoved:true,
    error:error?String(error):null,
  },null,2)+'\n');
}
if(error)process.exitCode=1;
else console.log(`Passed ${Object.keys(checks).length} browser assertions; evidence: ${output}`);
