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
const output=fileURLToPath(new URL('../../artifacts/restricted-terminal-ui/',import.meta.url));
const profile=await mkdtemp(join(tmpdir(),'velella-terminal-ui-'));
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
const qa=()=>evaluate('JSON.parse(document.querySelector("#qa-summary").textContent)');
async function clickText(text){
  assert.equal(await evaluate(`(()=>{const el=[...document.querySelectorAll('button,summary')].find(el=>el.textContent.trim()===${JSON.stringify(text)});if(!el||el.disabled)return false;el.click();return true})()`),true,`click ${text}`);
}
async function select(selector,value){
  assert.equal(await evaluate(`(()=>{const el=document.querySelector(${JSON.stringify(selector)});if(!el||el.disabled)return false;const option=[...el.options].find(o=>o.value===${JSON.stringify(value)});if(!option||option.disabled)return false;el.value=${JSON.stringify(value)};el.dispatchEvent(new Event('change',{bubbles:true}));return true})()`),true,`select ${value}`);
}
async function open(scenario,extra=''){
  const url=`${server.resolvedUrls.local[0]}tests/workbench-terminal-ui.html?scenario=${scenario}${extra}`;
  await cdp('Page.navigate',{url});
  await until(()=>evaluate('!!document.querySelector(\'button[aria-label="在 浏览器测试 Git 项目 中新建工作区"]:not(:disabled)\')'),'workspace navigation');
  const name=scenario==='direct'?'浏览器测试普通文件夹':'浏览器测试 Git 项目';
  await evaluate(`document.querySelector(${JSON.stringify(`button[aria-label="在 ${name} 中新建工作区"]`)}).click()`);
  await until(()=>evaluate('!!document.querySelector(".workspace-composer[open]")'),'composer');
  if(scenario!=='direct'){
    await evaluate(`(()=>{const el=document.querySelector('input[placeholder="例如：界面改版"]');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(el,'终端验收');el.dispatchEvent(new Event('input',{bubbles:true}))})()`);
  }
  await select('select[aria-label="创建后"]','codex');
  await until(async()=> (await qa()).probeCalls===1,'initial probe');
}
const mode='select[aria-describedby="terminal-execution-status terminal-execution-scope"]';
async function screenshot(name){
  const result=await cdp('Page.captureScreenshot',{format:'png'});
  await writeFile(join(output,name+'.png'),Buffer.from(result.data,'base64'));
}
async function terminalReady(){
  await until(async()=> (await qa()).simulatedSessionCount===1,'simulated terminal session');
  await until(()=>evaluate('!!document.querySelector(".terminal-canvas:not(.terminal-history) .xterm")'),'interactive TerminalView');
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

  await open('ready');
  checks.no_chat_creation_option=await evaluate('!document.querySelector(\'option[value="codex-chat"]\')');
  checks.native_default=await evaluate(`document.querySelector(${JSON.stringify(mode)}).value==='native'`);
  await select(mode,'restricted');
  checks.scope_copy=await evaluate(`['原有 Codex 终端界面','本机已有的 ChatGPT 登录','模型 API 连接','沙箱工具无法读取','支持 !命令'].every(text=>document.querySelector('#terminal-execution-scope').textContent.includes(text))`);
  await clickText('高级设置');
  checks.managed_goal_disabled=await evaluate('document.querySelector(\'textarea[aria-label="工作目标"]\').disabled');
  checks.no_local_login_check=await evaluate('![...document.querySelectorAll(".workspace-composer button")].some(el=>el.textContent.includes("检查 CLI 与登录"))');
  await screenshot('restricted-composer');
  await clickText('创建并打开');
  await terminalReady();
  let state=await qa();
  checks.original_codex_launch=state.lastOpenRequest.launch==='codex'&&state.lastOpenRequest.sandbox.mode==='restricted';
  checks.terminal_tab=state.assertions.everySavedTabIsTerminal&&state.assertions.everySuccessfulSessionUsesTerminal;
  checks.no_chat_rpc=state.assertions.noChatRpc;
  checks.restricted_skips_local_auth_probe=!state.counters['agent.check'];
  checks.restricted_title=await evaluate('[...document.querySelectorAll(\'[role="tab"]\')].some(el=>el.textContent.includes("Codex · 工具受限"))');
  await evaluate('document.querySelector(".xterm-helper-textarea").focus()');
  await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,text:'a'});
  await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',windowsVirtualKeyCode:65});
  await until(async()=>!!(await qa()).counters['workbench.terminal.input'],'recorded terminal keystroke');
  checks.original_terminal_input=true;
  await until(async()=>!!(await qa()).counters['workbench.terminal.resize'],'recorded terminal resize');
  checks.original_terminal_resize=true;
  snapshots.restricted=await qa();
  await screenshot('restricted-terminal');

  await open('direct');
  checks.direct_restricted_disabled=await evaluate(`document.querySelector(${JSON.stringify(mode)}).querySelector('option[value="restricted"]').disabled`);
  checks.direct_native_retained=await evaluate(`document.querySelector(${JSON.stringify(mode)}).value==='native'`);
  snapshots.direct=await qa();

  await open('unavailable');
  checks.unavailable_disabled=await evaluate(`document.querySelector(${JSON.stringify(mode)}).querySelector('option[value="restricted"]').disabled`);
  snapshots.unavailable=await qa();

  await open('reprobe-failure');
  await select(mode,'restricted');
  await clickText('重新检查');
  await until(async()=> (await qa()).probeCalls===2,'failed reprobe');
  checks.reprobe_preserves_selection=await evaluate(`document.querySelector(${JSON.stringify(mode)}).value==='restricted'`);
  checks.reprobe_blocks_submit=await evaluate('[...document.querySelectorAll(".workspace-composer button")].find(el=>el.textContent==="创建并打开").disabled');
  snapshots.reprobe=await qa();
  checks.reprobe_did_not_create=snapshots.reprobe.createCalls===0&&snapshots.reprobe.openCalls===0;

  await open('launch-failure');
  await select(mode,'restricted');
  await clickText('创建并打开');
  await until(()=>evaluate('!![...document.querySelectorAll(".workspace-composer button")].find(el=>el.textContent==="继续打开"&&!el.disabled)'),'retry button');
  checks.first_launch_error=await evaluate('document.querySelector(".workspace-composer [role=alert]").textContent.includes("工作区已保留")');
  await clickText('继续打开');
  await terminalReady();
  state=await qa();
  checks.retry_one_create=state.createCalls===1&&state.createdWorkspaceCount===1&&state.openCalls===2;
  checks.retry_same_mode_and_id=state.assertions.retryUsedSameRequestId&&state.assertions.retryKeptRestrictedMode;
  snapshots.retry=state;

  await open('ready','&native=1');
  await clickText('创建并打开');
  await terminalReady();
  state=await qa();
  checks.native_payload_unchanged=state.lastOpenRequest.launch==='codex'&&!('sandbox' in state.lastOpenRequest);
  checks.native_still_terminal=state.assertions.everySavedTabIsTerminal&&state.assertions.everySuccessfulSessionUsesTerminal;
  snapshots.native=state;
  checks.no_real_rpc=escapedRpc===0&&network.every(url=>!new URL(url).pathname.startsWith('/_velella'));
  checks.local_resources_only=network.every(url=>new URL(url).origin===new URL(server.resolvedUrls.local[0]).origin);
  checks.no_browser_exceptions=exceptions.length===0;
  for(const [name,passed] of Object.entries(checks))assert.equal(passed,true,name);
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
