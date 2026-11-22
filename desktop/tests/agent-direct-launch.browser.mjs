import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
let browser,server,url;
before(async()=>{
 const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {TooltipProvider} from '/src/ui';import {AppearanceProvider} from '/src/Appearance';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);`;
 server=await createServer({root,configFile:false,logLevel:'error',plugins:[react(),{name:'direct-launch',resolveId:id=>id==='/__direct_launch.tsx'?id:null,load:id=>id==='/__direct_launch.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__direct_launch'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__direct_launch.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}}],server:{host:'127.0.0.1',port:0,fs:{allow:[root,'D:/orcas/desktop/node_modules']}}});
 await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}/__direct_launch`;browser=await chromium.launch({channel:'msedge',headless:true});
});
after(async()=>{await browser?.close();await server?.close()});
const agents=[['codex','Codex','codex'],['claude-code','Claude','claude'],['gemini','Gemini CLI','gemini'],['opencode','OpenCode','opencode'],['omp','Oh My Pi (OMP)','omp'],['kimi','Kimi','kimi'],['zcode','ZCode','zcode-free'],['minimax','MiniMax Code','mcode']];
const newMenu=page=>page.getByRole('button',{name:'新建标签',exact:true}).or(page.getByRole('button',{name:'新建终端',exact:true})).first();
async function fixture(run,{preferences={},failSave=false}={}){
 const context=await browser.newContext(),page=await context.newPage(),requests=[],errors=[],sessions=[];
 page.setDefaultTimeout(7000);
 let layout={revision:0,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:null,tabs:[]}]},failed=false;
 await page.addInitScript(p=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:agents:v1',JSON.stringify(p));localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}))},preferences);
 page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/_velella',async route=>{
  const q=route.request().postDataJSON();requests.push(q);let data={ok:true};
  if(q.cmd==='protocol')data.capabilities=['terminal-permissions-v1'];
  if(q.cmd==='workbench.snapshot')Object.assign(data,{projects:[{id:'p',name:'Fixture',path:'/fixture',direct:true}],workspaces:[{id:'w',project_id:'p',name:'Fixture',path:'/fixture',kind:'folder',archived:false,favorite:false,position:0}],tasks:[],source_links:[]});
  if(q.cmd==='workbench.sessions')data.sessions=sessions;
  if(q.cmd==='workbench.layout')Object.assign(data,{layout,split_layout_version:1});
  if(q.cmd==='workbench.files')data.files=[];
  if(q.cmd==='workbench.git')Object.assign(data,{branch:'',status:''});
  if(q.cmd==='workbench.terminal.open'){
   const a=agents.find(a=>a[0]===q.launch);assert(a);if(!sessions.some(s=>s.id===q.request_id))sessions.push({id:q.request_id,title:a[1],kind:'terminal',program:a[2],execution_mode:'native',task_id:null,native_chat:false,status:{pid:1234,exit:null,state:'starting',input_protocol:1}});data.session_id=q.request_id;
  }
  if(q.cmd==='workbench.layout.save'){layout=q.layout;layout.revision++;if(failSave&&!failed){failed=true;data={ok:false,error:'layout acknowledgement lost'}}else data.layout=layout;}
  if(q.cmd==='workbench.terminal.output'){const b=Buffer.from('READY\r\n'),from=Math.min(q.since??0,b.length);Object.assign(data,{from,end:b.length,total:b.length,data:b.subarray(from).toString('base64'),geometry:from===0?[{offset:0,rows:24,cols:80,sequence:0}]:[]})}
  if(q.cmd==='workbench.terminal.activity')Object.assign(data,{enabled:false,events:[],next:0,total:0});
  await route.fulfill({json:data});
 });
 try{await page.goto(url);await newMenu(page).waitFor();await run(page,requests);assert.deepEqual(errors,[])}catch(error){writeFileSync(new URL('../direct-launch-fixture-error.json',import.meta.url),JSON.stringify({body:await page.locator('body').innerText(),requests,errors},null,2));throw error}finally{await context.close()}
}
for(const [id,name] of agents)test(`${name}: normal menu click opens directly with inherited CLI settings`,async()=>fixture(async(page,requests)=>{
 await newMenu(page).click();await page.getByRole('menuitem',{name:`新建 ${name} 终端`,exact:true}).click();
 await page.getByRole('tab',{name,exact:true}).waitFor();assert.equal(await page.getByRole('dialog').count(),0);
 const opens=requests.filter(q=>q.cmd==='workbench.terminal.open');assert.equal(opens.length,1);assert.equal(opens[0].launch,id);assert.match(opens[0].request_id,/^[\da-f-]{36}$/);
 for(const field of ['permission_mode','sandbox','launch_options','initial_prompt'])assert.equal(opens[0][field],undefined);
}));
test('direct ZCode launch uses the saved per-agent permission without rewriting preferences',async()=>fixture(async(page,requests)=>{
 const before=await page.evaluate(()=>localStorage.getItem('velella:agents:v1'));
 await newMenu(page).click();await page.getByRole('menuitem',{name:'新建 ZCode 终端',exact:true}).click();await page.getByRole('tab',{name:'ZCode',exact:true}).waitFor();
 assert.equal(requests.find(q=>q.cmd==='workbench.terminal.open').permission_mode,'guarded');assert.equal(await page.getByRole('dialog').count(),0);assert.equal(await page.evaluate(()=>localStorage.getItem('velella:agents:v1')),before);
},{preferences:{permissions:{zcode:'guarded'}}}));
test('configuration dialog is available only through the explicit startup-options submenu',async()=>fixture(async(page,requests)=>{
 await newMenu(page).click();await page.getByRole('menuitem',{name:'启动选项…',exact:true}).hover();await page.getByRole('menuitem',{name:'ZCode 启动选项…',exact:true}).click();
 await page.getByRole('dialog',{name:'新建 ZCode 终端'}).waitFor();assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.open').length,0);
}));
test('retry after an ambiguous layout save reuses the existing session and request',async()=>fixture(async(page,requests)=>{
 for(let i=0;i<2;i++){await newMenu(page).click();await page.getByRole('menuitem',{name:'新建 ZCode 终端',exact:true}).click();await page.getByRole('menu').waitFor({state:'hidden'});if(!i)await page.getByText(/ZCode 终端打开未完成/).waitFor();}
 await page.getByRole('tab',{name:'ZCode',exact:true}).waitFor();assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.open').length,1);assert.equal(await page.getByRole('dialog').count(),0);
},{failSave:true}));
