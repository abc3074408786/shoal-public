import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import {fileURLToPath} from 'node:url';
import {mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const entry=`import {Terminal} from '@xterm/xterm';import {createTerminalFrameStore} from '/src/terminal-frames';
window.makeFrames=async(styled)=>{
 const term=new Terminal({cols:180,rows:48,allowProposedApi:true,scrollback:0});
 const store=createTerminalFrameStore();
 for(let frame=0;frame<200;frame++){
  const rows=Array.from({length:48},(_,r)=>Array.from({length:180},(_,c)=>(styled?'\\x1b['+(31+(c%6))+'m':'')+String.fromCharCode(65+(frame+r+c)%26)).join(''));
  await new Promise(done=>term.write('\\x1b[H'+rows.join('\\r\\n'),done));store.capture(term,frame);
 }
 term.dispose();window.memoryStore=store;
 const snapshot=store.snapshot();return {frames:snapshot.frames.length,dropped:snapshot.dropped,ansiCharacters:snapshot.frames.reduce((n,f)=>n+f.ansi.length,0)};
};window.ready=true;`;

test('terminal history retains compact bounded data instead of per-cell object graphs',async()=>{
 const server=await createServer({root,configFile:false,logLevel:'error',plugins:[{name:'memory-fixture',resolveId:id=>id==='/__memory.js'?id:null,load:id=>id==='/__memory.js'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url!=='/__memory')return next();res.setHeader('Content-Type','text/html');res.end('<script type="module" src="/__memory.js"></script>')})}}],server:{host:'127.0.0.1',port:0}});
 let browser;const results=[];
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});
  for(const styled of [false,true]){
   const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__memory`);await page.waitForFunction(()=>window.ready);
   const cdp=await page.context().newCDPSession(page);
   const heap=async()=>{await cdp.send('HeapProfiler.collectGarbage');return (await cdp.send('Runtime.getHeapUsage')).usedSize};
   // Warm xterm caches before measuring retained frame-store data.
   await page.evaluate(()=>window.makeFrames(false));await page.evaluate(()=>window.memoryStore=null);const baseline=await heap();
   const stats=await page.evaluate(styled=>window.makeFrames(styled),styled);const retained=(await heap())-baseline;
   results.push({styled,...stats,retainedBytes:retained,retainedMiB:retained/1024/1024});await page.close();
  }
  const out=resolve(root,'../artifacts/terminal-memory-20261027');mkdirSync(out,{recursive:true});writeFileSync(resolve(out,`${process.env.MEMORY_LABEL||'current'}-heap.json`),JSON.stringify(results,null,2));console.log(JSON.stringify(results));
  for(const result of results)assert.ok(result.retainedBytes<12*1024*1024,`${result.styled?'styled':'plain'} frame history retains ${result.retainedMiB.toFixed(1)} MiB for <=4 Mi characters`);
 }finally{await browser?.close();await server.close()}
});
