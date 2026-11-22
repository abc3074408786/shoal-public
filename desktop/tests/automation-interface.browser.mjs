// Explicit browser regression: node tests/automation-interface.browser.mjs
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
const output=process.env.AUTOMATION_QA_OUTPUT||fileURLToPath(new URL('../../artifacts/automation-interface/',import.meta.url));
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
      configureServer(s){s.middlewares.use((req,_res,next)=>{if(req.url==='/tests/velella.png')req.url='/velella.png';next()});s.middlewares.use('/_velella',(_req,res)=>{
        escapedRpc++;res.statusCode=500;res.end('No real RPC bridge exists in this test');
      })},
    }],server:{host:'127.0.0.1',port:0,fs:{allow:[root,process.env.VELELLA_QA_SHARED_ROOT||root]}},
  });
  await server.listen();
  chrome=spawn(process.env.CHROME_BIN||(process.platform==='win32'?'C:/Program Files/Google/Chrome/Application/chrome.exe':'/usr/bin/google-chrome'),[
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
  async function click(selector){
    await until(()=>evaluate(`!!document.querySelector(${JSON.stringify(selector)})&&!document.querySelector(${JSON.stringify(selector)}).disabled`),'enabled '+selector);
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`);
    let previous=null,point;
    await until(async()=>{const state=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)}),r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return{x,y,hit:e.contains(document.elementFromPoint(x,y))}})()`);point=state;const stable=previous&&Math.abs(previous.x-state.x)<0.1&&Math.abs(previous.y-state.y)<0.1;previous=state;return stable&&state.hit},'stable hit target '+selector);
    for(const type of ['mousePressed','mouseReleased'])await cdp('Input.dispatchMouseEvent',{type,x:point.x,y:point.y,button:'left',clickCount:1});
  }
  async function key(key,code=key){const keyCode={Escape:27,ArrowDown:40,ArrowUp:38,Enter:13,Tab:9}[key];await cdp('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode:keyCode,nativeVirtualKeyCode:keyCode});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:keyCode,nativeVirtualKeyCode:keyCode});}

  async function byText(text){await until(()=>evaluate(`[...document.querySelectorAll('button')].some(e=>e.getClientRects().length&&e.textContent.trim()===${JSON.stringify(text)}&&!e.disabled)`),'enabled '+text);await evaluate(`(()=>{const e=[...document.querySelectorAll('button')].find(e=>e.getClientRects().length&&e.textContent.trim()===${JSON.stringify(text)});if(!e)throw Error('Missing '+${JSON.stringify(text)});e.dataset.testClick='yes'})()`);await click('[data-test-click="yes"]');await evaluate(`document.querySelector('[data-test-click]')?.removeAttribute('data-test-click')`)}
  await cdp('Page.navigate',{url:base+'tests/automation-interface.html?task&theme=dark'});
  await until(()=>evaluate('!!document.querySelector(".wb-content")'),'app');await byText('自动化');
  await until(()=>evaluate('document.querySelectorAll(".automation-template").length===4'),'templates');
  checks.no_write_on_open=await evaluate(`!window.interfaceQA.requests.some(r=>['automation.save','automation.run'].includes(r.cmd))`);
  // The same shell must stay mounted across page navigation, including its
  // project selection, resize state, theme control and all four destinations.
  await evaluate('window.originalShell=document.querySelector(".ide-shell");window.originalSidebar=document.querySelector(".ide-sidebar");true');
  for(const theme of ['dark','light']){
    if(theme==='light'){await byText('工作台');await click('button[aria-label="切换浅色主题"]');await byText('自动化')}
    for(const [width,height] of [[2560,1556],[1440,900],[1100,760],[900,700]]){
      await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await delay(180);
      checks[`${theme}_${width}_no_overflow`]=await evaluate('document.documentElement.scrollWidth<=innerWidth');await shot(`${theme}-${width}`);
      checks[`${theme}_${width}_shared_shell`]=await evaluate('document.querySelector(".ide-shell")===window.originalShell&&document.querySelector(".ide-sidebar")===window.originalSidebar&&document.querySelectorAll(".ide-sidebar").length===1&&getComputedStyle(window.originalSidebar).display!=="none"&&getComputedStyle(document.querySelector(".app-titlebar")).display!=="none"&&!document.querySelector(".automation-sidebar")');
    }
  }
  await click('.automation-template');await until(()=>evaluate('!!document.querySelector("dialog[open] input[value=每日仓库审计]")'),'prefilled');
  checks.template_paused=await evaluate('!document.querySelectorAll(".automation-editor input[type=checkbox]")[0].checked');
  checks.save_needs_review=await evaluate(`[...document.querySelectorAll('dialog button')].find(e=>e.textContent==='保存自动化').disabled`);
  checks.no_write_on_template=await evaluate(`!window.interfaceQA.requests.some(r=>r.cmd==='automation.save')`);await delay(250);await shot('editor-900');
  await key('Escape');await until(()=>evaluate('!document.querySelector("dialog[open]")'),'closed');
  checks.focus_return=await evaluate('document.activeElement.classList.contains("automation-template")');
  await click('.automation-template');await until(()=>evaluate('!!document.querySelector("dialog[open]")'),'reopen');await click('.automation-editor .ack:last-of-type input');await byText('保存自动化');
  await until(()=>evaluate('document.querySelectorAll(".automation-row").length===1'),'saved');checks.saved_paused=await evaluate('window.interfaceQA.automations[0].enabled===false');
  await byText('编辑日程');await until(()=>evaluate('!!document.querySelector("dialog[open]")'),'edit');await until(()=>evaluate('document.querySelector(".automation-editor form h3")?.textContent==="编辑自动化"'),'saved config loaded');
  await click('.automation-editor .ack:nth-last-of-type(2) input');await click('.automation-editor .ack:last-of-type input');await byText('保存自动化');
  await until(()=>evaluate('!document.querySelector("dialog[open]")'),'enabled');checks.saved_enabled=await evaluate('window.interfaceQA.automations[0].enabled===true');
  await byText('暂停');await until(()=>evaluate('window.interfaceQA.automations[0].enabled===false'),'paused');checks.paused=true;
  await byText('编辑日程');await until(()=>evaluate('!!document.querySelector("dialog[open]")'),'run edit');await until(()=>evaluate('document.querySelector(".automation-editor form h3")?.textContent==="编辑自动化"'),'saved config loaded');
  checks.run_needs_review=await evaluate(`[...document.querySelectorAll('dialog button')].find(e=>e.textContent==='立即运行一次').disabled`);
  await click('.automation-editor .ack:last-of-type input');await byText('立即运行一次');await until(()=>evaluate('window.interfaceQA.runs.length===1'),'run');
  checks.confirmed_run=await evaluate(`window.interfaceQA.requests.some(r=>r.cmd==='automation.run'&&r.confirmed===true)`);
  await key('Escape');await click('button[aria-label="刷新自动化"]');await byText('运行记录');
  await until(()=>evaluate('!!document.querySelector(".automation-row")'),'history');await shot('history-900');
  await byText('打开任务');await until(()=>evaluate('!!document.querySelector(".wb-content")'),'task opened');checks.open_task=true;
  await byText('自动化');await until(()=>evaluate('!!document.querySelector(".automation-row")'),'back');await click('button[aria-controls="automation-filters"]');
  await evaluate(`(()=>{const e=document.querySelectorAll('#automation-filters select')[1];e.value='enabled';e.dispatchEvent(new Event('change',{bubbles:true}))})()`);await delay(80);
  checks.filter_enabled=await evaluate('!document.querySelector(".automation-row")');await byText('清除筛选');await until(()=>evaluate('!!document.querySelector(".automation-row")'),'clear');
  await click('input[aria-label="搜索自动化"]');await cdp('Input.insertText',{text:'不存在的自动化'});await delay(100);
  checks.search_empty=await evaluate('!document.querySelector(".automation-row")&&document.querySelector(".automations-empty").textContent.includes("筛选")');
  await byText('清除筛选');await until(()=>evaluate('!!document.querySelector(".automation-row")'),'search cleared');
  await evaluate(`(()=>{const e=document.querySelector('#automation-filters select');e.value='ui-project';e.dispatchEvent(new Event('change',{bubbles:true}))})()`);await delay(80);
  checks.project_filter=await evaluate('document.querySelectorAll(".automation-row").length===1');
  await evaluate('window.interfaceQA.fail()');await click('button[aria-label="刷新自动化"]');await until(()=>evaluate('document.querySelector(".automations-page [role=alert]")?.textContent.includes("模拟连接失败")'),'refresh error');checks.error_visible=true;
  await click('button[aria-label="刷新自动化"]');await until(()=>evaluate('!document.querySelector(".automations-page [role=alert]")'),'retry');checks.retry=true;
  await byText('清除筛选');await evaluate('window.interfaceQA.seed()');await click('button[aria-label="刷新自动化"]');await until(()=>evaluate('document.querySelectorAll(".automation-job").length===3'),'seeded console');
  checks.real_reason=await evaluate('document.querySelector(".automation-result-text").textContent.includes("终端权限请求")');
  checks.task_phase_over_stale_run=await evaluate('document.querySelector(".automation-inspector .automation-state").textContent==="执行中"');
  await click('.automation-checks summary');checks.real_check_output=await evaluate('document.querySelector(".automation-checks").textContent.includes("1 项测试失败")');
  await click('.automation-job:nth-of-type(2) button');checks.select_updates_result=await evaluate('document.querySelector(".automation-result-text").textContent.includes("1 / 1 项验证通过")');
  checks.verified_visible=await evaluate('document.querySelector(".automation-verification").textContent.includes("验证已通过")');
  await click('.automation-job:nth-of-type(3) button');checks.paused_not_scheduled=await evaluate('document.querySelector(".automation-detail-grid").textContent.includes("不会按此时间触发")');
  checks.dispatch_failure_visible=await evaluate('document.querySelector(".automation-result-text").textContent.includes("准备脚本已变化")');
  checks.missing_task_no_open=await evaluate('!document.querySelector(".automation-inspector .automation-open-task")');
  await click('.automation-job:nth-of-type(2) button');await byText('编辑日程');await until(()=>evaluate('!!document.querySelector("dialog[open]")'),'weekly editor');await until(()=>evaluate('document.querySelector(".automation-editor form h3")?.textContent==="编辑自动化"'),'saved config loaded');
  checks.weekly_preset=await evaluate('document.querySelector("select[aria-label=重复方式]").value==="10080"');
  await evaluate(`(()=>{const e=document.querySelector('select[aria-label=重复方式]');e.value='0';e.dispatchEvent(new Event('change',{bubbles:true}))})()`);await click('.automation-editor .ack:last-of-type input');await byText('保存自动化');await until(()=>evaluate('!document.querySelector("dialog[open]")'),'once saved');
  checks.once_interval_preserved=await evaluate('window.interfaceQA.automations.find(a=>a.id==="automation-2").interval_minutes===0');
  await evaluate('window.interfaceQA.seed()');await click('button[aria-label="刷新自动化"]');await click('.automation-job:nth-of-type(1) button');
  for(const theme of ['light','dark']){
   if(theme==='dark'){await byText('工作台');await click('button[aria-label="切换深色主题"]');await byText('自动化')}
   for(const [width,height] of [[1440,900],[1100,760],[900,700]]){
    await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await delay(120);
    checks[`console_${theme}_${width}_no_overflow`]=await evaluate('document.documentElement.scrollWidth<=innerWidth');await evaluate('document.querySelector(".automations-page").scrollTop=0');await shot(`console-${theme}-${width}`);
   }
  }
  checks.reference_a_headline=await evaluate('document.querySelector("#automations-title").textContent==="让重复工作，按计划发生。"');
  checks.reference_a_three_columns=await evaluate('document.querySelectorAll(".automation-list-head>span").length===3');
  checks.reference_a_mint=await evaluate('getComputedStyle(document.querySelector(".automation-create")).backgroundColor==="oklch(0.82 0.1 157)"');
  checks.reference_a_sidebar=await evaluate('Math.round(document.querySelector(".ide-sidebar").getBoundingClientRect().width)===180');
  checks.shared_breadcrumb=await evaluate('document.querySelector(".workbench-breadcrumb").textContent.includes("自动化")&&!document.querySelector(".automation-breadcrumb")');
  checks.no_page_shell=await evaluate('!document.querySelector(".automation-full,.automation-sidebar,.automation-wordmark,.automation-nav")');
  checks.reference_navigation=await evaluate('document.querySelector(".workbench-navigation").textContent==="工作台Agent 编排自动化任务来源"');
  checks.shared_theme=await evaluate('getComputedStyle(document.querySelector(".automations-page")).backgroundColor===getComputedStyle(document.querySelector(".ide-main")).backgroundColor');
  checks.templates_preserved=await evaluate('document.querySelectorAll(".automation-template").length===4');
  checks.no_real_rpc=escapedRpc===0;
  checks.no_browser_exceptions=exceptions.length===0;
  for(const [name,pass] of Object.entries(checks))assert.equal(pass,true,name);
}catch(failure){error=failure;console.error(failure);try{snapshots.failure=await evaluate('({dialog:document.querySelector("dialog")?.outerHTML,active:document.activeElement?.outerHTML})');const image=await cdp('Page.captureScreenshot',{format:'png'});await writeFile(join(output,'failure.png'),Buffer.from(image.data,'base64'))}catch{}}
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
    status:error?'failed':'passed',scope:'Mocked full WorkspaceApp + actual TerminalView; no CLI/model/login/runtime',
    checks,snapshots,exceptions,escapedRpc,temporaryProfileRemoved:true,
    error:error?String(error):null,
  },null,2)+'\n');
}
if(error)process.exitCode=1;
else console.log(`Passed ${Object.keys(checks).length} browser assertions; evidence: ${output}`);
