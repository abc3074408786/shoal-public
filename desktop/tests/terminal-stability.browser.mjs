import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const outputDir=fileURLToPath(new URL('../../artifacts/terminal-stability/',import.meta.url));
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';
import {Terminal} from '@xterm/xterm';import WorkspaceApp from '/src/WorkspaceApp';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';
import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';
window.terminals=[];window.created=0;window.disposed=0;
window.addEventListener('velella:terminal-created',event=>{event.detail.terminal.element.dataset.testSession=event.detail.sessionId});
const open=Terminal.prototype.open,dispose=Terminal.prototype.dispose;
Terminal.prototype.open=function(el){open.call(this,el);window.terminals.push(this);window.created++;};
Terminal.prototype.dispose=function(){window.disposed++;return dispose.call(this)};
createRoot(document.getElementById('root')).render(<TooltipProvider>{location.search.includes('throughput')?<TerminalView workspace={{projectId:'p',workspaceId:'w',id:'s',status:{exit:null,input_protocol:1}}}/>:<WorkspaceApp/>}</TooltipProvider>);`;

async function fixture(run){
 const server=await createServer({root,configFile:false,cacheDir:join(root,'node_modules/.vite-terminal-stability'),logLevel:'error',plugins:[react(),{
  name:'terminal-stability-fixture',resolveId:id=>id==='/__stability.tsx'?id:null,load:id=>id==='/__stability.tsx'?entry:null,
  configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url?.startsWith('/__stability?')){void s.transformIndexHtml(req.url,'<html><body><div id="root" style="height:800px;width:1200px"></div><script type="module" src="/__stability.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}
 }],server:{host:'127.0.0.1',port:0}});
 let browser;
 try{await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1280,height:900}});
  await page.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false}));});
  const failures=[];page.on('pageerror',error=>failures.push(error.message));page.on('requestfailed',request=>failures.push(request.url()+': '+request.failure()?.errorText));page.on('response',response=>{if(response.status()>=400)failures.push(response.status()+' '+response.url())});
  try{await run(page,`http://127.0.0.1:${server.httpServer.address().port}/__stability?`)}catch(error){console.log('fixture diagnostics',JSON.stringify({failures,url:page.url(),body:(await page.locator('body').innerText()).slice(0,2000)}));throw error}
 }finally{await browser?.close();await server.close()}
}
function receipt(name,value){mkdirSync(outputDir,{recursive:true});writeFileSync(outputDir+`${process.env.STABILITY_LABEL||'current'}-${name}.json`,JSON.stringify(value,null,2));}

