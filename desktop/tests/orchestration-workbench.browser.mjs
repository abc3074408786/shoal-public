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
const output=process.env.VELELLA_UI_EVIDENCE_DIR||fileURLToPath(new URL('../../artifacts/agent-orchestration/workbench-preservation/',import.meta.url));
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
  async function click(selector){
    const locate=()=>evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e||e.disabled)throw Error('Missing enabled '+${JSON.stringify(selector)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect(),p={x:r.x+r.width/2,y:r.y+r.height/2},hit=document.elementFromPoint(p.x,p.y);return {...p,covered:!e.contains(hit),hit:hit?.outerHTML.slice(0,300)}})()`);
    let point=await locate();await cdp('Input.dispatchMouseEvent',{type:'mouseMoved',x:point.x,y:point.y});
    await until(async()=>{point=await locate();return !point.covered},'uncovered click target '+selector);
    snapshots.lastClick={selector,...point};
    for(const type of ['mousePressed','mouseReleased'])await cdp('Input.dispatchMouseEvent',{type,x:point.x,y:point.y,button:'left',clickCount:1});
  }
  async function key(key,code=key){const keyCode={Escape:27,ArrowDown:40,ArrowUp:38,Enter:13,Tab:9}[key];await cdp('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode:keyCode,nativeVirtualKeyCode:keyCode});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:keyCode,nativeVirtualKeyCode:keyCode});}
  async function loaded(){await until(()=>evaluate('!!document.querySelector(".wb-content") && !!document.querySelector("button[aria-label=新建标签]") && !document.querySelector("button[aria-label=新建标签]").disabled'),'workbench ready')}

  async function textClick(text,scope='document'){
    const point=await evaluate(`(()=>{const e=[...${scope}.querySelectorAll('button')].find(e=>e.textContent.trim()===${JSON.stringify(text)}&&!e.disabled);if(!e)throw Error('Missing button '+${JSON.stringify(text)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await cdp('Input.dispatchMouseEvent',{type,...point,button:'left',clickCount:1});
  }
  async function fill(selector,value){await click(selector);await evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});await cdp('Input.insertText',{text:value});}

  await cdp('Page.navigate',{url:base+'tests/workbench-interface.html?task&theme=dark&human-control'});
  await until(()=>evaluate('!!document.querySelector(".wb-content") && !!document.querySelector(".xterm") && !document.querySelector("button[aria-label=新建标签]")?.disabled'),'installed workbench');
  checks.a_brand=await evaluate(`!!document.querySelector('.workbench-wordmark')&&document.querySelector('.ide-shell').classList.contains('workbench-a-shell')`);
  checks.separate_tab_row=await evaluate(`(()=>{const t=document.querySelector('.workbench-tabbar').getBoundingClientRect(),h=document.querySelector('.app-titlebar').getBoundingClientRect();return t.top>=h.bottom-1})()`);
  await until(()=>evaluate('!!document.querySelector(".workbench-tabbar .terminal-toolbar-actions")'),'terminal actions in tab bar');
  checks.no_terminal_heading=await evaluate('!document.querySelector(".terminal-toolbar,.terminal-caption")');
  checks.mist_terminal_gray=await evaluate('getComputedStyle(document.querySelector(".terminal-canvas")).backgroundColor==="rgb(35, 45, 42)"');
  checks.mist_terminal_spacing=await evaluate('getComputedStyle(document.querySelector(".terminal-canvas")).padding==="16px"');
  checks.mist_joined_frame=await evaluate(`(()=>{const bar=document.querySelector('.workbench-tabbar'),body=document.querySelector('.wb-body'),a=bar.getBoundingClientRect(),b=body.getBoundingClientRect();return Math.abs(a.left-b.left)<1&&Math.abs(a.right-b.right)<1&&getComputedStyle(bar).borderTopLeftRadius==='8px'&&getComputedStyle(body).borderBottomLeftRadius==='8px'})()`);
  checks.mist_integrated_tab=await evaluate(`(()=>{const tab=document.querySelector('.workbench-tabbar .ide-tab.selected'),canvas=document.querySelector('.terminal-canvas');return getComputedStyle(tab).backgroundColor===getComputedStyle(canvas).backgroundColor&&getComputedStyle(tab).borderTopLeftRadius==='6px'})()`);
  await shot('mist-terminal-single');
  checks.terminal_actions_in_tabs=await evaluate(`(()=>{const bar=document.querySelector('.workbench-tabbar').getBoundingClientRect(),actions=document.querySelector('.terminal-toolbar-actions').getBoundingClientRect();return document.querySelectorAll('.terminal-toolbar-actions').length===1&&actions.top>=bar.top&&actions.bottom<=bar.bottom})()`);
  checks.initial_sessions=await evaluate(`interfaceQA.sessions.length===1&&document.querySelectorAll('.ide-tab').length===5`);
  checks.control_not_automatically_released=await evaluate(`!interfaceQA.requests.some(r=>r.cmd==='workbench.terminal.control')`);
  await click('button[aria-label="终端操作"]');
  await until(()=>evaluate(`!!document.querySelector('[role=menu]')`),'terminal menu');
  assert.equal(await evaluate(`!![...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='继续后台检查')`),true,'native terminal exposes return control');
  checks.native_return_control=true;
  await evaluate(`[...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='继续后台检查').click()`);
  await until(()=>evaluate(`interfaceQA.requests.some(r=>r.cmd==='workbench.terminal.control'&&r.session_id==='ui-session'&&r.human===false)`),'explicit return control receipt');
  checks.control_return_scoped_to_session=true;
  await key('Escape');
  await click('.ide-sidebar button[aria-label="Agent 编排"]');await until(()=>evaluate('!!document.querySelector(".orch-page")'),'orchestration');
  await click('.ide-sidebar button[aria-label="工作台"]');await loaded();
  checks.return_keeps_tabs=await evaluate(`document.querySelectorAll('.ide-tab').length===5&&interfaceQA.sessions.length===1`);
  await click('button[aria-label="布局与标签"]');await until(()=>evaluate(`!!document.querySelector('[role=menu]')`),'layout menu');await evaluate(`[...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='分屏').click()`);await until(()=>evaluate('document.querySelectorAll(".wb-pane").length===2'),'split');checks.split=true;
  await until(()=>evaluate('document.querySelectorAll(".wb-tabbar .terminal-toolbar-actions").length===1'),'pane terminal actions');
  checks.split_actions_in_pane_tabs=await evaluate('!document.querySelector(".workbench-tabbar .terminal-toolbar-actions")');
  checks.mist_split_frame=await evaluate(`(()=>{const pane=document.querySelector('.wb-pane:has(>.wb-terminal)'),tabs=pane.querySelector('.wb-tabbar'),screen=pane.querySelector('.terminal-canvas'),a=pane.getBoundingClientRect(),b=tabs.getBoundingClientRect(),c=screen.getBoundingClientRect();return getComputedStyle(pane).borderTopLeftRadius==='8px'&&b.left>=a.left&&b.right<=a.right&&c.top>=b.bottom})()`);
  await shot('mist-terminal-split');
  await click('button[aria-label="布局与标签"]');await until(()=>evaluate(`!![...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='合并分屏')`),'merge menu');await evaluate(`[...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='合并分屏').click()`);await until(()=>evaluate('document.querySelectorAll(".wb-pane").length===1'),'merge');checks.merge=true;
  if(await evaluate(`!!document.querySelector('button[aria-label="展开工具栏"]')`))await click('button[aria-label="展开工具栏"]');
  await until(()=>evaluate(`!!document.querySelector('.wb-files')`),'tools visible');checks.tools_open=true;
  await click('button[aria-label="关闭右侧工具栏"]');await until(()=>evaluate(`!document.querySelector('.wb-files')`),'tools closed');checks.tools_close=true;
  await click('button[aria-label="关闭标签 Codex · 界面改进"]');await until(()=>evaluate('interfaceQA.layouts["task:ui-task"].panes[0].tabs.length===4'),'close persisted');
  checks.close_keeps_session=await evaluate(`interfaceQA.sessions.length===1&&!interfaceQA.requests.some(r=>/stop|kill/.test(r.cmd))`);
  const loadsBeforeReload=documentLoads;await cdp('Page.reload');await until(()=>documentLoads>loadsBeforeReload,'reloaded document');await loaded();checks.reload_restores_tabs=await evaluate(`document.querySelectorAll('.ide-tab').length===4`);
  for(const width of [1440,1100,900]){await cdp('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await delay(100);checks['fit_'+width]=await evaluate('document.documentElement.scrollWidth<=innerWidth');}
  await shot('workbench-a-preserved');
  await evaluate(`[...document.querySelectorAll('.ide-workspace')].find(e=>e.textContent.includes('准备开始')).click()`);await until(()=>evaluate('!!document.querySelector(".workspace-overview")'),'overview');
  checks.native_entry=await evaluate(`document.querySelector('.workspace-overview-actions .primary').textContent==='新建 Codex 终端'`);
  checks.hidden_sessions_have_no_actions=await evaluate('document.querySelectorAll(".terminal-toolbar-actions").length===0');
  checks.mist_frame_absent_on_overview=await evaluate('getComputedStyle(document.querySelector(".ide-main")).paddingInlineStart==="0px"&&getComputedStyle(document.querySelector(".workbench-tabbar")).borderTopLeftRadius==="0px"');
  await click('.workspace-overview-actions .primary');await until(()=>evaluate('!!document.querySelector("dialog[open]")'),'native launcher');checks.native_launcher=await evaluate(`document.querySelector('dialog[open]').textContent.includes('执行模式')`);await key('Escape');
  checks.no_real_rpc=escapedRpc===0;checks.no_browser_exceptions=exceptions.length===0;
  for(const [name,pass] of Object.entries(checks))assert.equal(pass,true,name);
 }catch(failure){error=failure;console.error(failure);try{await writeFile(join(output,'failure-context.json'),JSON.stringify(await evaluate(`({body:document.body.innerText,requests:interfaceQA.requests.slice(-12),layouts:interfaceQA.layouts,fields:[...document.querySelectorAll('textarea')].map(e=>({value:e.value,valid:e.validity.valid})),dialogs:document.querySelectorAll('[role=dialog]').length})`),null,2));const snap=await cdp('Page.captureScreenshot',{format:'png'});await writeFile(join(output,'failure.png'),Buffer.from(snap.data,'base64'));}catch{}}
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
    status:error?'failed':'passed',scope:'Installed A workbench preservation: actual WorkspaceApp/terminal component, fixture RPC only',
    checks,snapshots,exceptions,escapedRpc,temporaryProfileRemoved:true,
    error:error?String(error):null,
  },null,2)+'\n');
}
if(error)process.exitCode=1;
else console.log(`Passed ${Object.keys(checks).length} browser assertions; evidence: ${output}`);
