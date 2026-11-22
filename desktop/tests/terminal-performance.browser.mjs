import {test} from 'node:test';import assert from 'node:assert/strict';
import {createRequire} from 'node:module';import {createServer} from 'vite';import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';import {resolve} from 'node:path';import {readFileSync,mkdirSync} from 'node:fs';
import exportHelper from '../electron/terminal-performance-export.cjs';
const {chromium}=createRequire(import.meta.url)('playwright'),root=fileURLToPath(new URL('..',import.meta.url));
const html='<html><body><div id="actions"></div><div id="root" style="width:900px;height:550px;display:flex"></div><script type="module" src="/__performance-entry.tsx"></script></body></html>';
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.addEventListener('velella:terminal-created',e=>window.term=e.detail.terminal);createRoot(document.getElementById('root')).render(<TooltipProvider><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id:'fixture',status:{exit:null,input_protocol:1}}}/></TooltipProvider>);`;
test('terminal exports queue, transport, parse and frame timing without user content',async()=>{
 const server=await createServer({root,configFile:false,plugins:[react(),{name:'timing-fixture',configureServer(s){s.middlewares.use((req,res,next)=>{
  if(req.url==='/__performance'){void s.transformIndexHtml(req.url,html).then(v=>{res.setHeader('Content-Type','text/html');res.end(v)});return}next();
 })},resolveId:id=>id==='/__performance-entry.tsx'?id:null,load:id=>id==='/__performance-entry.tsx'?entry:null}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser;
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({acceptDownloads:true});
  await page.addInitScript(()=>localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'})));
  let output=Buffer.from('PS fixture> ');const sent=[],errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/_velella',async route=>{
   const q=route.request().postDataJSON();let r={ok:true};
   if(q.cmd.endsWith('terminal.input')){await new Promise(resolve=>setTimeout(resolve,30));sent.push(q.text);output=Buffer.concat([output,Buffer.from(q.text==='\r'?'\r\nREADY':q.text)])}
   if(q.cmd.endsWith('terminal.output')){const from=Math.min(q.since??0,output.length);r={ok:true,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:90,rows:25,sequence:1}]:[]}}
   await route.fulfill({json:r});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__performance`);
  const input=page.locator('.xterm-helper-textarea');await input.focus();await input.pressSequentially('PRIVATE_COMMAND');await input.press('Enter');
  await page.waitForFunction(()=>Array.from({length:term.buffer.active.length},(_,i)=>term.buffer.active.getLine(i)?.translateToString(true)).join('').includes('READY'));
  await input.evaluate(element=>{element.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));element.value='你';element.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'你'}));element.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'你'}))});
  await page.waitForTimeout(150);
  await page.locator('.xterm-screen').click({button:'right',modifiers:['Control']});
  const downloaded=page.waitForEvent('download');await page.getByRole('menuitem',{name:'导出终端性能记录'}).click();
  const download=await downloaded;const data=readFileSync(await download.path(),'utf8'),report=JSON.parse(data);
  const stages=new Set(report.entries.map(entry=>entry.stage));
  for(const stage of ['native_input','composition_start','composition_end','input_queued','input_send','input_ack','output_read','output_parse','render_callback','paint_opportunity'])assert.ok(stages.has(stage),stage);
  assert.ok(report.entries.some(entry=>entry.stage==='input_send'&&entry.queueMs>=30));
  assert.ok(report.entries.some(entry=>entry.kind==='enter'));assert.ok(!data.includes('PRIVATE_COMMAND'));assert.ok(!data.includes('READY'));assert.ok(!data.includes('你'));
  assert.equal(sent.join(''),'PRIVATE_COMMAND\r你');assert.deepEqual(errors,[]);
  const out=process.env.TERMINAL_INPUT_OUTPUT||resolve(root,'../artifacts/terminal-input-performance');mkdirSync(out,{recursive:true});await download.saveAs(resolve(out,'performance-export.json'));
  // Exercise the desktop branch with the real file writer and observable UI receipt.
  let savedPath,failSave=false;
  await page.exposeFunction('savePerformance',async snapshot=>{
   if(failSave)throw new Error('fixture disk denied');
   const saved=await exportHelper.exportTerminalPerformance(snapshot,out);savedPath=saved.path;return saved;
  });
  await page.evaluate(()=>{window.velellaDesktop={invoke:async(command,args)=>{
   if(command==='terminal_performance_export')return window.savePerformance(args.snapshot);
   if(command==='rpc')return (await fetch('/_velella',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(args.request)})).json();
   throw new Error(command);
  },listen:()=>()=>{}}});
  await page.locator('.xterm-screen').click({button:'right',modifiers:['Control']});
  await page.getByRole('menuitem',{name:'导出终端性能记录'}).click();
  await page.getByRole('status').filter({hasText:'已导出到：'}).waitFor();
  assert.ok((await page.getByRole('status').filter({hasText:'已导出到：'}).textContent()).includes(savedPath));
  assert.equal(JSON.parse(readFileSync(savedPath,'utf8')).sessionId,'fixture');
  failSave=true;
  await page.locator('.xterm-screen').click({button:'right',modifiers:['Control']});
  await page.getByRole('menuitem',{name:'导出终端性能记录'}).click();
  await page.getByRole('status').filter({hasText:'导出失败：'}).waitFor();
  assert.equal(await page.getByRole('status').filter({hasText:'已导出到：'}).count(),0);
 }finally{await browser?.close();await server.close()}
});