test('workspace tabs share the native titlebar and retain controls on narrow windows',async()=>fixture(async(page,url)=>{
 await page.addInitScript(()=>{window.velellaDesktop={invoke:async(command,args)=>{if(command==='window')return false;if(command==='rpc')return fetch('/_velella',{method:'POST',body:JSON.stringify(args.request)}).then(r=>r.json());throw Error('Unexpected native fixture command '+command)},listen:()=>()=>{}}});
 const workspace={id:'w',project_id:'p',name:'Workspace',kind:'worktree',path:'fixture',archived:false,favorite:false,position:0};
 const sessions=Array.from({length:5},(_,i)=>({id:'s'+i,title:'Terminal '+i,kind:'terminal',program:'powershell',status:{pid:100+i,exit:null,input_protocol:1}}));
 let layout={revision:1,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:'t0',tabs:sessions.map((s,i)=>({id:'t'+i,target:s.id,kind:'terminal',title:s.title}))}]};
 const requests=[],output=Buffer.from('TERMINAL_READY\r\n');
 await page.route('**/_velella',async route=>{const q=route.request().postDataJSON();requests.push(q);let data={ok:true};
  if(q.cmd==='workbench.snapshot')Object.assign(data,{projects:[{id:'p',name:'Project',path:'fixture'}],workspaces:[workspace],tasks:[]});
  if(q.cmd==='workbench.layout')data.layout=layout;
  if(q.cmd==='workbench.layout.save'){layout={...q.layout,revision:layout.revision+1};data.layout=layout;}
  if(q.cmd==='workbench.sessions')data.sessions=sessions;
  if(q.cmd==='workbench.files')data.files=[];
  if(q.cmd==='workbench.terminal.output'){const from=q.since??0;Object.assign(data,{from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:0}]:[]});}
  await route.fulfill({json:data});
 });
 await page.goto(url+'topbar');await page.getByRole('tab',{name:'Terminal 0',exact:true}).waitFor();
 await page.waitForFunction(()=>document.querySelectorAll('.terminal-toolbar-actions').length===1);
 const metrics=[];mkdirSync(outputDir,{recursive:true});
 for(const width of [1440,1100,900]){
  await page.setViewportSize({width,height:900});await page.locator('#root').evaluate((e,width)=>e.style.width=width+'px',width);
  const m=await page.evaluate(()=>{const header=document.querySelector('.app-titlebar'),tabs=document.querySelector('.workbench-tabbar'),controls=document.querySelector('.window-controls'),tab=document.querySelector('.ide-tab'),main=document.querySelector('.ide-main');const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height}};return {header:rect(header),tabs:rect(tabs),controls:rect(controls),tab:rect(tab),main:rect(main),parentIsTitlebar:tabs.parentElement===header,noSecondRow:!document.querySelector('.ide-main>.workbench-tabbar'),tabDrag:getComputedStyle(tab).webkitAppRegion,blankTabstripDrag:getComputedStyle(document.querySelector('.app-titlebar .ide-tabs')).webkitAppRegion,controlDrag:getComputedStyle(controls).webkitAppRegion,dragWidth:document.querySelector('.workbench-window-drag')?.getBoundingClientRect().width??0}});
  metrics.push({width,...m});assert.equal(m.parentIsTitlebar,true);assert.equal(m.noSecondRow,true);assert.ok(m.tabs.y>=m.header.y&&m.tabs.bottom<=m.header.bottom+1);assert.ok(m.tab.width>=96);assert.ok(m.controls.right<=width+1);assert.ok(m.tabs.right<=m.controls.x);assert.ok(m.dragWidth>=24);assert.equal(m.tabDrag,'no-drag');assert.equal(m.blankTabstripDrag,'drag','the empty tabstrip must move the window');assert.equal(m.controlDrag,'no-drag');
  await page.screenshot({path:join(outputDir,`titlebar-${width}.png`)});
 }
 await page.getByRole('tab',{name:'Terminal 1',exact:true}).click();await page.waitForFunction(()=>document.querySelector('[role=tab][title="Terminal 1"]').getAttribute('aria-selected')==='true');
 await page.getByRole('button',{name:'收起项目栏',exact:true}).click();await page.getByRole('button',{name:'展开项目栏',exact:true}).click();
 await page.getByRole('button',{name:'新建标签',exact:true}).click();await page.getByRole('menu').waitFor();await page.keyboard.press('Escape');
 await page.getByRole('button',{name:'布局与标签',exact:true}).click();await page.getByRole('menuitem',{name:'分屏',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.wb-pane').length===2);assert.equal(await page.locator('.wb-tabbar').count(),2);
 await page.getByRole('button',{name:'布局与标签',exact:true}).click();await page.getByRole('menuitem',{name:'合并分屏',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.wb-pane').length===1);
 assert.equal(requests.some(q=>/\.(stop|close|recover|open)$/.test(q.cmd)),false);
 receipt('titlebar',{passed:true,metrics,tabSwitch:true,sidebarToggle:true,menus:true,splitMerge:true,noSessionRestart:true});
}));

test('16 MiB terminal burst drains within five seconds without dropping bytes',async()=>fixture(async(page,url)=>{
 const output=Buffer.concat([Buffer.from('\x1b[0m'.repeat(4*1024*1024)),Buffer.from('BURST_COMPLETE')]);
 let from=0,started=0,reads=0,maxInFlight=0,inFlight=0;
 await page.route('**/_velella',async route=>{const req=route.request().postDataJSON();let data={ok:true};if(req.cmd.endsWith('.output')){
  started ||= performance.now();inFlight++;maxInFlight=Math.max(maxInFlight,inFlight);from=req.since;const end=Math.min(from+128*1024,output.length);reads++;
  data={ok:true,from,end,total:output.length,data:output.subarray(from,end).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:1}]:[]};
  await route.fulfill({json:data});inFlight--;return;
 }await route.fulfill({json:data})});
 await page.goto(url+'throughput');
 let caught=false;try{await page.waitForFunction(()=>window.terminals.some(t=>t.buffer.active.getLine(t.buffer.active.cursorY+t.buffer.active.baseY)?.translateToString(true).includes('BURST_COMPLETE')),{},{timeout:5000});caught=true}catch{}
 const result={caught,elapsedMs:performance.now()-started,from,total:output.length,reads,maxInFlight};receipt('burst',result);
 assert.equal(caught,true,JSON.stringify(result));assert.equal(maxInFlight,1);
}));

