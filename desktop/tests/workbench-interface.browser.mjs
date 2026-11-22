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
const output=process.env.WORKBENCH_QA_OUTPUT||fileURLToPath(new URL('../../artifacts/workbench-interface/',import.meta.url));
const profile=await mkdtemp(join(tmpdir(),'velella-interface-'));
const checks={},snapshots={},exceptions=[],network=[];
let chrome,socket,server,escapedRpc=0,error;
let sequence=0;
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
    }],server:{host:'127.0.0.1',port:0,fs:{allow:[root,process.env.VELELLA_QA_SHARED_ROOT||root]}},
  });
  await server.listen();
  chrome=spawn(process.env.CHROME_BIN||'/usr/bin/google-chrome',[
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
  async function loaded(){await until(()=>evaluate('!!document.querySelector(".workspace-info") && !document.querySelector("button[aria-label=新建标签]")?.disabled'),'workbench ready')}
  await cdp('Page.navigate',{url:base+'tests/workbench-interface.html?task&theme=dark'});await loaded();
  checks.navigation=await evaluate(`['工作台','自动化','任务来源','工作区工具'].every(label=>[...document.querySelectorAll('.ide-sidebar .ide-nav')].some(button=>button.textContent===label))`);
  checks.a_shell_geometry=await evaluate(`document.querySelector('.ide-sidebar').getBoundingClientRect().width===180&&document.querySelector('.app-titlebar').getBoundingClientRect().height===48&&document.querySelector('.ide-sidebar').getBoundingClientRect().top===0`);
  checks.a_brand=await evaluate(`document.querySelector('.workbench-wordmark').textContent==='Velella'&&!!document.querySelector('.workbench-wordmark svg path')`);
  checks.tabs_below_topbar=await evaluate(`document.querySelector('.workbench-tabbar').getBoundingClientRect().top>=document.querySelector('.app-titlebar').getBoundingClientRect().bottom`);
  checks.permission_visible=await evaluate(`document.querySelector('.workspace-task-indicator').textContent==='等待授权'`);
  checks.tabs_prioritized=await evaluate(`!document.querySelector('.chrome-actions .workspace-branch')&&!!document.querySelector('.workspace-info .workspace-branch')`);
  for(const theme of ['dark','light']){
    if(theme==='light')await click('button[aria-label="切换浅色主题"]');
    await until(()=>evaluate(`document.documentElement.dataset.theme===${JSON.stringify(theme)}`),'theme');
    for(const [width,height] of [[1440,900],[1100,760],[900,700]]){
      await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      await delay(180);
      checks[`${theme}_${width}_fits`]=await evaluate(`(()=>{const es=['.app-titlebar','.workspace-info','.wb-content'];return document.documentElement.scrollWidth<=innerWidth && es.every(s=>{const r=document.querySelector(s).getBoundingClientRect();return r.left>=0&&r.right<=innerWidth+.5&&r.width>0})})()`);
      checks[`${theme}_${width}_branch`]=await evaluate(`getComputedStyle(document.querySelector('.workspace-branch')).display===${JSON.stringify(width<1100?'none':'flex')}`);
      checks[`${theme}_${width}_terminal_background`]=await evaluate(`getComputedStyle(document.querySelector('.xterm-viewport')).backgroundColor===getComputedStyle(document.querySelector('.terminal-wrap')).backgroundColor`);
      snapshots[`${theme}_${width}_terminal`]=await evaluate(`(()=>{let e=document.elementFromPoint(innerWidth-100,innerHeight-100);const out=[];while(e){out.push({tag:e.tagName,cls:e.className,bg:getComputedStyle(e).backgroundColor});e=e.parentElement}return out})()`);
      await shot(`${theme}-${width}`);
    }
  }
  await click('button[aria-label="分支与更改"]');
  checks.compact_branch_menu=await evaluate(`document.querySelector('[role=menu]').textContent.includes('codex/workbench-interface-refresh-long-branch-name')`);
  await key('Escape');
  await click('button[aria-label="新建标签"]');
  await until(()=>evaluate('!!document.querySelector("[role=menu]")'),'menu');
  checks.menu_groups=await evaluate(`['终端与会话','文件与浏览器','审查'].every(t=>[...document.querySelectorAll('.wb-menu-label')].some(e=>e.textContent===t))`);
  await key('ArrowDown');
  checks.menu_keyboard=await evaluate(`document.activeElement.getAttribute('role')==='menuitem'`);
  await key('Escape');await until(()=>evaluate('!document.querySelector("[role=menu]")'),'menu closed');
  snapshots.focusAfterEscape=await evaluate(`({active:document.activeElement.outerHTML,menu:!!document.querySelector('[role=menu]')})`);
  checks.menu_focus_return=await evaluate(`document.activeElement.getAttribute('aria-label')==='新建标签'`);
  await click('button[aria-label="展开工具栏"]');await delay(100);
  checks.right_panel_visible=await evaluate(`!!document.querySelector('.wb-files')&&document.querySelector('.wb-content').getBoundingClientRect().width>250`);
  await shot('tools-900');
  await click('button[aria-label="布局与标签"]');
  await evaluate(`[...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='分屏').click()`);
  await until(()=>evaluate('document.querySelectorAll(".wb-pane").length===2'),'split');
  checks.split_visible=true;await shot('split-900');
  await click('button[aria-label="布局与标签"]');await evaluate(`[...document.querySelectorAll('[role=menuitem]')].find(e=>e.textContent==='合并分屏').click()`);
  await until(()=>evaluate('document.querySelectorAll(".wb-pane").length===1'),'merged');
  await click('button[aria-label="关闭标签 Codex · 界面改进"]');
  await until(()=>evaluate('window.interfaceQA.layouts["task:ui-task"].panes[0].tabs.length===4'),'close saved');
  checks.close_keeps_session=await evaluate(`window.interfaceQA.sessions.length===1&&!window.interfaceQA.requests.some(r=>/stop|kill/.test(r.cmd))`);
  const pageLoaded=new Promise(resolve=>{const listener=event=>{if(JSON.parse(event.data).method==='Page.loadEventFired'){socket.removeEventListener('message',listener);resolve()}};socket.addEventListener('message',listener)});
  await cdp('Page.reload');await pageLoaded;await loaded();
  checks.reload_restores_layout=await evaluate(`document.querySelectorAll('.ide-tab').length===4&&!!document.querySelector('.wb-files')`);
  await evaluate(`[...document.querySelectorAll('.ide-workspace')].find(e=>e.textContent.includes('准备开始')).click()`);
  await until(()=>evaluate('!!document.querySelector(".workspace-overview")'),'empty workspace');
  checks.empty_primary=await evaluate(`document.querySelector('.workspace-overview-actions .primary').textContent==='新建 Codex 终端'`);
  checks.a_overview=await evaluate(`document.querySelector('.workspace-overview-label').textContent==='WORKSPACE / OPERATIONS'&&document.querySelectorAll('.workbench-overview-summary strong').length===2`);
  checks.no_task_tab=await evaluate(`![...document.querySelectorAll('.right-panel-tabs [role=tab]')].some(e=>e.textContent==='任务')`);
  await shot('empty-900');
  await cdp('Emulation.setDeviceMetricsOverride',{width:1440,height:900,deviceScaleFactor:1,mobile:false});
  await delay(120);await shot('overview-dark-1440');
  await click('button[aria-label="切换浅色主题"]');
  await delay(120);await shot('overview-light-1440');
  checks.overview_columns=await evaluate(`document.querySelectorAll('.workspace-overview-section')[0].getBoundingClientRect().right<document.querySelectorAll('.workspace-overview-section')[1].getBoundingClientRect().left`);
  await click('.workspace-overview-actions .primary');await until(()=>evaluate('!!document.querySelector("dialog[open]")'),'launcher');
  checks.launcher_preserved=await evaluate(`document.querySelector('dialog[open]').textContent.includes('执行模式')`);
  await key('Escape');await until(()=>evaluate('!document.querySelector("dialog[open]")'),'launcher closed');
  await click('button[aria-label="收起项目栏"]');await until(()=>evaluate(`!!document.querySelector('button[aria-label="展开项目栏"]')`),'sidebar collapsed');
  checks.sidebar_collapsed=await evaluate(`document.querySelector('.ide-shell').classList.contains('left-hidden')`);
  await click('button[aria-label="展开项目栏"]');
  checks.no_real_rpc=escapedRpc===0;
  checks.no_fixture_rpc_errors=await evaluate(`!document.body.textContent.includes('Fixture refused RPC')`);
  checks.no_browser_exceptions=exceptions.length===0;
  for(const [name,pass] of Object.entries(checks))assert.equal(pass,true,name);
}catch(failure){error=failure;console.error(failure)}
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
  await rm(profile,{recursive:true,force:true});
  await mkdir(output,{recursive:true});
  await writeFile(join(output,'result.json'),JSON.stringify({
    status:error?'failed':'passed',scope:'Mocked full WorkspaceApp + actual TerminalView; no CLI/model/login/runtime',
    checks,snapshots,exceptions,escapedRpc,temporaryProfileRemoved:true,
    error:error?String(error):null,
  },null,2)+'\n');
}
if(error)process.exitCode=1;
else console.log(`Passed ${Object.keys(checks).length} browser assertions; evidence: ${output}`);
