import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const output=process.env.TERMINAL_LONG_OUTPUT||resolve(root,'../artifacts/terminal-long-session-20261028');
const entry=`import React,{useState} from 'react';import {createRoot} from 'react-dom/client';import {Terminal} from '@xterm/xterm';import PersistentTerminalView,{TerminalSessionProvider,useRetainedTerminals} from '/src/TerminalSessions';import {TooltipProvider} from '/src/ui';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';import '/src/workspace.css';
window.metrics={created:0,disposed:0};window.terms=new Map();
const open=Terminal.prototype.open,dispose=Terminal.prototype.dispose;
Terminal.prototype.open=function(el){window.metrics.created++;open.call(this,el)};
Terminal.prototype.dispose=function(){window.metrics.disposed++;window.terms.delete(this.testId);return dispose.call(this)};
window.addEventListener('velella:terminal-created',e=>{e.detail.terminal.testId=e.detail.sessionId;window.terms.set(e.detail.sessionId,new WeakRef(e.detail.terminal))});
window.term=id=>window.terms.get(id)?.deref();
function Harness(){const [ids,setIds]=useState([]),[active,setActive]=useState(null),[actions,setActions]=useState(null);
window.setSessions=next=>{setIds(next);setActive(next[0]||null)};window.selectSession=setActive;
useRetainedTerminals('stress',ids);
return <><div id="tabs">{ids.map(id=><button key={id} onClick={()=>setActive(id)}>Select {id}</button>)}</div><div ref={setActions} id="actions"/><div id="viewer" data-active={active||''} style={{width:1100,height:640,display:'flex'}}>{active&&<PersistentTerminalView actionsTarget={actions} workspace={{projectId:'fixture',workspaceId:'stress',id:active,status:{pid:100,exit:null,input_protocol:1}}}/>}</div></>}
createRoot(document.getElementById('root')).render(<TooltipProvider><TerminalSessionProvider><Harness/></TerminalSessionProvider></TooltipProvider>);`;

