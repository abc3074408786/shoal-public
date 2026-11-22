import {test} from 'node:test';import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';import {resolve} from 'node:path';import {realpathSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
const html='<html><body><div id="root" style="width:900px;height:550px;display:flex"></div><script type="module" src="/__input-entry.tsx"></script></body></html>';
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import {Terminal} from '@xterm/xterm';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';const open=Terminal.prototype.open;Terminal.prototype.open=function(e){open.call(this,e);window.term=this};createRoot(document.getElementById('root')).render(<TooltipProvider><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id:'fixture',status:{exit:null,input_protocol:1}}}/></TooltipProvider>);`;
for(const scenario of ['ordered','read-error','send-error'])test(`clipboard preserves ordering and failure policy: ${scenario}`,async()=>{
 const server=await createServer({root,configFile:false,plugins:[react(),{name:'ordered-input-fixture',configureServer(s){s.middlewares.use((req,res,next)=>{
  if(req.url==='/__input'){void s.transformIndexHtml(req.url,html).then(v=>{res.setHeader('Content-Type','text/html');res.end(v)});return}
  if(req.url==='/__input-entry.tsx'){void s.transformRequest(req.url).then(v=>{res.setHeader('Content-Type','text/javascript');res.end(v.code)});return}next();
 })},resolveId(id){if(id==='/__input-entry.tsx')return id},load(id){if(id==='/__input-entry.tsx')return entry}}],server:{host:'127.0.0.1',port:0,fs:{allow:[root,realpathSync(resolve(root,'node_modules'))]}},logLevel:'error'});
 let browser;try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage();
  await page.addInitScript(()=>{localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));Object.defineProperty(navigator,'clipboard',{value:{readText:()=>new Promise((resolve,reject)=>{window.finishClipboard=resolve;window.failClipboard=reject}),writeText:()=>Promise.resolve()}})});
  const sent=[];const output=Buffer.from('\x1b[?2004h');
  await page.route('**/_velella',async route=>{const q=route.request().postDataJSON();let r={ok:true};
   if(q.cmd.endsWith('terminal.output')){const from=Math.min(q.since??0,output.length);r={ok:true,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:90,rows:25,sequence:1}]:[]}}
   if(q.cmd.endsWith('terminal.input')){sent.push(q.text);if(scenario==='send-error'&&sent.length===1)r={ok:false,error:'fixture ambiguous input timeout'}}await route.fulfill({json:r});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__input`);await page.waitForFunction(()=>window.term?.modes.bracketedPasteMode);
  const input=page.locator('.xterm-helper-textarea');await input.focus();await input.press('Control+v');await page.waitForFunction(()=>!!window.finishClipboard);
  await input.pressSequentially('suffix');await input.press('Enter');await page.waitForTimeout(100);
  assert.deepEqual(sent,[],'typing/Enter must wait behind the earlier clipboard gesture');
  if(scenario==='read-error')await page.evaluate(()=>window.failClipboard(new Error('fixture clipboard denied')));
  else await page.evaluate(()=>window.finishClipboard('中文\n第二行'));
  if(scenario==='send-error'){
   const resume=page.getByRole('button',{name:'核对后恢复输入'});await resume.waitFor();
   await page.waitForTimeout(100);assert.deepEqual(sent,['\x1b[200~中文\r第二行\x1b[201~']);
   await resume.click();await input.focus();await input.press('z');
   await page.waitForTimeout(180);assert.deepEqual(sent,['\x1b[200~中文\r第二行\x1b[201~','z'],'resume must not replay paste or queued suffix/Enter');
  }else{
   await page.waitForTimeout(180);
   assert.equal(sent.join(''),scenario==='ordered'?'\x1b[200~中文\r第二行\x1b[201~suffix\r':'suffix\r');
   if(scenario==='read-error')assert.equal(await page.getByRole('button',{name:'核对后恢复输入'}).count(),0,'known clipboard read failure must not pause typing');
  }
 }finally{await browser?.close();await server.close()}
});
