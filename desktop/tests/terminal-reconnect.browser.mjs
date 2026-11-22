import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
let server,browser,url;
before(async()=>{
 server=await createServer({root,configFile:false,logLevel:'error',plugins:[react(),{
  name:'reconnect-fixture',resolveId:id=>id==='/__reconnect.tsx'?id:null,
  load:id=>id==='/__reconnect.tsx'?`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {TooltipProvider} from '/src/ui';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.terms=[];window.addEventListener('velella:terminal-created',e=>window.terms.push(e.detail.terminal));createRoot(document.getElementById('root')).render(<TooltipProvider><WorkspaceApp/></TooltipProvider>);`:null,
  configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__reconnect'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__reconnect.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}
 }],server:{host:'127.0.0.1',port:0}});
 await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}/__reconnect`;
 browser=await chromium.launch({channel:'msedge',headless:true});
});
after(async()=>{await browser?.close();await server?.close()});

async function fixture(mode,run){
 const page=await browser.newPage();const requests=[];
 const lost=mode==='lost'||mode==='failure';
 const session={id:'s',title:'PowerShell',kind:'terminal',program:'powershell',task_id:null,native_id:null,recoverable:mode!=='unreachable',auto_recoverable:lost,status:mode==='unreachable'?null:{pid:1234,exit:mode==='exited'?0:null,state:lost?'host_lost':'done',input_protocol:lost?0:1}};
 const history=Buffer.from('SAVED_HISTORY\r\nPS C:\\fixture> ');let output=history;
 await page.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));});
 await page.route('**/_velella',async route=>{
  const q=route.request().postDataJSON();requests.push(q);let data={ok:true};
  if(q.cmd==='workbench.snapshot')Object.assign(data,{projects:[{id:'p',name:'Fixture',path:'fixture'}],workspaces:[{id:'w',project_id:'p',name:'Fixture',kind:'folder',path:'fixture',archived:false,favorite:false,position:0}],tasks:[]});
  if(q.cmd==='workbench.layout')data.layout={revision:1,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:'t',tabs:[{id:'t',target:'s',kind:'terminal',title:'PowerShell'}]}]};
  if(q.cmd==='workbench.sessions')data.sessions=[session];
  if(q.cmd==='workbench.files')data.files=[];
  if(q.cmd==='workbench.terminal.recover'){
   assert.equal(q.automatic,true);session.auto_recoverable=false;
   if(mode==='failure')data={ok:false,error:'fixture launch failed'};
   else{session.recoverable=false;session.status={pid:5678,exit:null,state:'starting',input_protocol:1};output=Buffer.concat([history,Buffer.from('\r\nNEW_SHELL_READY\r\n')]);}
  }
  if(q.cmd==='workbench.terminal.output'){const from=q.since??0;Object.assign(data,{from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:0}]:[]});}
  await route.fulfill({json:data});
 });
 try{await page.goto(url);await page.getByRole('tab',{name:'PowerShell',exact:true}).waitFor();await run(page,requests)}finally{await page.close()}
}
test('lost shell reconnects once, retains history and sends input only to replacement host',async()=>fixture('lost',async(page,requests)=>{
 await page.waitForFunction(()=>window.terms.some(t=>!t.options.disableStdin));
 await page.locator('.xterm-helper-textarea').focus();await page.keyboard.type('echo restored');await page.keyboard.press('Enter');
 await page.waitForFunction(()=>document.body.innerText.includes('Shell 已重新启动'));
 assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,1);
 const deadline=Date.now()+5000;while(requests.filter(q=>q.cmd==='workbench.terminal.input').map(q=>q.text).join('')!=='echo restored\r'&&Date.now()<deadline)await page.waitForTimeout(25);
 const inputs=requests.filter(q=>q.cmd==='workbench.terminal.input');assert.equal(inputs.map(q=>q.text).join(''),'echo restored\r');assert.ok(inputs.every(q=>q.expected_host_pid===5678));
 const text=await page.evaluate(()=>{const t=window.terms.at(-1);return Array.from({length:t.buffer.active.length},(_,i)=>t.buffer.active.getLine(i)?.translateToString(true)).join('\n')});
 assert.match(text,/SAVED_HISTORY/);assert.match(text,/NEW_SHELL_READY/);assert.doesNotMatch(await page.locator('body').innerText(),/输入适配不可用/);
}));
for(const mode of ['unreachable','exited','failure'])test(`${mode} does not cause a restart loop or misleading adapter error`,async()=>fixture(mode,async(page,requests)=>{
 const message=mode==='unreachable'?'正在重新连接终端':mode==='exited'?'进程已结束':'自动恢复未完成';
 await page.getByRole('status').filter({hasText:message}).waitFor();await page.waitForTimeout(2300);
 assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,mode==='failure'?1:0);
 assert.doesNotMatch(await page.locator('body').innerText(),/输入适配不可用/);
 if(mode!=='unreachable')await page.getByRole('button',{name:'重新启动终端',exact:true}).waitFor();
 if(mode==='failure'){await page.reload();await page.getByRole('button',{name:'重新启动终端',exact:true}).waitFor();await page.waitForTimeout(2300);assert.equal(requests.filter(q=>q.cmd==='workbench.terminal.recover').length,1);}
}));
