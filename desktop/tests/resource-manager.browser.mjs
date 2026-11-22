import {test} from 'node:test';import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';import {resolve} from 'node:path';import {mkdirSync,writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
const out=resolve(root,'../artifacts/resource-manager-20261028');
const html='<html><body><div id="root" style="height:100vh"></div><script type="module" src="/__resources.tsx"></script></body></html>';
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';createRoot(document.getElementById('root')).render(<ConfirmationProvider><TooltipProvider><WorkspaceApp/></TooltipProvider></ConfirmationProvider>);`;

test('resource manager tree, navigation, geometry, polling, scan preview and confirmed cleanup',{timeout:90000},async()=>{
 const server=await createServer({root,configFile:false,plugins:[react(),{name:'resource-fixture',resolveId:id=>id==='/__resources.tsx'?id:null,load:id=>id==='/__resources.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__resources'){void s.transformIndexHtml(req.url,html).then(v=>{res.setHeader('Content-Type','text/html');res.end(v)});return}next()})}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser;
 try{
  mkdirSync(out,{recursive:true});await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1440,height:960}});
  await page.addInitScript(()=>{
   localStorage.setItem('velella:workspace','alpha');localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));
   window.velellaDesktop={invoke:async(command,args)=>{
    if(command==='window')return false;
    const response=await fetch(command==='desktop_resources'?'/__metrics':command.startsWith('resource_')?'/__'+command:'/_velella',{method:'POST',body:JSON.stringify(command==='rpc'?args.request:args)});
    if(!response.ok)throw Error('fixture metrics unavailable');return response.json();
   },listen:()=>()=>{}};
  });
  let metricsReads=0,metricsOk=true,snapshotOk=true,sessionOk=true;
  await page.route('**/__metrics',async route=>{metricsReads++;await route.fulfill(metricsOk?{json:{sampledAt:Date.now(),scope:'electron-private',privateBytes:300e6,processes:[{pid:101,type:'Browser',privateBytes:100e6},{pid:102,type:'Tab',privateBytes:200e6}]}}:{status:503,json:{error:'fixture'}})});
  const workspaces=['alpha','beta'].map((id,i)=>({id,project_id:'p',name:i?'Beta':'Alpha',path:'fixture/'+id,kind:'worktree',archived:false,favorite:false,position:i}));
  const live=(id,title)=>({id,title,kind:'terminal',native_chat:false,program:'powershell',status:{pid:100+id.length,exit:null,state:'running',input_protocol:1}});
  const lists={alpha:[live('a1','Alpha shell'),{...live('ended','Ended'),status:{pid:7,exit:0,state:'completed'}},{...live('lost','Lost'),status:{pid:8,exit:null,state:'host_lost'}},{...live('chat','Chat'),native_chat:true}],beta:[live('b1','Beta shell'),live('b2','Second shell')]};
  const layouts=Object.fromEntries(workspaces.map(w=>[w.id,{revision:1,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:'t1',tabs:[{id:'t1',kind:'terminal',target:lists[w.id][0].id,title:lists[w.id][0].title}]}]}]));
  const mutations=[],errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/_velella',async route=>{
   const q=route.request().postDataJSON();let data={ok:true};
   if(q.cmd==='workbench.snapshot')data=snapshotOk?{ok:true,projects:[{id:'p',name:'Project',path:'fixture'}],workspaces,tasks:[]}:{ok:false,error:'fixture backend unavailable'};
   if(q.cmd==='workbench.sessions')data=sessionOk?{ok:true,sessions:lists[q.workspace_id]}:{ok:false,error:'fixture sessions unavailable'};
   if(q.cmd==='workbench.layout')data={ok:true,layout:layouts[q.workspace_id],split_layout_version:1};
   if(q.cmd==='workbench.layout.save'){layouts[q.workspace_id]={...q.layout,revision:layouts[q.workspace_id].revision+1};data.layout=layouts[q.workspace_id];mutations.push(q.cmd)}
   if(q.cmd==='workbench.files')data.files=[];
   if(q.cmd==='workbench.terminal.output'){const output=Buffer.from('READY\r\n'),from=Math.min(q.since??0,output.length);data={ok:true,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:1}]:[]}}
   if(/\.(open|stop|recover|input)$/.test(q.cmd))mutations.push(q.cmd);
   await route.fulfill({json:data});
  });

  let managerReads=0,scanReads=0,cleanReads=0,managerOk=true;
  const metrics={cpu:0.4,workingSetBytes:1.44e9,privateBytes:1.79e9};
  await page.route('**/__resource_manager',async route=>{managerReads++;await route.fulfill(managerOk?{json:{...metrics,sampledAt:Date.now(),projects:[{id:'p',name:'Project',...metrics,workspaces:workspaces.map(w=>({...w,...metrics,sessions:lists[w.id].filter(s=>s.id.length===2).map(s=>({...s,...metrics,pid:s.status.pid,processCount:2}))}))}],desktop:{name:'Velella 桌面',...metrics,processes:[{pid:101,name:'Velella.exe',...metrics}]}}}:{status:503,json:{error:'unavailable'}})});
  await page.route('**/__resource_space_scan',async route=>{scanReads++;assert.deepEqual(route.request().postDataJSON().target,{kind:'local'},'a resource scan must name the host it reads');await route.fulfill({json:{token:'preview',scannedAt:Date.now(),rows:[{id:'beta',name:'Beta',path:'fixture/beta',bytes:250e6,partial:true,caches:[{key:'cache',relative:'.vite',path:'fixture/beta/.vite',bytes:1e6},{key:'partial',relative:'.turbo',path:'fixture/beta/.turbo',bytes:1e6,partial:true}]}]}})});
  await page.route('**/__resource_space_clean',async route=>{cleanReads++;assert.deepEqual(route.request().postDataJSON(),{target:{kind:'local'},token:'preview',keys:['cache'],confirmed:true});await route.fulfill({json:{ok:true,completed:[{path:'fixture/beta/.vite',bytes:1e6}]}})});
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__resources`);
  const openDetails=async name=>{await page.getByRole('button',{name:'状态与资源',exact:true}).click();await page.getByRole('menuitem',{name,exact:true}).click()};
  await page.getByRole('button',{name:'状态与资源',exact:true}).waitFor();await page.waitForTimeout(200);assert.equal(metricsReads,0,'diagnostics must not sample until opened');assert.equal(await page.locator('.workbench-backend,.ide-connection').count(),0);assert.equal(await page.locator('.resource-status').innerText(),'状态与资源');

  await page.getByRole('button',{name:'Beta',exact:true}).click();await page.locator('.ide-workspace.selected').filter({hasText:'Beta'}).waitFor();
  await openDetails('运行终端');await page.getByRole('dialog',{name:'运行终端',exact:true}).getByRole('button',{name:/Second shell/}).click();
  await page.waitForFunction(()=>document.querySelectorAll('.ide-tab').length===2);assert.ok(layouts.beta.panes.some(p=>p.tabs.some(t=>t.target==='b2')),'details must open existing session');
  const widths=[];
  for(const width of [1440,1100,900,800]){
   await page.setViewportSize({width,height:960});await page.waitForTimeout(100);
   const m=await page.evaluate(()=>{const r=document.querySelector('.resource-status').getBoundingClientRect(),c=document.querySelector('.window-controls').getBoundingClientRect(),f=document.querySelector('.ide-status').getBoundingClientRect(),p=document.querySelector('.ide-status>span').getBoundingClientRect();return {resourceLeft:r.left,resourceRight:r.right,resourceTop:r.top,resourceBottom:r.bottom,footerLeft:f.left,footerRight:f.right,footerTop:f.top,footerBottom:f.bottom,pathRight:p.right,controlRight:c.right,topResources:document.querySelectorAll('.app-titlebar .resource-status').length}});
   assert.ok(m.resourceLeft>=m.footerLeft&&m.resourceRight<=m.footerRight&&m.resourceTop>=m.footerTop&&m.resourceBottom<=m.footerBottom+1&&m.footerBottom<=960&&m.footerBottom>=936&&m.pathRight<=m.resourceLeft&&m.controlRight<=width+1&&m.topResources===0,JSON.stringify({width,...m}));widths.push({width,...m});await page.screenshot({path:resolve(out,`bar-${width}.png`)});
  }
  await openDetails('资源管理器');await page.getByRole('dialog',{name:'资源管理器',exact:true}).waitFor();await page.keyboard.press('Escape');
  await page.setViewportSize({width:1440,height:960});
  await openDetails('桌面内存');await page.locator('.resource-total').getByText('300 MB',{exact:true}).waitFor();
  // Open diagnostics pause while hidden; closed diagnostics do not poll.
  await page.evaluate(()=>{window.fixtureHidden=true;Object.defineProperty(document,'hidden',{configurable:true,get:()=>window.fixtureHidden});document.dispatchEvent(new Event('visibilitychange'))});
  await page.waitForTimeout(100);const beforeHidden=metricsReads;await page.waitForTimeout(5200);assert.equal(metricsReads,beforeHidden);
  metricsOk=false;sessionOk=false;
  await page.evaluate(()=>{window.fixtureHidden=false;document.dispatchEvent(new Event('visibilitychange'))});
  await page.locator('.resource-total').getByText('—',{exact:true}).waitFor();await page.getByText(/暂时无法读取/).waitFor();await page.keyboard.press('Escape');
  await openDetails('运行终端');await page.getByRole('dialog',{name:'运行终端',exact:true}).getByText(/fixture sessions unavailable/).waitFor();await page.keyboard.press('Escape');
  metricsOk=true;sessionOk=true;await openDetails('桌面内存');await page.locator('.resource-total').getByText('300 MB',{exact:true}).waitFor();await page.keyboard.press('Escape');const closedMetrics=metricsReads;await page.waitForTimeout(5200);assert.equal(metricsReads,closedMetrics);
  snapshotOk=false;await page.getByRole('button',{name:'后台连接：重连中',exact:true}).waitFor();await page.getByRole('button',{name:'后台连接：连接失败',exact:true}).waitFor();
  await page.getByRole('button',{name:'后台连接：连接失败',exact:true}).click();assert.ok((await page.getByRole('dialog').innerText()).includes('fixture backend unavailable'));await page.keyboard.press('Escape');
  snapshotOk=true;await page.waitForFunction(()=>!document.querySelector('.resource-connection'));assert.equal(await page.locator('.resource-status').innerText(),'状态与资源');

  assert.equal(scanReads,0);assert.equal(managerReads,1);
  await openDetails('资源管理器');
  await page.locator('.resource-manager-summary').getByText('1.44 GB ∑ WS',{exact:true}).waitFor();
  await page.getByRole('button',{name:'切换内存列',exact:true}).click();assert.equal(await page.getByRole('button',{name:'切换内存列'}).innerText(),'Private');
  for(const viewport of [{width:1440,height:960},{width:800,height:520}]){
   await page.setViewportSize(viewport);await page.waitForTimeout(100);const rect=await page.locator('.resource-manager').boundingBox();assert.ok(rect.x>=0&&rect.y>=0&&rect.x+rect.width<=viewport.width&&rect.y+rect.height<=viewport.height);await page.screenshot({path:resolve(out,'manager-'+viewport.width+'.png')});
  }
  await page.setViewportSize({width:1440,height:960});
  // A hidden open panel must not sample; failure must replace stale readings.
  await page.evaluate(()=>{window.fixtureHidden=true;document.dispatchEvent(new Event('visibilitychange'))});await page.waitForTimeout(100);const hiddenReads=managerReads;await page.waitForTimeout(5200);assert.equal(managerReads,hiddenReads);
  managerOk=false;await page.evaluate(()=>{window.fixtureHidden=false;document.dispatchEvent(new Event('visibilitychange'))});await page.locator('.resource-manager-message').getByText(/暂时无法读取/).waitFor();assert.equal(await page.locator('.resource-manager-summary').innerText(),'—\n·\n— ∑ WS\n·\n— ∑ Private');
  managerOk=true;await page.getByRole('button',{name:'刷新资源',exact:true}).click();await page.locator('.resource-manager-summary').getByText('1.44 GB ∑ WS',{exact:true}).waitFor();
  await page.getByRole('button',{name:'预览工作区清理',exact:true}).click();assert.equal(scanReads,0);await page.getByRole('button',{name:'扫描',exact:true}).click();await page.getByText('扫描未完成，显示已读取部分。',{exact:true}).waitFor();
  assert.equal(await page.getByRole('checkbox',{name:'选择缓存 fixture/beta/.turbo',exact:true}).isDisabled(),true);
  await page.getByRole('checkbox',{name:'选择缓存 fixture/beta/.vite',exact:true}).check();await page.getByRole('button',{name:'预览并清理所选缓存 (1)',exact:true}).click();
  await page.getByRole('dialog',{name:'确认操作',exact:true}).waitFor();assert.ok((await page.getByRole('dialog',{name:'确认操作',exact:true}).innerText()).includes('fixture/beta/.vite'));await page.getByRole('button',{name:'取消',exact:true}).click();assert.equal(cleanReads,0);
  await page.getByRole('button',{name:'预览并清理所选缓存 (1)',exact:true}).click();await page.getByRole('button',{name:'确认',exact:true}).click();await page.getByText(/已移入回收站 1 个目录/).waitFor();assert.equal(cleanReads,1);
  await page.getByRole('button',{name:'‹ 返回资源列表',exact:true}).click();await page.locator('.resource-terminal').filter({hasText:'Alpha shell'}).click();await page.locator('.ide-workspace.selected').filter({hasText:'Alpha'}).waitFor();assert.equal(await page.locator('.resource-manager').count(),0);
  const closedReads=managerReads;await page.waitForTimeout(5200);assert.equal(managerReads,closedReads);assert.equal(scanReads,1);

  assert.equal(mutations.some(c=>c!=='workbench.layout.save'),false,'status navigation must not start/stop/input into sessions');assert.deepEqual(errors,[]);
  const receipt={managerReads,scanReads,cleanReads,status:'passed',widths,metricsReads,hiddenSamplingPaused:true,sessionNavigation:true,failuresUnavailable:true,connectionRecovery:true,noTerminalLifecycleMutation:true};writeFileSync(resolve(out,'browser-result.json'),JSON.stringify(receipt,null,2));console.log(JSON.stringify(receipt));
 }finally{await browser?.close();await server.close()}
});