test('long output and repeated retained-tab lifecycles remain bounded', {timeout:180000},async()=>{
 const result={renderer:'dom',volume:[],lifecycles:[],limits:'Finite volume/lifecycle stress in a browser; not an hours-long soak, process RSS or GPU/VRAM measurement.'};
 mkdirSync(output,{recursive:true});const save=()=>writeFileSync(resolve(output,'result.json'),JSON.stringify(result,null,2));
 const server=await createServer({root,configFile:false,cacheDir:resolve(root,'node_modules/.vite-terminal-long-session'),plugins:[react(),{
  name:'long-session-fixture',resolveId:id=>id==='/__long.tsx'?id:null,load:id=>id==='/__long.tsx'?entry:null,
  configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__long'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__long.tsx"></script></body></html>').then(v=>{res.setHeader('Content-Type','text/html');res.end(v)});return}next()})},
 }],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser;
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1200,height:800}});
  await page.addInitScript(()=>localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false})));
  const buffers=new Map(),consumed=new Map(),zeroReads=new Map(),errors=[],sent=[];
  page.on('pageerror',e=>errors.push(e.message));
  const line=n=>`ROW-${String(n).padStart(6,'0')}-中文-${'0123456789'.repeat(5)}`;
  const block=(start,count)=>Buffer.from(Array.from({length:count},(_,i)=>`\x1b[3${i%6+1}m${line(start+i)}\x1b[0m\r\n`).join('')+`MARK-${start+count-1}\r\n`);
  await page.route('**/_velella',async route=>{
   const q=route.request().postDataJSON(),id=q.session_id;let r={ok:true};
   if(q.cmd.endsWith('terminal.output')){
    const data=buffers.get(id)||Buffer.alloc(0),from=Math.min(q.since??0,data.length),end=Math.min(from+128*1024,data.length);
    if(from===0)zeroReads.set(id,(zeroReads.get(id)||0)+1);
    consumed.set(id,end);
    r={ok:true,from,end,total:data.length,data:data.subarray(from,end).toString('base64'),geometry:from===0?[{offset:0,cols:120,rows:40,sequence:1}]:[]};
   }
   if(q.cmd.endsWith('terminal.input')){sent.push({id,text:q.text});if(q.text==='\r')buffers.set(id,Buffer.concat([buffers.get(id)||Buffer.alloc(0),Buffer.from('INPUT-OK\r\n')]))}
   await route.fulfill({json:r});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__long`);await page.waitForFunction(()=>typeof window.setSessions==='function');
  const cdp=await page.context().newCDPSession(page);
  const settle=()=>page.evaluate(()=>new Promise(done=>requestAnimationFrame(()=>requestAnimationFrame(done))));
  async function heap(){await settle();await cdp.send('HeapProfiler.collectGarbage');await page.waitForTimeout(100);await cdp.send('HeapProfiler.collectGarbage');const h=await cdp.send('Runtime.getHeapUsage'),d=await cdp.send('Memory.getDOMCounters');assert.equal(typeof h.backingStorageSize,'number','typed-array backing storage measurement is required');return {heapBytes:h.usedSize,backingStorageBytes:h.backingStorageSize,embedderHeapBytes:h.embedderHeapUsedSize??null,nodes:d.nodes,listeners:d.jsEventListeners}}
  const lines=id=>page.evaluate(id=>{const t=window.term(id);return Array.from({length:t.buffer.active.length},(_,i)=>t.buffer.active.getLine(i)?.translateToString(true))},id);
  async function caughtUp(id,marker){await page.waitForFunction(({id,marker})=>{const t=window.term(id);return t&&Array.from({length:Math.min(5,t.buffer.active.length)},(_,i)=>t.buffer.active.getLine(t.buffer.active.length-1-i)?.translateToString(true)).includes(marker)},{id,marker},{timeout:20000});assert.equal(consumed.get(id),buffers.get(id).length)}
  async function mount(ids){await page.evaluate(ids=>window.setSessions(ids),ids);await page.locator('#viewer').filter({has:page.locator('.xterm')}).waitFor()}
  async function closeAll(){await page.evaluate(()=>window.setSessions([]));await page.waitForFunction(()=>metrics.created===metrics.disposed&&terms.size===0);await settle()}
  // Warm the same portal/button topology used by the lifecycle samples. A
  // single-tab baseline misclassifies React's first multi-portal high-water mark.
  const warmIds=Array.from({length:4},(_,i)=>`warm-${i}`);for(const id of warmIds)buffers.set(id,block(1,2000));await mount(warmIds);
  for(const id of warmIds){await page.getByRole('button',{name:`Select ${id}`,exact:true}).click();await caughtUp(id,'MARK-2000')}
  for(let i=0;i<8;i++){await page.getByRole('button',{name:`Select ${warmIds[i%4]}`,exact:true}).click();await settle()}
  await closeAll();buffers.clear();
  result.baseline=await heap();save();
  buffers.set('long',block(1,25000));const initial=Date.now();await mount(['long']);await caughtUp('long','MARK-25000');
  for(let batch=1;batch<=4;batch++){
   const start=batch===1?initial:Date.now();
   if(batch>1){buffers.set('long',Buffer.concat([buffers.get('long'),block((batch-1)*25000+1,25000)]));await caughtUp('long',`MARK-${batch*25000}`)}
   const drainMs=Date.now()-start,total=batch*25000,content=await lines('long');
   assert.deepEqual(content.filter(s=>s?.startsWith('ROW-')).slice(-20),Array.from({length:20},(_,i)=>line(total-19+i)));
   const dimensions=await page.evaluate(()=>({length:term('long').buffer.active.length,rows:term('long').rows,scrollback:term('long').options.scrollback}));
   assert.ok(dimensions.length<=dimensions.scrollback+dimensions.rows);
   const memory=await heap();result.volume.push({lines:total,bytes:buffers.get('long').length,drainMs,...dimensions,...memory});save();
  }
  // Real toolbar search, wheel scrolling and input after the retained buffer is full.
  await page.getByRole('button',{name:'搜索终端',exact:true}).click();const search=page.getByRole('textbox',{name:'查找终端文本',exact:true});
  const searchStart=Date.now();await search.fill('ROW-099900');await search.press('Enter');await page.waitForFunction(()=>term('long').getSelection()==='ROW-099900');
  result.searchMs=Date.now()-searchStart;await page.getByRole('button',{name:'关闭终端搜索',exact:true}).click();
  await page.locator('.xterm-screen').hover();const viewportBefore=await page.evaluate(()=>term('long').buffer.active.viewportY);
  const scrollStart=Date.now();await page.mouse.wheel(0,-1200);await settle();result.scrollMs=Date.now()-scrollStart;
  const viewportAfter=await page.evaluate(()=>term('long').buffer.active.viewportY);result.scrollViewport={before:viewportBefore,after:viewportAfter};
  assert.ok(viewportAfter<viewportBefore,'wheel action must actually move the retained viewport upward');
  const input=page.locator('.xterm-helper-textarea');await input.focus();await input.pressSequentially('ok');const echoStart=Date.now();await input.press('Enter');await caughtUp('long','INPUT-OK');result.echoMs=Date.now()-echoStart;
  assert.equal(sent.filter(s=>s.id==='long').map(s=>s.text).join(''),'ok\r');
  await closeAll();buffers.clear();result.afterVolumeClose=await heap();save();
  for(let cycle=0;cycle<3;cycle++){
   const ids=Array.from({length:4},(_,i)=>`cycle-${cycle}-${i}`);for(const id of ids)buffers.set(id,block(1,12000));
   await mount(ids);
   for(const id of ids){await page.getByRole('button',{name:`Select ${id}`,exact:true}).click();await caughtUp(id,'MARK-12000')}
   const created=await page.evaluate(()=>metrics.created),switchMs=[];
   for(let i=0;i<24;i++){const id=ids[i%4],start=Date.now();await page.getByRole('button',{name:`Select ${id}`,exact:true}).click();await page.waitForFunction(id=>document.querySelector('#viewer').dataset.active===id,id);await settle();switchMs.push(Date.now()-start)}
   assert.equal(await page.evaluate(()=>metrics.created),created,'switches must retain existing terminals');
   for(const id of ids)assert.equal(zeroReads.get(id),1,'cached switches must not reread history from zero');
   await closeAll();for(const id of ids)buffers.delete(id);
   const memory=await heap();result.lifecycles.push({cycle:cycle+1,tabs:4,switches:24,switchMedianMs:switchMs.sort((a,b)=>a-b)[12],switchMaxMs:Math.max(...switchMs),...memory,instances:await page.evaluate(()=>({...metrics}))});save();
  }
  const memoryGrowth=result.volume.at(-1).heapBytes-result.volume[1].heapBytes;
  result.plateauGrowthBytes=memoryGrowth;assert.ok(memoryGrowth<12*1024*1024,'retained heap keeps growing after scrollback saturation');
  result.backingPlateauGrowthBytes=result.volume.at(-1).backingStorageBytes-result.volume[1].backingStorageBytes;
  assert.ok(result.backingPlateauGrowthBytes<12*1024*1024,'typed-array backing storage grows after saturation');
  for(const cycle of result.lifecycles)assert.ok(cycle.backingStorageBytes-result.baseline.backingStorageBytes<12*1024*1024,'closed terminals retain typed-array buffers');
  for(const cycle of result.lifecycles){assert.ok(cycle.heapBytes-result.baseline.heapBytes<12*1024*1024,'closed tabs retain excessive heap');assert.ok(cycle.nodes-result.baseline.nodes<500,'detached DOM accumulates');assert.ok(cycle.listeners-result.baseline.listeners<50,'listeners accumulate')}
  assert.ok(result.searchMs<1000&&result.scrollMs<1000&&result.echoMs<1000,'post-volume interaction exceeds budget');assert.deepEqual(errors,[]);
  result.status='passed';save();console.log(JSON.stringify(result));
 }catch(error){result.status='failed';result.error=String(error);save();throw error}
 finally{await browser?.close();await server.close()}
});
