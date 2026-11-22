import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdirSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
let server,browser,url;
before(async()=>{
 const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import Hybrid,{HybridApprovals} from '/src/HybridCodexLauncher';import '/src/theme.css';import '/src/style.css';import '/src/creation-dialog.css';const approval=location.search.includes('approvals');createRoot(document.getElementById('root')).render(approval?<HybridApprovals workspaceId="w" sessionId="s"/>:<Hybrid workspaceId="w" onClose={()=>{document.title='closed'}} onOpened={async id=>{document.title=id}}/>);`;
 server=await createServer({root,configFile:false,logLevel:'error',plugins:[react(),{name:'hybrid-fixture',resolveId:id=>id==='/__hybrid.tsx'?id:null,load:id=>id==='/__hybrid.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url.startsWith('/__hybrid?')||req.url==='/__hybrid'){void s.transformIndexHtml('/__hybrid','<html lang="zh-CN"><body><div id="root"></div><script type="module" src="/__hybrid.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}}],server:{host:'127.0.0.1',port:0}});
 await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}/__hybrid`;browser=await chromium.launch({channel:'msedge',headless:true});
});
after(async()=>{await browser?.close();await server?.close()});
test('hybrid launcher sends local workspace and selected SSH profile once',async()=>{
 const page=await browser.newPage();const launches=[];
 await page.route('**/_velella',route=>{const req=route.request().postDataJSON();if(req.cmd==='ssh.list')return route.fulfill({json:{ok:true,profiles:[{id:'p',name:'Linux',host:'agent',user:'doctor'}]}});launches.push(req);return route.fulfill({json:{ok:true,session_id:'s'}})});
 await page.goto(url);await page.getByRole('button',{name:'创建并打开'}).click();await page.waitForFunction(()=>document.title==='closed');assert.equal(launches.length,1);assert.equal(launches[0].workspace_id,'w');assert.equal(launches[0].launch,'hybrid-codex');assert.equal(launches[0].ssh_profile_id,'p');await page.close();
});
test('approval shows exact local operation and sends its digest with rejection',async()=>{
 const page=await browser.newPage({viewport:{width:800,height:600}});const decisions=[];let state='pending';
 await page.route('**/_velella',route=>{const req=route.request().postDataJSON();if(req.cmd==='workbench.hybrid.decide'){decisions.push(req);state='rejected';return route.fulfill({json:{ok:true}})}return route.fulfill({json:{ok:true,operations:[{id:'op1',digest:'sha256:fixture',state,tool:'local_run_command',arguments:{program:'python',args:['-c','print("local")'],cwd:'.'}}]}})});
 await page.goto(url+'?approvals');await page.getByRole('button',{name:'拒绝',exact:true}).waitFor();assert.match(await page.locator('pre').innerText(),/python/);
 const artifact=fileURLToPath(new URL('../../artifacts/minimax-hybrid-20261112/',import.meta.url));mkdirSync(artifact,{recursive:true});await page.screenshot({path:artifact+'hybrid-approval.png'});
 await page.getByRole('button',{name:'拒绝',exact:true}).click();await page.getByRole('button',{name:'拒绝',exact:true}).waitFor({state:'detached'});assert.equal(decisions.length,1);assert.equal(decisions[0].approved,false);assert.equal(decisions[0].digest,'sha256:fixture');assert.equal(decisions[0].confirmed,true);await page.close();
});
