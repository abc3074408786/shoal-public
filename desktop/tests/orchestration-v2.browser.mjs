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
const output=process.env.VELELLA_UI_EVIDENCE_DIR||fileURLToPath(new URL('../../artifacts/orchestration-v2-20261025/browser-evidence/',import.meta.url));
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
  async function fillAt(selector,index,value){const point=await evaluate(`(()=>{const e=[...document.querySelectorAll(${JSON.stringify(selector)})][${index}];if(!e||e.disabled)throw Error('Missing enabled '+${JSON.stringify(selector)}+' at '+${index});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await cdp('Input.dispatchMouseEvent',{type,...point,button:'left',clickCount:1});await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});await cdp('Input.insertText',{text:value});}
  await cdp('Page.navigate',{url:base+'tests/orchestration-v2.html'});
  await until(()=>evaluate("!!document.querySelector('button[aria-label=\"返回项目首页\"]')"),'project sidebar ready');
  await click('button[aria-label="返回项目首页"]');
  await until(()=>evaluate("(()=>{const e=document.querySelector('.ide-nav[aria-label=\"Agent 编排\"]');return !!e&&!e.disabled})()"),'navigation');
  await click('.ide-nav[aria-label="Agent 编排"]');
  await until(()=>evaluate("[...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='新建协作')"),'v2 ready');
  checks.a_shell=await evaluate("document.querySelector('.ide-shell').classList.contains('workbench-a-shell')");
  await evaluate('setPreflightBlocked(true)');await textClick('新建协作');
  await until(()=>evaluate("document.querySelector('.orch-preflight')?.textContent.includes('还没有 Git 提交')"),'preflight explains missing commit');
  checks.preflight_blocks_create=await evaluate("[...document.querySelectorAll('button')].find(b=>b.textContent==='生成协作方案').disabled && !orchestrationFixture.requests.some(r=>r.cmd==='orchestration.create')");
  await key('Escape');await evaluate('setPreflightBlocked(false)');
  await textClick('新建协作');await until(()=>evaluate("!!document.querySelector('.orch-create textarea')"),'create');
  await fill('.orch-create textarea','完整工作流验收');await fill('.orch-create label:nth-child(3) textarea','目标与检查均有证据');
  await until(()=>evaluate("[...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='生成协作方案'&&!e.disabled)"),'preflight ready');
  await textClick('生成协作方案');await until(()=>evaluate("!!document.querySelector('[aria-label=\"当前协作与会话\"]')"),'execution summary');
  checks.execution_first=await evaluate("!document.querySelector('.orch-node-editor') && document.querySelector('[aria-label=\"当前协作与会话\"]').textContent.includes('方案已准备好')");
  await textClick('查看并确认方案');await until(()=>evaluate("document.querySelectorAll('.orch-node-editor').length===2"),'draft');
  await textClick('角色与能力');await until(()=>evaluate("document.querySelectorAll('.orch-node-editor').length===6"),'roles');
  checks.four_providers=await evaluate("JSON.stringify([...document.querySelector('.orch-node-editor .orch-form-row:nth-of-type(2) select').options].map(o=>o.value))===JSON.stringify(['codex','claude-code','omp','opencode'])");
  await evaluate("(()=>{const e=document.querySelectorAll('.orch-node-editor')[2].querySelector('.orch-form-row:nth-of-type(2) select');e.value='codex';e.dispatchEvent(new Event('change',{bubbles:true}))})()");
  await until(()=>evaluate("!!document.querySelectorAll('.orch-node-editor')[2].querySelector('select[aria-label=执行位置]')"),'remote role selector');
  await until(()=>evaluate("document.querySelectorAll('.orch-node-editor')[2].querySelector('select[aria-label=执行位置]').options.length===2"),'SSH profile list');
  await evaluate("(()=>{const e=document.querySelectorAll('.orch-node-editor')[2].querySelector('select[aria-label=执行位置]');e.value='b0cfb946-d724-43d6-87e1-1530b16f50ae';e.dispatchEvent(new Event('change',{bubbles:true}))})()");
  await until(()=>evaluate("document.querySelectorAll('.orch-node-editor')[2].textContent.includes('支持实现与探索角色')"),'remote role configured with scoped support');
  await fill('.orch-node-editor input','协调者');
  await click('.orch-node-editor > details:nth-of-type(3) > summary');
  await fill('input[aria-label="MCP 目录文件路径"]','D:/fixture/mcp-catalog.json');await textClick('载入目录预览',"document.querySelector('.orch-mcp-source')");
  await until(()=>evaluate("!!document.querySelector('.orch-mcp-source .orch-report')"),'catalog preview');
  checks.mcp_preview_no_confirmation=await evaluate("!orchestrationFixture.requests.some(r=>r.cmd==='orchestration.mcp.confirm')&&document.querySelector('.orch-mcp-source .orch-report').innerText.includes('sha256:')");
  checks.mcp_no_default_tools=await evaluate("[...document.querySelector('.orch-mcp-source').querySelectorAll('button')].find(b=>b.textContent==='确认来源并添加所选工具').disabled");
  await click('.orch-mcp-source input[type="checkbox"]');await evaluate('orchestrationFixture.driftSource()');
  await textClick('确认来源并添加所选工具',"document.querySelector('.orch-mcp-source')");
  await until(()=>evaluate("document.querySelector('.orch-mcp-source [role=alert]')?.textContent.includes('来源已变化')"),'source drift rejected');
  checks.mcp_stale_preview_rejected=await evaluate("!document.querySelector('.orch-node-editor .orch-node-editor')");
  await textClick('载入目录预览',"document.querySelector('.orch-mcp-source')");
  await until(()=>evaluate("document.querySelector('.orch-mcp-source .orch-fingerprint')?.textContent.includes('sha256:bbbb')"),'reloaded source');
  await click('.orch-mcp-source input[type="checkbox"]');await textClick('确认来源并添加所选工具',"document.querySelector('.orch-mcp-source')");
  await until(()=>evaluate("!!document.querySelector('.orch-node-editor .orch-node-editor')"),'source added');
  checks.mcp_bound_fields_readonly=await evaluate("[...document.querySelector('.orch-node-editor .orch-node-editor').querySelectorAll('input,textarea')].filter(e=>e.readOnly).length===4");
  checks.unsaved_disables_confirm=await evaluate("[...document.querySelectorAll('button')].find(e=>e.textContent==='确认并启动').disabled");
  await textClick('保存方案');await until(()=>evaluate("![...document.querySelectorAll('button')].find(e=>e.textContent==='确认并启动').disabled"),'saved');
  checks.mcp_source_saved=await evaluate("(()=>{const server=orchestrationFixture.requests.find(r=>r.cmd==='orchestration.plan.save').plan.roles[0].mcp_servers[0];return server.source.path==='D:/fixture/mcp-catalog.json'&&server.source.digest==='sha256:'+'b'.repeat(64)&&server.tools.join(',')==='read'})()");
  checks.remote_binding_saved=await evaluate("(()=>{const r=orchestrationFixture.requests.find(r=>r.cmd==='orchestration.plan.save').plan.roles[2].remote_codex;return r.ssh_profile_id==='b0cfb946-d724-43d6-87e1-1530b16f50ae'&&r.ssh_profile_fingerprint==='sha256:'+'d'.repeat(64)&&r.ssh_target_fingerprint==='sha256:'+'e'.repeat(64)&&r.remote_model==='deepseek-flash'&&r.provider_fingerprint==='sha256:'+'f'.repeat(64)&&document.body.innerText.includes('Codex / deepseek-flash')&&Array.from(document.querySelectorAll('input')).some(i=>i.placeholder==='留空使用远端模型 deepseek-flash')})()");
  await evaluate("(()=>{const e=document.querySelectorAll('select[aria-label=执行位置]')[1];e.value='';e.dispatchEvent(new Event('change',{bubbles:true}))})()");
  await textClick('保存方案');await until(()=>evaluate("![...document.querySelectorAll('button')].find(e=>e.textContent==='确认并启动').disabled"),'local-only plan saved');
  await fill('input[aria-label="模板名称"]','可复用流程');await textClick('保存为模板');
  await until(()=>evaluate("orchestrationFixture.requests.some(r=>r.cmd==='orchestration.templates.save')"),'template saved');
  await shot('roles-light');
  await textClick('目标与流程');await textClick('确认并启动');await until(()=>evaluate("!!document.querySelector('dialog[open]')"),'confirm');
  checks.confirmation_is_explicit=await evaluate("!orchestrationFixture.requests.some(r=>r.cmd==='orchestration.plan.confirm')");
  await textClick('确认',"document.querySelector('dialog[open]')");await until(()=>evaluate("document.querySelectorAll('.orch-graph-node').length===4"),'graph');
  checks.graph=true;checks.edges=await evaluate("document.querySelectorAll('.orch-edge').length===4");
  await evaluate("orchestrationFixture.usage()");
  await click('.orch-report > summary');
  await until(()=>evaluate("document.body.innerText.includes('omp.getContextUsage')"),'native usage receipt rendered');
  checks.context_native_estimate=await evaluate("document.body.innerText.includes('原生估算')&&document.body.innerText.includes('40.0%')");
  checks.context_unknown_and_threshold=await evaluate("document.body.innerText.includes('上下文未知')&&document.body.innerText.includes('已达到交接阈值')");
  await evaluate('orchestrationFixture.stage()');await until(()=>evaluate("!!document.querySelector('.orch-accept textarea')"),'human gate');
  await fill('.orch-accept textarea','阶段成果已核对');await textClick('确认阶段成果');
  await until(()=>evaluate("!document.querySelector('.orch-accept textarea')"),'gate cleared');checks.human_gate=true;
  await textClick('目标与流程');await fill('.orch-plan>label textarea','批准后的完整验收标准');await fill('input[aria-label="变更原因"]','补全总体验收说明');
  await textClick('提交变更提议');await until(()=>evaluate("[...document.querySelectorAll('button')].some(e=>e.textContent==='批准变更')"),'proposal');
  await textClick('批准变更');await until(()=>evaluate("!document.body.innerText.includes('有未保存修改')&&document.querySelector('.orch-plan>label textarea')?.value==='批准后的完整验收标准'"),'approved draft reloaded');checks.revision_reload=true;
  await textClick('暂停派发');await until(()=>evaluate("document.querySelector('.orch-run-heading').textContent.includes('已暂停派发')"),'paused');
  await textClick('继续核对');await until(()=>evaluate("document.querySelector('.orch-run-heading').textContent.includes('执行中')"),'resumed');checks.pause_resume=true;
  await textClick('执行与决定');await shot('execution-light');
  await textClick('视图');await textClick('切换深色主题');await shot('execution-dark');
  for(const width of [1440,1100,900]){await cdp('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await delay(100);checks['fit_'+width]=await evaluate('document.documentElement.scrollWidth<=innerWidth');}
  await evaluate('orchestrationFixture.finish()');await textClick('交付与证据');
  await until(()=>evaluate("document.querySelector('.orch-delivery').textContent.includes('证据已核对')"),'evidence stages');
  checks.evidence_details=await evaluate("document.querySelectorAll('.orch-evidence').length===4");
  checks.no_deployment_claim=await evaluate("document.querySelector('.orch-delivery').textContent.includes('部署：未执行')&&document.querySelector('.orch-delivery').textContent.includes('业务 UAT：未执行')");
  await click('.orch-evidence summary');await shot('delivery-dark');
  await fill('.orch-accept textarea','完整候选与逐项证据已核对');await textClick('确认总体验收');await until(()=>evaluate("document.querySelector('.orch-delivery h3').textContent==='交付已验收'"),'accepted');checks.acceptance=true;
  await click('details.orch-report summary');await fillAt('details.orch-report textarea',0,'修复已验收候选中的后续缺陷');await fillAt('details.orch-report textarea',1,'后续目标通过且保留前任候选');await fillAt('details.orch-report textarea',2,'可复现：fixture 在验收后仍能观察到该缺陷');await textClick('创建继任协作');await until(()=>evaluate("document.querySelector('.orch-run-heading h2')?.textContent==='修复已验收候选中的后续缺陷'"),'reopen');checks.controlled_reopen=await evaluate("orchestrationFixture.requests.some(r=>r.cmd==='orchestration.reopen')&&document.body.innerText.includes('继任候选')");
  await textClick('新建协作');await until(()=>evaluate("!!document.querySelector('.orch-create select')"),'second create');
  checks.template_available=await evaluate("document.querySelector('.orch-create label:nth-child(5) select').options.length===2");
  await key('Escape');await textClick('旧版协作');await until(()=>evaluate("[...document.querySelectorAll('button')].some(e=>e.textContent==='返回新版编排')"),'legacy');
  await textClick('返回新版编排');await until(()=>evaluate("[...document.querySelectorAll('button')].some(e=>e.textContent==='旧版协作')"),'back to v2');checks.legacy_return=true;
  await until(()=>evaluate("document.querySelector('.orch-run-heading h2')?.textContent==='修复已验收候选中的后续缺陷'"),'selected workflow restored');checks.selection_persists=true;
  await evaluate('orchestrationFixture.usage()');
  await until(()=>evaluate("[...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='打开 Oh My Pi (OMP) 会话')"),'pre-enrollment session shortcut');
  checks.pre_enrollment_session=await evaluate("document.querySelector('[aria-label=\"当前协作与会话\"]').textContent.includes('打开 Oh My Pi (OMP) 会话')");
  await textClick('打开 Oh My Pi (OMP) 会话');await until(()=>evaluate("!!document.querySelector('.orch-session') && !!document.querySelector('.orch-page')"),'inline session panel');checks.inline_session_panel=true;
  await textClick('在工作台展开');await until(()=>evaluate("!document.querySelector('.orch-page') && !!document.querySelector('.wb-content')"),'session navigation');checks.session_navigation=true;
  await click('button[aria-label="返回项目首页"]');
  await until(()=>evaluate("!!document.querySelector('.ide-nav[aria-label=\"Agent 编排\"]')"),'return navigation');
  await click('.ide-nav[aria-label="Agent 编排"]');await until(()=>evaluate("document.querySelector('.orch-run-heading h2')?.textContent==='修复已验收候选中的后续缺陷'"),'return from workbench');
  await delay(1700);checks.no_poll_focus_steal=await evaluate("!!document.querySelector('.orch-page')");
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
    status:error?'failed':'passed',scope:'Actual A WorkspaceApp and V2 orchestration console with isolated in-memory RPC; no real models or runtime',
    checks,snapshots,exceptions,escapedRpc,temporaryProfileRemoved:true,
    error:error?String(error):null,
  },null,2)+'\n');
}
if(error)process.exitCode=1;
else console.log(`Passed ${Object.keys(checks).length} browser assertions; evidence: ${output}`);