test('streamed terminal chunks render promptly between idle reads',async()=>fixture(async(page,url)=>{
 await page.addInitScript(()=>{
  const probe=window.streamProbe={sent:[],latencies:[],reads:0,output:''};let started=false;
  window.addEventListener('velella:terminal-created',event=>{
   const term=event.detail.terminal;
   term.onWriteParsed(()=>{
    const b=term.buffer.active;let text='';
    for(let i=0;i<b.length;i++)text+=b.getLine(i)?.translateToString(true)+'\n';
    for(let i=probe.latencies.length;i<probe.sent.length;i++){
     if(!text.includes(`STREAM_${i}_END`))break;
     probe.latencies.push(performance.now()-probe.sent[i]);
    }
   });
  });
  window.velellaDesktop={listen:()=>()=>{},invoke:async(command,args)=>{
   if(command!=='rpc')return false;
   const q=args.request;if(!q.cmd.endsWith('.output'))return {ok:true};probe.reads++;
   if(!started){started=true;setTimeout(function produce(){
    const i=probe.sent.length;probe.sent.push(performance.now());probe.output+=`STREAM_${i}_END\r\n`;
    if(i<39)setTimeout(produce,80);
   },500)}
   const from=q.since??0,end=probe.output.length;
   return {ok:true,from,end,total:end,data:btoa(probe.output.slice(from)),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:1}]:[]};
  }};
 });
 await page.goto(url+'throughput-stream');
 await page.waitForFunction(()=>window.streamProbe?.latencies.length===40,{},{timeout:10000});
 const result=await page.evaluate(()=>{const p=window.streamProbe,l=p.latencies.slice(1).sort((a,b)=>a-b);return {chunks:p.latencies.length,reads:p.reads,p50:l[Math.floor(l.length*.5)],p95:l[Math.floor(l.length*.95)],max:l.at(-1)}});
 receipt('stream',result);console.log('stream latency',JSON.stringify(result));
 assert.ok(result.p95<40,`stream p95 latency ${result.p95} ms exceeds two frames plus timer allowance`);
}));

test('display history cleanup is reversible and persists across reopening without PTY input',async()=>fixture(async(page,url)=>{
 const output=Buffer.from('OLD_FRAME\r\n'.repeat(35)+'\x1b[H\x1b[0JCURRENT_FRAME\r\nCURRENT_INPUT');
 const commands=[];
 await page.route('**/_velella',async route=>{const q=route.request().postDataJSON();commands.push(q.cmd);const from=q.since??0;await route.fulfill({json:q.cmd.endsWith('.output')?{ok:true,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:8,sequence:0}]:[]}:{ok:true}})});
 await page.goto(url+'throughput');
 const active=()=>page.evaluate(()=>{const t=window.terminals.at(-1),b=t.buffer.active;return {baseY:b.baseY,screen:[0,1].map(i=>b.getLine(b.baseY+i).translateToString(true)),capacity:t.options.scrollback}});
 await page.waitForFunction(()=>window.terminals.at(-1)?.buffer.active.baseY>20);
 const before=await active();assert.deepEqual(before.screen,['CURRENT_FRAME','CURRENT_INPUT']);
 await page.locator('.terminal-canvas').click({button:'right',modifiers:['Control']});
 await page.getByRole('menuitem',{name:'清理显示历史',exact:true}).click();
 await page.waitForFunction(()=>window.terminals.at(-1)?.buffer.active.baseY===0);
 assert.deepEqual((await active()).screen,before.screen);
 await page.reload();
 await page.waitForFunction(()=>window.terminals.at(-1)?.buffer.active.getLine(0)?.translateToString(true)==='CURRENT_FRAME');
 assert.equal((await active()).baseY,0,'reopening does not bring fragments back');
 await page.locator('.terminal-canvas').click({button:'right',modifiers:['Control']});
 await page.getByRole('menuitem',{name:'恢复完整显示历史',exact:true}).click();
 await page.waitForFunction(()=>window.terminals.at(-1)?.buffer.active.baseY>20);
 assert.deepEqual((await active()).screen,before.screen);
 assert.equal(commands.some(c=>/\.(input|stop|close|control)$/.test(c)),false,'display actions never touch PTY input or lifecycle');
 receipt('display-history',{passed:true,reopen:true,restore:true,currentScreenPreserved:true,noPtyInput:true});
}));

