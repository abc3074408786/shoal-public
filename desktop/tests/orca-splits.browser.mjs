import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdirSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const out=fileURLToPath(new URL('../../artifacts/orca-splits-20261025/browser/',import.meta.url));mkdirSync(out,{recursive:true});
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import {Terminal} from '@xterm/xterm';import WorkspaceApp from '/src/WorkspaceApp';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';import '/src/workbench-a.css';
window.created=[];window.disposed=0;window.addEventListener('velella:terminal-created',e=>{window.created.push(e.detail.sessionId);e.detail.terminal.element.dataset.testSession=e.detail.sessionId});const dispose=Terminal.prototype.dispose;Terminal.prototype.dispose=function(){window.disposed++;return dispose.call(this)};createRoot(document.getElementById('root')).render(<TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider>);`;
const leaves=t=>t.kind==='leaf'?[t.id]:[...leaves(t.first),...leaves(t.second)];
async function fixture(name,run){
 const server=await createServer({root,configFile:false,logLevel:'error',cacheDir:join(root,'node_modules/.vite-orca-splits'),plugins:[react(),{name:'split-fixture',resolveId:id=>id==='/__split.tsx'?id:null,load:id=>id==='/__split.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__split'){void s.transformIndexHtml(req.url,'<html><body><div id="root" style="height:100vh"></div><script type="module" src="/__split.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}}],server:{host:'127.0.0.1',port:0}});
 let browser,page;
 const workspace={id:'w',project_id:'p',name:'分屏交互验收',kind:'folder',path:'fixture',archived:false,favorite:false,position:0};
 const session=id=>({id,title:id==='s1'?'终端一':id==='s2'?'终端二':'新终端',kind:'terminal',program:'powershell',status:{pid:100,exit:null,input_protocol:1}});
 const state={layout:{revision:0,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:'t1',tabs:[{id:'t1',kind:'terminal',target:'s1',title:'终端一'},{id:'t2',kind:'terminal',target:'s2',title:'终端二'},{id:'file',kind:'files',target:'readme.md',title:'说明.md'},{id:'browser',kind:'browser',target:'browser-draft-fixture',title:'浏览器'}]}]},sessions:[session('s1'),session('s2')],requests:[],errors:[],failSave:false,opens:0};
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});page=await browser.newPage({viewport:{width:1440,height:1000}});
  page.on('pageerror',e=>state.errors.push(e.message));
  await page.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:ui:theme','dark');localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false}));});
  await page.route('**/_velella',async route=>{
   const q=route.request().postDataJSON();state.requests.push(q);let data={ok:true};
   if(q.cmd==='workbench.snapshot')Object.assign(data,{projects:[{id:'p',name:'测试项目',path:'fixture',direct:true}],workspaces:[workspace],tasks:[]});
   else if(q.cmd==='workbench.layout')Object.assign(data,{layout:state.layout,split_layout_version:1});
   else if(q.cmd==='workbench.layout.save'){
    if(state.failSave){state.failSave=false;data={ok:false,error:'模拟保存响应失败'};}else{assert.equal(q.layout.revision,state.layout.revision,'revision must be current');state.layout={...q.layout,revision:state.layout.revision+1};data.layout=state.layout;}
   }else if(q.cmd==='workbench.sessions')data.sessions=state.sessions;
   else if(q.cmd==='workbench.files')data.files=['readme.md'];
   else if(q.cmd==='workbench.file.read')data.content='# 分屏说明\n\n文件与终端可并排工作。';
   else if(q.cmd==='workbench.terminal.open'){state.opens++;if(!state.sessions.some(s=>s.id===q.request_id))state.sessions.push(session(q.request_id));data.session_id=q.request_id;}
   else if(q.cmd==='workbench.terminal.output'){const output=Buffer.from('READY '+q.session_id+'\r\n> ');const from=q.since||0;Object.assign(data,{from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:24,sequence:1}]:[]});}
   else if(q.cmd==='workbench.git')Object.assign(data,{branch:'',git:false,status:'',diff:'',staged:''});
   await route.fulfill({json:data});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__split`);
  await page.getByRole('button',{name:'向右拆分终端',exact:true}).waitFor();
  await run(page,state);
  assert.deepEqual(state.errors,[]);
  writeFileSync(join(out,name+'.json'),JSON.stringify({passed:true,scope:'Real WorkspaceApp/TerminalView, mocked RPC, no real shell or model',opens:state.opens,sessionCount:state.sessions.length,layout:state.layout,errors:state.errors,created:await page.evaluate(()=>window.created),disposed:await page.evaluate(()=>window.disposed)},null,2));
 }catch(e){await page?.screenshot({path:join(out,name+'-failure.png')});writeFileSync(join(out,name+'-failure.json'),JSON.stringify({error:String(e),errors:state.errors,layout:state.layout,body:await page?.locator('body').innerText()},null,2));throw e;}
 finally{await browser?.close();await server.close();}
}
const idle=page=>page.waitForFunction(()=>!document.querySelector('button[aria-label="布局与标签"]')?.disabled);
const terminalPane=(page,id)=>page.locator(`[data-terminal-pane="${id}"]`);
async function dragTab(page,title,paneId,edge){
 const target=page.locator(`[data-pane-id="${paneId}"]`),box=await target.boundingBox();
 const point=edge==='right'?{x:box.width-8,y:box.height/2}:edge==='bottom'?{x:box.width/2,y:box.height-8}:{x:box.width/2,y:box.height/2};
 await page.getByRole('tab',{name:title,exact:true}).locator('..').dragTo(target,{targetPosition:point});await idle(page);
}
test('both split layers nest, resize, move, preserve input and restore from storage',async()=>fixture('interaction',async(page,s)=>{
 await page.getByRole('button',{name:'向右拆分终端',exact:true}).click();await idle(page);assert.equal(leaves(s.layout.panes[0].tabs[0].terminalTree).length,2);
 const second=leaves(s.layout.panes[0].tabs[0].terminalTree)[1];
 await terminalPane(page,second).locator('.terminal-canvas').click();await page.keyboard.press('Alt+Shift+D');await idle(page);
 await terminalPane(page,'s1').getByRole('button',{name:'向右拆分终端',exact:true}).click();await idle(page);
 const ids=leaves(s.layout.panes[0].tabs[0].terminalTree);assert.equal(ids.length,4);assert.equal(s.opens,3);
 for(const [i,id] of ids.entries()){await terminalPane(page,id).locator('.terminal-canvas').click();assert.ok((await terminalPane(page,id).getAttribute('class')).includes('is-active'));await page.keyboard.type('probe'+i);}
 await page.waitForTimeout(100);for(const [i,id] of ids.entries())assert.ok(s.requests.filter(q=>q.cmd==='workbench.terminal.input'&&q.session_id===id).map(q=>q.text||'').join('').includes('probe'+i));
 const divider=page.getByRole('separator',{name:'终端分界线',exact:true}).first(),box=await divider.boundingBox();
 // Avoid the intersection with the nested horizontal divider at mid-height.
 await page.mouse.move(box.x+4,box.y+box.height*.3);await page.mouse.down();await page.mouse.move(box.x+84,box.y+box.height*.3,{steps:5});await page.mouse.up();await idle(page);assert.ok(s.layout.panes[0].tabs[0].terminalTree.ratio>.5);
 await divider.focus();await page.keyboard.press('ArrowLeft');await idle(page);
 const beforeMove=await page.evaluate(()=>({created:window.created.length,disposed:window.disposed}));
 await terminalPane(page,'s1').getByRole('button',{name:'展开当前终端',exact:true}).click();assert.equal(await page.locator('[data-terminal-pane]:visible').count(),1);await page.getByRole('button',{name:'还原终端分屏',exact:true}).click();assert.equal(await page.locator('[data-terminal-pane]:visible').count(),4);
 const from=terminalPane(page,ids[0]).locator('.terminal-pane-title'),dest=terminalPane(page,ids[3]),destBox=await dest.boundingBox();await from.dragTo(dest,{targetPosition:{x:destBox.width/2,y:destBox.height-8}});await idle(page);assert.deepEqual(leaves(s.layout.panes[0].tabs[0].terminalTree),[ids[1],ids[2],ids[3],ids[0]],'moving a terminal actually changes placement without duplicating it');
 await dragTab(page,'终端二','main','right');assert.equal(s.layout.panes.length,2);
 await dragTab(page,'说明.md','main','bottom');assert.equal(s.layout.panes.length,3);
 await dragTab(page,'浏览器','main','right');assert.equal(s.layout.panes.length,4);
 assert.equal(await page.locator('[data-pane-id]').count(),4);assert.equal(await page.locator('.browser-pane').count(),1);assert.ok((await page.locator('body').innerText()).includes('文件与终端可并排工作'));
 const workDivider=page.getByRole('separator',{name:'工作台分界线',exact:true}).first();await workDivider.focus();await page.keyboard.press('ArrowRight');await idle(page);assert.equal(s.layout.tree.ratio,.55);
 assert.equal(await page.evaluate(()=>window.disposed),beforeMove.disposed);assert.equal(await page.evaluate(()=>window.created.length),beforeMove.created+1,'only the newly visible second original terminal is mounted');
 for(const width of [1440,1100,900]){await page.setViewportSize({width,height:1000});await page.screenshot({path:join(out,`nested-${width}.png`)});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));}
 await page.setViewportSize({width:1440,height:1000});
 const saved=structuredClone(s.layout);await page.reload();await page.locator('[data-pane-id]').first().waitFor();await idle(page);assert.deepEqual(s.layout,saved);assert.equal(await page.locator('[data-pane-id]').count(),4);
 const closeId=ids[2];await terminalPane(page,closeId).getByRole('button',{name:'关闭终端分屏',exact:true}).click();await idle(page);assert.equal(s.sessions.length,5);assert.equal(s.layout.closed[0].target,closeId);assert.ok(!s.requests.some(q=>q.cmd.endsWith('.stop')));
 await page.getByRole('button',{name:'布局与标签',exact:true}).click();await page.getByRole('menuitem',{name:'合并分屏',exact:true}).click();await idle(page);assert.equal(s.layout.panes.length,1);assert.equal(s.layout.panes[0].tabs.length,4);
}));
test('failed layout save retries the same terminal instead of creating another',async()=>fixture('retry',async(page,s)=>{
 s.failSave=true;await page.getByRole('button',{name:'向右拆分终端',exact:true}).click();await page.getByRole('alert').waitFor();await idle(page);assert.equal(s.opens,1);assert.equal(s.sessions.length,3);
 await page.getByRole('button',{name:'向右拆分终端',exact:true}).click();await idle(page);assert.equal(s.opens,1);assert.equal(s.sessions.length,3);assert.equal(leaves(s.layout.panes[0].tabs[0].terminalTree).length,2);assert.equal(await page.getByRole('alert').count(),0);
}));
