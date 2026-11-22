import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdir} from 'node:fs/promises';

const {chromium}=createRequire(import.meta.url)('playwright');
let server,browser,url;
before(async()=>{
 const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {AppearanceProvider} from '/src/Appearance';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';createRoot(document.getElementById('root')).render(<React.StrictMode><AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider></React.StrictMode>);`;
 const built=process.env.VELELLA_BROWSER_BUILD;
 server=await createServer({root:built||fileURLToPath(new URL('..',import.meta.url)),cacheDir:fileURLToPath(new URL('../../artifacts/terminal-history-separation-20261106/vite-test-cache',import.meta.url)),configFile:false,logLevel:'error',plugins:built?[]:[react(),{name:'terminal-history-fixture',resolveId:id=>id==='/__history.tsx'?id:null,load:id=>id==='/__history.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url!='/__history')return next();void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__history.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)})})}}],server:{host:'127.0.0.1',port:0}});
 await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}/${built?'':'__history'}`;browser=await chromium.launch({channel:'msedge',headless:true});
});
after(async()=>{await browser?.close();await server?.close()});

const session=(id,program='powershell',state='host_lost',task_id=null)=>({id,title:program==='powershell'?'PowerShell':program==='codex'?'Codex':program,program,kind:'terminal',task_id,native_id:null,auto_recoverable:state==='host_lost',recoverable:state==='host_lost'||state==='done',status:{pid:42,exit:state==='done'?0:null,state,input_protocol:state==='host_lost'?0:1}});
const terminalTab=(id,title='PowerShell')=>({id:'tab-'+id,target:id,kind:'terminal',title});
async function fixture({sessions=[],tabs=[],failStop=false},run){
 const context=await browser.newContext(),requests=[],errors=[];
 let layout={revision:1,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:tabs[0]?.id||null,tabs}]};
 await context.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:ui:theme',JSON.stringify('light'));localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));});
 await context.route('**/_velella',async route=>{
  const q=route.request().postDataJSON();requests.push(q);let data={ok:true};
  switch(q.cmd){
   case 'workbench.snapshot':Object.assign(data,{projects:[{id:'p',name:'Knowledge fixture',path:'/fixture',direct:true}],workspaces:[{id:'w',project_id:'p',name:'Knowledge fixture',path:'/fixture',kind:'folder',archived:false,favorite:false,position:0}],tasks:[],source_links:[]});break;
   case 'workbench.layout':Object.assign(data,{layout,split_layout_version:1});break;
   case 'workbench.layout.save':layout={...q.layout,revision:layout.revision+1};data.layout=layout;break;
   case 'workbench.sessions':data.sessions=sessions;break;
   case 'workbench.terminal.open':{const created=session('created-shell-'+sessions.length,'powershell','waiting_input');created.title='PowerShell created';sessions.push(created);data.session_id=created.id;break;}
   case 'workbench.files':data.files=[];break;
   case 'workbench.git':Object.assign(data,{branch:'',status:''});break;
   case 'workbench.terminal.output':{const text=Buffer.from('SAVED_RECORD_'+q.session_id+'\r\n'),from=Math.min(q.since||0,text.length);Object.assign(data,{from,end:text.length,total:text.length,data:text.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:0}]:[]});break;}
   case 'workbench.terminal.stop':if(failStop)data={ok:false,error:'STOP_FIXTURE_FAILED'};else{const s=sessions.find(s=>s.id===q.session_id);s.status={...s.status,exit:0,state:'stopped'};s.auto_recoverable=false}break;
   case 'workbench.terminal.recover':{const s=sessions.find(s=>s.id===q.session_id);s.status={...s.status,exit:null,state:'waiting_input',input_protocol:1,pid:43};s.auto_recoverable=false;break;}
   case 'workbench.terminal.activity':Object.assign(data,{enabled:false,events:[],next:0,total:0});break;
   case 'workbench.terminal.resize':case 'workbench.terminal.control':case 'workbench.terminal.input':break;
   default:throw Error('Unexpected RPC '+q.cmd);
  }
  await route.fulfill({json:data});
 });
 const page=await context.newPage();page.setDefaultTimeout(15000);page.on('pageerror',e=>errors.push(String(e)));
 try{await page.goto(url);await page.getByRole('button',{name:'会话记录',exact:true}).waitFor();await run({page,requests,layout:()=>layout,sessions});assert.deepEqual(errors,[])}catch(error){console.error(JSON.stringify({errors,body:await page.locator('body').innerText(),requests:requests.slice(-18),layout}));throw error}finally{await context.close()}
}

test('saved records are categorized and history replay neither opens tabs nor restarts or controls a terminal',async()=>{
 const records=[...Array.from({length:19},(_,i)=>session('shell-'+i)),...Array.from({length:9},(_,i)=>session('codex-'+i,'codex')),...Array.from({length:2},(_,i)=>session('claude-'+i,'claude')),...Array.from({length:2},(_,i)=>session('zcode-'+i,'zcode-free'))];
 await fixture({sessions:records},async({page,requests,layout})=>{
  await page.locator('.workspace-start[data-history-count="32"][data-open-terminals="0"]').waitFor();assert.equal(await page.locator('.workspace-start-session').filter({hasText:'PowerShell'}).count(),0);
  await page.getByRole('button',{name:'会话记录',exact:true}).click();const dialog=page.getByRole('dialog',{name:'会话记录',exact:true});await dialog.getByRole('button',{name:'Agent 会话（13）',exact:true}).waitFor();assert.equal(await dialog.locator('.history-session-row').count(),13);
  await dialog.locator('[data-session-id="codex-8"]').click();await dialog.locator('.terminal-canvas[data-terminal-offset]').waitFor();assert.equal(layout().panes[0].tabs.length,0);
  assert.equal(requests.filter(q=>/terminal\.(open|recover|input|resize|control)$/.test(q.cmd)).length,0);
  await dialog.getByRole('button',{name:'终端记录（19）',exact:true}).click();assert.equal(await dialog.locator('.history-session-row').count(),19);assert.match(await dialog.innerText(),/已失联/);assert.doesNotMatch(await dialog.innerText(),/可恢复/);
 });
});

test('close cancellation keeps the terminal; confirmed close stops once before removing its tab and preserves history',async()=>fixture({sessions:[session('live','powershell','waiting_input')],tabs:[terminalTab('live')]},async({page,requests,layout})=>{
 const close=page.getByRole('button',{name:'关闭标签 PowerShell',exact:true});await close.click();let confirm=page.getByRole('dialog',{name:'确认操作',exact:true});await confirm.getByRole('button',{name:'取消',exact:true}).click();assert.equal(layout().panes[0].tabs.length,1);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.stop').length,0);
 await close.click();await confirm.getByRole('button',{name:'确认',exact:true}).click();await page.locator('.workspace-start[data-history-count="1"]').waitFor();assert.equal(layout().panes[0].tabs.length,0);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.stop').length,1);assert.equal(requests.find(q=>q.cmd==='workbench.terminal.stop').confirmed,true);
}));

test('a failed stop keeps its tab visible and reports the failure',async()=>fixture({sessions:[session('live','powershell','waiting_input')],tabs:[terminalTab('live')],failStop:true},async({page,requests,layout})=>{
 await page.getByRole('button',{name:'关闭标签 PowerShell',exact:true}).click();await page.getByRole('dialog',{name:'确认操作',exact:true}).getByRole('button',{name:'确认',exact:true}).click();await page.getByText('STOP_FIXTURE_FAILED',{exact:false}).waitFor();assert.equal(layout().panes[0].tabs.length,1);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.stop').length,1);await page.getByRole('tab',{name:'PowerShell',exact:true}).waitFor();
}));

test('closing one terminal split stops only that leaf',async()=>{
 const tab={...terminalTab('a'),terminalTree:{kind:'split',id:'split',axis:'horizontal',ratio:.5,first:{kind:'leaf',id:'a'},second:{kind:'leaf',id:'b'}}};
 await fixture({sessions:[session('a','powershell','waiting_input'),session('b','powershell','waiting_input')],tabs:[tab]},async({page,requests,layout})=>{
  await page.locator('[data-terminal-pane="b"] button[aria-label="关闭终端分屏"]').click();await page.getByRole('dialog',{name:'确认操作',exact:true}).getByRole('button',{name:'确认',exact:true}).click();await page.locator('[data-terminal-pane="b"]').waitFor({state:'detached'});assert.deepEqual(requests.filter(q=>q.cmd==='workbench.terminal.stop').map(q=>q.session_id),['b']);assert.equal(layout().panes[0].tabs[0].target,'a');
 });
});

test('managed terminal close names the task-wide effect and can be cancelled',async()=>fixture({sessions:[session('managed','codex','waiting_input','task-owned')],tabs:[terminalTab('managed','Codex')]},async({page,requests,layout})=>{
 await page.getByRole('button',{name:'关闭标签 Codex',exact:true}).click();const confirm=page.getByRole('dialog',{name:'确认操作',exact:true});await confirm.getByText(/受管任务.*全部执行进程/).waitFor();await confirm.getByRole('button',{name:'取消',exact:true}).click();assert.equal(layout().panes[0].tabs.length,1);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.stop').length,0);
}));

test('resuming a history record uses the existing session identity exactly once',async()=>fixture({sessions:[session('original-codex','codex')]},async({page,requests,layout})=>{
 await page.getByRole('button',{name:'会话记录',exact:true}).click();const dialog=page.getByRole('dialog',{name:'会话记录',exact:true});await dialog.locator('[data-session-id="original-codex"]').click();await dialog.getByRole('button',{name:'继续会话',exact:true}).click();await page.getByRole('tab',{name:'Codex',exact:true}).waitFor();assert.equal(layout().panes[0].tabs[0].target,'original-codex');assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,1);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.open').length,0);
}));

test('the workspace strip spans the window below the app menu and stays available with no terminal',async()=>fixture({},async({page,requests})=>{
 await page.setViewportSize({width:1600,height:900});const strip=page.locator('.workspace-terminal-strip');await strip.getByText('暂无打开标签',{exact:true}).waitFor();
 const bounds=await page.evaluate(()=>{const rect=s=>{const r=document.querySelector(s).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,bottom:r.bottom}};return{strip:rect('.workspace-terminal-strip'),shell:rect('.ide-shell'),menu:rect('.app-titlebar'),sidebar:rect('.ide-sidebar'),main:rect('.ide-main')}});
 assert.ok(Math.abs(bounds.strip.width-(bounds.shell.width-2))<=2);assert.ok(bounds.strip.y>=bounds.menu.bottom-1);assert.ok(bounds.sidebar.y>=bounds.strip.bottom-1);assert.ok(bounds.main.y>=bounds.strip.bottom-1);
 assert.equal(await page.locator('.layout-project-column .layout-terminal-bar').count(),0);await strip.getByRole('button',{name:'新建标签',exact:true}).waitFor();assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.open').length,0);
 const dir=fileURLToPath(new URL('../../artifacts/terminal-tabstrip-20261106/screenshots/',import.meta.url));await mkdir(dir,{recursive:true});await page.screenshot({path:dir+(process.env.VELELLA_BROWSER_BUILD?'candidate':'source')+'-empty.png'});
}));

test('global tabs select the original sessions and the adjacent plus opens one new terminal',async()=>fixture({sessions:[session('a','powershell','waiting_input'),session('b','codex','waiting_input')],tabs:[terminalTab('a'),terminalTab('b','Codex')]},async({page,requests,layout})=>{
 const strip=page.locator('.workspace-terminal-strip');await strip.getByRole('tab',{name:'Codex',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.workspace-terminal-strip [role=tab][aria-selected=true]')?.textContent==='Codex');assert.equal(layout().panes[0].active,'tab-b');
 await strip.getByRole('button',{name:'新建标签',exact:true}).click();await page.getByRole('menuitem',{name:'新建终端',exact:true}).click();await strip.getByRole('tab',{name:'PowerShell created',exact:true}).waitFor();assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.open').length,1);assert.equal(layout().panes[0].tabs.length,3);
 const dir=fileURLToPath(new URL('../../artifacts/terminal-tabstrip-20261106/screenshots/',import.meta.url));await mkdir(dir,{recursive:true});await page.mouse.move(800,400);await page.getByRole('menu').waitFor({state:'detached'});await page.screenshot({animations:'disabled',path:dir+(process.env.VELELLA_BROWSER_BUILD?'candidate':'source')+'-tabs.png'});
}));

test('long terminal names scroll inside the global strip without clipping new-tab or history actions',async()=>{
 const records=Array.from({length:12},(_,i)=>({...session('wide-'+i,'powershell','waiting_input'),title:'终端 '+i+' · 很长的项目会话名称与执行说明'}));
 await fixture({sessions:records,tabs:records.map(s=>terminalTab(s.id,s.title))},async({page})=>{
  await page.setViewportSize({width:960,height:720});const strip=page.locator('.workspace-terminal-strip');await strip.getByRole('tab').last().scrollIntoViewIfNeeded();const plus=await strip.getByRole('button',{name:'新建标签',exact:true}).boundingBox(),history=await strip.getByRole('button',{name:'会话记录',exact:true}).boundingBox();
  const dir=fileURLToPath(new URL('../../artifacts/terminal-tabstrip-20261106/screenshots/',import.meta.url));await mkdir(dir,{recursive:true});await page.screenshot({path:dir+(process.env.VELELLA_BROWSER_BUILD?'candidate':'source')+'-narrow.png'});
  const metrics=await page.evaluate(()=>Object.fromEntries(['.workspace-terminal-strip','.shell-page-chrome','.workspace-tab-navigation','.ide-tabs','.project-tools'].map(s=>{const e=document.querySelector(s),r=e.getBoundingClientRect(),c=getComputedStyle(e);return[s,{x:r.x,width:r.width,scrollWidth:e.scrollWidth,clientWidth:e.clientWidth,scrollLeft:e.scrollLeft,flex:c.flex,minWidth:c.minWidth}]})));
  assert.ok(plus&&history&&plus.x+plus.width<=history.x&&history.x+history.width<=960,JSON.stringify({plus,history,metrics}));assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);assert.equal(await strip.locator('.ide-tabs').evaluate(e=>e.scrollWidth>e.clientWidth&&e.scrollLeft>0),true);
 });
});