test('real workspace tab, pane and workspace switches retain terminal instances and consumed offsets',async()=>fixture(async(page,url)=>{
 const workspaces=['w','other'].map(id=>({id,project_id:'p',name:id,kind:'worktree',path:'fixture/'+id,archived:false,favorite:false,position:0}));
 const sessions=Array.from({length:8},(_,i)=>({id:'s'+i,title:'terminal '+i,kind:'terminal',program:'powershell',status:{exit:null,input_protocol:1}}));
 const layouts=Object.fromEntries(workspaces.map(w=>[w.id,{revision:1,axis:'horizontal',ratio:.5,closed:[],panes:[0,1].map(p=>({id:'pane'+p,active:'t'+p*4,tabs:sessions.slice(p*4,p*4+4).map((s,i)=>({id:'t'+(p*4+i),target:w.id+s.id,kind:'terminal',title:s.title}))}))}]));
 const reads=[],commands=[],inputs=[];const output=Buffer.from(Array.from({length:200},(_,i)=>`history ${i}\r\n`).join(''));
 await page.route('**/_velella',async route=>{const r=route.request().postDataJSON();commands.push(r.cmd);let data={ok:true};
  if(r.cmd==='workbench.snapshot')data={...data,projects:[{id:'p',name:'fixture',path:'fixture'}],workspaces,tasks:[]};
  if(r.cmd==='workbench.layout')data.layout=layouts[r.workspace_id];
  if(r.cmd==='workbench.layout.save'){layouts[r.workspace_id]={...r.layout,revision:layouts[r.workspace_id].revision+1};data.layout=layouts[r.workspace_id]}
  if(r.cmd==='workbench.sessions')data.sessions=sessions.map(s=>({...s,id:r.workspace_id+s.id}));
  if(r.cmd==='workbench.files')data.files=[];
  if(r.cmd==='workbench.terminal.input')inputs.push(r);
  if(r.cmd==='workbench.terminal.output'){const from=r.since??0;reads.push({id:r.session_id,from});data={...data,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:80,rows:24,sequence:1}]:[]}}
  await route.fulfill({json:data});
 });
 await page.goto(url+'workspaces');await page.getByRole('tab',{name:'terminal 0',exact:true}).waitFor();
 await page.waitForFunction(()=>document.querySelectorAll('.wb-tabbar .terminal-toolbar-actions').length===2);
 for(const index of [0,1]){
  const pane=page.locator('.wb-pane').nth(index),other=page.locator('.wb-pane').nth(1-index);
  await pane.getByRole('button',{name:'搜索终端',exact:true}).click();
  await pane.getByRole('textbox',{name:'查找终端文本',exact:true}).waitFor();
  assert.equal(await other.getByRole('textbox',{name:'查找终端文本',exact:true}).count(),0,'pane controls act only on their own terminal');
  await pane.getByRole('button',{name:'关闭终端搜索',exact:true}).click();
 }
 for(let n=0;n<100;n++){const tab=page.getByRole('tab',{name:'terminal '+(n%8),exact:true});await tab.click();await page.waitForFunction(()=>!document.querySelector('[role="tab"][disabled]'));}
 const result=await page.evaluate(()=>({created:window.created,disposed:window.disposed}));result.zeroReads=reads.filter(r=>r.from===0).length;receipt('switch',result);
 assert.equal(result.created,8,JSON.stringify(result));assert.equal(result.disposed,0);assert.equal(result.zeroReads,8);
 assert.equal(await page.locator('.terminal-toolbar-actions').count(),2,'hidden sessions do not leave stale actions in pane tab bars');
 await page.getByRole('tab',{name:'terminal 0',exact:true}).click();
 await page.locator('.wb-pane').first().locator('.xterm[data-test-session="ws0"]').waitFor();
 const firstInput=page.waitForResponse(response=>{if(!response.url().endsWith('/_velella'))return false;const request=response.request().postDataJSON();return request.cmd==='workbench.terminal.input'&&request.session_id==='ws0'&&request.text==='a'});
 await page.locator('.wb-pane').first().locator('.xterm-helper-textarea').press('a');await firstInput;
 assert.ok(inputs.some(r=>r.session_id==='ws0'&&r.text==='a'),'the first key after revealing a cached terminal reaches its original session');
 await page.getByRole('button',{name:'other',exact:true}).click();await page.waitForFunction(()=>window.created===10);
 await page.getByRole('button',{name:'w',exact:true}).click();await page.getByRole('tab',{name:'terminal 0',exact:true}).click();
 await page.getByRole('button',{name:'移动 terminal 0 到另一分屏',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('[role="tab"][disabled]'));
 assert.deepEqual(await page.evaluate(()=>({created:window.created,disposed:window.disposed})),{created:10,disposed:0});
 const area=page.locator('.wb-pane').nth(1).locator('.xterm-helper-textarea');await area.focus();await page.waitForTimeout(2200);assert.equal(await area.evaluate(e=>e===document.activeElement),true,'status refresh preserves keyboard focus');
 await page.getByRole('button',{name:'关闭标签 terminal 0',exact:true}).click();await page.waitForFunction(()=>window.disposed===1);
 assert.equal(commands.some(c=>c.endsWith('.stop')||c.endsWith('.close')),false,'closing a view never stops the PTY');
}));
