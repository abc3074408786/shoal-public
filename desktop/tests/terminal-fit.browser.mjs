import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url)),output=process.env.TERMINAL_FIT_OUTPUT||resolve(root,'../artifacts/terminal-reply-parity-20261028/fit');
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';import {AppearanceProvider} from '/src/Appearance';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.addEventListener('velella:terminal-created',e=>window.term=e.detail.terminal);createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id:'fit',program:'powershell',status:{exit:null,input_protocol:1}}}/></TooltipProvider></AppearanceProvider>);`;
const html='<html><body style="margin:0"><div id="root" style="height:100vh;display:flex"></div><script type="module" src="/__fit.tsx"></script></body></html>';
let browser;
const server=await createServer({root,configFile:false,plugins:[react(),{name:'fit-regression',configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/fit'){void s.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return}next()})},resolveId(id){if(id==='/__fit.tsx')return id},load(id){if(id==='/__fit.tsx')return entry}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
const results=[];
try{
 await server.listen();browser=await chromium.launch({channel:'msedge',headless:true,args:['--enable-unsafe-swiftshader']});mkdirSync(output,{recursive:true});
 for(const renderer of ['dom','webgl'])for(const scale of [1,1.5,2]){
  const context=await browser.newContext({viewport:{width:1440,height:900},deviceScaleFactor:scale}),page=await context.newPage();let sequence=0,geometry=[{offset:0,rows:24,cols:80,reset:true,sequence:0}],inputs=0;
  await page.addInitScript(renderer=>localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer,fontSize:14,lineHeight:1.16})),renderer);
  const bytes=Buffer.from('NATIVE_TERMINAL_READY\r\n');
  await page.route('**/_velella',async route=>{const q=route.request().postDataJSON();let response={ok:true};
   if(q.cmd.endsWith('.output')){const from=Math.min(q.since||0,bytes.length);response={...response,from,end:bytes.length,total:bytes.length,data:bytes.subarray(from).toString('base64'),geometry}}
   if(q.cmd.endsWith('.resize')){geometry.push({offset:bytes.length,rows:q.rows,cols:q.cols,reset:false,sequence:++sequence})}
   if(q.cmd.endsWith('.input'))inputs++;
   await route.fulfill({json:response});
  });
  await page.goto('http://127.0.0.1:'+server.httpServer.address().port+'/fit');
  await page.waitForFunction(()=>window.term?.rows>24);await page.waitForTimeout(180);
  for(const viewport of [{width:1440,height:900},{width:900,height:600}]){
   await page.setViewportSize(viewport);await page.waitForTimeout(250);
   const measured=await page.evaluate(()=>{const t=window.term,canvas=document.querySelector('.terminal-canvas'),box=canvas.getBoundingClientRect(),screen=document.querySelector('.xterm-screen').getBoundingClientRect(),style=getComputedStyle(canvas);return {rows:t.rows,cols:t.cols,screenBottom:screen.bottom,availableBottom:box.bottom-parseFloat(style.paddingBottom),screenRight:screen.right,availableRight:box.right-parseFloat(style.paddingRight),renderer:canvas.dataset.terminalRenderer}});
   results.push({requestedRenderer:renderer,scale,viewport,...measured});writeFileSync(resolve(output,'result.json'),JSON.stringify(results,null,2));
   assert.ok(measured.screenBottom<=measured.availableBottom+1,`native footer clipped: ${JSON.stringify(results.at(-1))}`);
   assert.ok(measured.screenRight<=measured.availableRight+1,`native right edge clipped: ${JSON.stringify(results.at(-1))}`);
  }
  assert.equal(inputs,0,'fitting must not send user or historical protocol input');await context.close();
 }
 console.log(JSON.stringify({passed:true,scenarios:results.length,output}));
}finally{await browser?.close();await server.close()}
