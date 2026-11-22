import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
let browser,server,url;
before(async()=>{
 const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {TooltipProvider} from '/src/ui';import {AppearanceProvider} from '/src/Appearance';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.terms=[];window.addEventListener('velella:terminal-created',e=>window.terms.push(e.detail.terminal));createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);`;
 server=await createServer({root,configFile:false,cacheDir:root+'/.vite-native-restore',logLevel:'error',plugins:[react(),{name:'native-restore',resolveId:id=>id==='/__native_restore.tsx'?id:null,load:id=>id==='/__native_restore.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__native_restore'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__native_restore.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}}],server:{host:'127.0.0.1',port:0}});
 await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}/__native_restore`;browser=await chromium.launch({channel:'msedge',headless:true});
});
after(async()=>{await browser?.close();await server?.close()});
for(const program of ['codex','claude','gemini','opencode','omp','kimi','zcode-free','mcode'])for(const scenario of (program==='zcode-free'?['dead','live','legacy','legacy-stopped','stopped','stop-reopen','stopped-failure']:program==='codex'?['dead','live','twice','stopped','stop-reopen','stopped-failure']:program==='claude'?['dead','live','stopped','stop-reopen']:['dead','live','legacy','legacy-stopped','stopped','stop-reopen']))test(`${program}: opening ${scenario} saved terminal restores directly without a history/restart action`,async()=>{
 const live=scenario==='live'||scenario==='stop-reopen',legacy=scenario.startsWith('legacy'),stopped=scenario.startsWith('stopped')||scenario==='legacy-stopped',failure=scenario==='stopped-failure';
 const context=await browser.newContext(),page=await context.newPage(),requests=[];
 const native=program==='zcode-free'?'sess_original':program==='opencode'?'ses_original':program==='kimi'?'session_d8fdd138-475c-43c0-9f4d-722ec8a48741':'d8fdd138-475c-43c0-9f4d-722ec8a48741';
 const providerLabel=({'zcode-free':'ZCode',mcode:'MiniMax Code',gemini:'Gemini CLI',opencode:'OpenCode',omp:'Oh My Pi (OMP)',kimi:'Kimi'})[program];
 const session={id:'original-terminal',title:program,kind:'terminal',program,task_id:null,native_id:legacy?null:native,restore_selection_required:legacy,recoverable:!live,auto_recoverable:!live&&!legacy&&!stopped,open_recoverable:!live&&!legacy&&stopped,status:{pid:live?5678:1234,exit:stopped?1:null,state:live?'waiting_input':stopped?'stopped':'host_lost',input_protocol:live?1:0}};
 let output=Buffer.from('ORIGINAL_QUESTION\r\nORIGINAL_ANSWER\r\n');
 await page.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}))});
 await page.route('**/_velella',async route=>{
  const q=route.request().postDataJSON();requests.push(q);let data={ok:true};
  if(q.cmd==='workbench.snapshot')Object.assign(data,{projects:[{id:'p',name:'Restore fixture',path:'/fixture',direct:true}],workspaces:[{id:'w',project_id:'p',name:'Restore fixture',path:'/fixture',kind:'folder',archived:false,favorite:false,position:0}],tasks:[],source_links:[]});
  if(q.cmd==='workbench.sessions')data.sessions=[session];
  if(q.cmd==='workbench.layout')Object.assign(data,{layout:{revision:0,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:'tab',tabs:[{id:'tab',kind:'terminal',target:session.id,title:program},...(scenario==='stop-reopen'?[{id:'file',kind:'files',target:'notes.txt',title:'notes.txt'}]:[])]}]},split_layout_version:1});
  if(q.cmd==='workbench.layout.save')data.layout=q.layout;
  if(q.cmd==='workbench.files')data.files=[];
  if(q.cmd==='workbench.git')Object.assign(data,{branch:'',status:''});
  if(q.cmd==='workbench.terminal.activity')Object.assign(data,{enabled:false,events:[],next:0,total:0});
  if(q.cmd==='workbench.terminal.restore.list')data.sessions=[{id:'sess_other',title:'Other conversation',created_at:2},{id:native,title:'Original conversation',created_at:1}];
  if(q.cmd==='workbench.terminal.restore.select'){assert.equal(q.confirmed,true);assert.equal(q.native_id,native);session.native_id=native;session.restore_selection_required=false;session.auto_recoverable=!stopped;session.open_recoverable=stopped;}
  if(q.cmd==='workbench.terminal.recover'){
   assert.equal(q.automatic,true);assert.equal(q.session_id,session.id);if(stopped||scenario==='stop-reopen')assert.equal(q.on_open,true);session.auto_recoverable=false;session.open_recoverable=false;
   if(failure)data={ok:false,error:'fixture native restore failed'};
   else{session.recoverable=false;session.status={pid:session.status.pid===1234?5678:6789,exit:null,state:'waiting_input',input_protocol:1};output=Buffer.concat([output,Buffer.from('SAME_SESSION_READY\r\n')]);}
  }
  if(q.cmd==='workbench.terminal.output'){const from=Math.min(q.since??0,output.length);Object.assign(data,{from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,rows:30,cols:100,sequence:0}]:[]})}
  await route.fulfill({json:data});
 });
 try{
  await page.goto(url);
  if(legacy){await page.getByRole('dialog',{name:`首次还原 ${providerLabel} 会话`}).waitFor();assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,0);await page.getByRole('button').filter({hasText:'Original conversation'}).click();}
  if(failure){await page.getByRole('status').filter({hasText:'自动恢复未完成'}).waitFor();await page.waitForTimeout(2300);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,1);assert.equal(await page.evaluate(()=>window.terms.every(t=>t.options.disableStdin)),true);return;}
  const usable=()=>page.waitForFunction(()=>{const t=window.terms.at(-1);return t?.element?.isConnected&&!t.options.disableStdin&&Array.from({length:t.buffer.active.length},(_,i)=>t.buffer.active.getLine(i)?.translateToString(true)).join('\n').includes('ORIGINAL_ANSWER')});
  await usable();
  if(scenario==='stop-reopen'){
   session.status={pid:5678,exit:1,state:'stopped',input_protocol:1};session.recoverable=true;session.auto_recoverable=false;session.open_recoverable=true;
   await page.getByRole('status').filter({hasText:'进程已结束'}).waitFor();await page.waitForTimeout(2300);
   assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,0,'a stop must remain stopped while polling the same visible pane');
   await page.getByRole('tab',{name:'notes.txt',exact:true}).click();await page.getByRole('tab',{name:'notes.txt',exact:true}).getAttribute('aria-selected').then(value=>assert.equal(value,'true'));
   await page.getByRole('tab',{name:program,exact:true}).click();await usable();
   await page.waitForFunction(()=>window.terms.some(t=>!t.options.disableStdin));
  }
  assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,live&&scenario!=='stop-reopen'?0:1);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.open').length,0);
  assert.equal(await page.getByRole('status').count(),0,'restored session must be directly usable without a restored-history banner');
  assert.equal(await page.getByRole('button',{name:'重新启动终端',exact:true}).count(),0);
  const text=await page.evaluate(()=>{const t=window.terms.at(-1);return Array.from({length:t.buffer.active.length},(_,i)=>t.buffer.active.getLine(i)?.translateToString(true)).join('\n')});
  assert.match(text,/ORIGINAL_QUESTION/);assert.match(text,/ORIGINAL_ANSWER/);
  if(scenario==='twice'){
   session.status={pid:5678,exit:null,state:'host_lost',input_protocol:0};session.auto_recoverable=true;session.recoverable=true;
   const deadline=Date.now()+6000;while(requests.filter(q=>q.cmd==='workbench.terminal.recover').length<2&&Date.now()<deadline)await new Promise(r=>setTimeout(r,50));
   assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,2,'a later host loss in the retained pane must restore the new host generation');await usable();
  }
  await page.locator('.xterm-helper-textarea').focus();await page.keyboard.type('continue original');await page.keyboard.press('Enter');
  const inputDeadline=Date.now()+5000;while(requests.filter(q=>q.cmd==='workbench.terminal.input').map(q=>q.text).join('')!=='continue original\r'&&Date.now()<inputDeadline)await page.waitForTimeout(25);
  const inputs=requests.filter(q=>q.cmd==='workbench.terminal.input');assert.equal(inputs.map(q=>q.text).join(''),'continue original\r');assert.ok(inputs.every(q=>q.expected_host_pid===(scenario==='twice'||scenario==='stop-reopen'?6789:5678)));assert.equal(session.native_id,native);
  if(legacy){await page.reload();await page.getByRole('tab',{name:program,exact:true}).waitFor();assert.equal(await page.getByRole('dialog',{name:`首次还原 ${providerLabel} 会话`}).count(),0);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.restore.select').length,1);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.restore.list').length,1);}
 }finally{await context.close()}
});
