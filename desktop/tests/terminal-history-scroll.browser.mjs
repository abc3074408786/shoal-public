import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {writeFileSync,mkdirSync,readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import assert from 'node:assert/strict';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url)),artifacts=resolve(root,'../../browser-'+Date.now());mkdirSync(artifacts);
const html='<html><body><div id="root" style="margin:24px;width:900px;height:550px;display:flex"></div><script type="module" src="/__scroll-entry.tsx"></script></body></html>';
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';
import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';
window.addEventListener('velella:terminal-created',e=>window.term=e.detail.terminal);
createRoot(document.getElementById('root')).render(<TooltipProvider><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id:'history',status:{exit:0,input_protocol:1}}}/></TooltipProvider>);`;
const server=await createServer({root,configFile:false,plugins:[react(),{name:'history-scroll-fixture',configureServer(server){server.middlewares.use((req,res,next)=>{
 if(req.url==='/__scroll'){void server.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return}
 if(req.url==='/__scroll-entry.tsx'){void server.transformRequest(req.url).then(value=>{res.setHeader('Content-Type','text/javascript');res.end(value.code)});return}next();
})},resolveId(id){if(id==='/__scroll-entry.tsx')return id},load(id){if(id==='/__scroll-entry.tsx')return entry}}],server:{host:'127.0.0.1',port:0,fs:{allow:[resolve(root,'../../..')]}},logLevel:'error'});
let browser;const results=[];
try{
 await server.listen();browser=await chromium.launch({channel:'msedge',headless:true,args:['--enable-unsafe-swiftshader']});
 for(const renderer of ['dom','webgl']){
  const context=await browser.newContext({viewport:{width:1280,height:850},deviceScaleFactor:1.5});const page=await context.newPage();page.setDefaultTimeout(10000);
  await page.addInitScript(renderer=>localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer,cursorBlink:false,fontSize:14})),renderer);
  const recorded=process.argv.includes('--recorded');
  const output=recorded?readFileSync(resolve(root,'../../history-fixture/terminal.bin')):Buffer.from(Array.from({length:1000},(_,i)=>`\x1b[32mHISTORY_${String(i).padStart(4,'0')} 中文测试 ${'x'.repeat(210)}\x1b[0m\r\n`).join(''));
  let geometry=[{offset:0,cols:240,rows:70,sequence:0}];
  if(recorded){const bytes=readFileSync(resolve(root,'../../history-fixture/terminal.geometry'));geometry=[];for(let i=0;i<bytes.length;i+=16)geometry.push({offset:Number(bytes.readBigUInt64LE(i)),rows:bytes.readUInt16LE(i+8),cols:bytes.readUInt16LE(i+10),reset:bytes.readUInt32LE(i+12)===1,sequence:i/16})}
  if(process.argv.includes('--ignore-tiny'))geometry=geometry.filter(g=>g.cols>2&&g.rows>1);
  await page.route('**/_velella',async route=>{const q=route.request().postDataJSON();const from=Math.min(q.since||0,output.length),end=Math.min(from+65536,output.length);const previous=geometry.filter(g=>g.offset<=from).at(-1);const events=[...(previous?[{...previous,reset:previous.offset===from&&previous.reset}]:[]),...geometry.filter(g=>g.offset>from&&g.offset<=end)];await route.fulfill({json:q.cmd.endsWith('terminal.output')?{ok:true,from,end,total:output.length,data:output.subarray(from,end).toString('base64'),geometry:events}:{ok:true}})});
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__scroll`);
  await page.waitForFunction(()=>window.term?.buffer.active.baseY>500&&document.querySelector('.terminal-canvas').dataset.terminalOffset===document.querySelector('.terminal-canvas').dataset.terminalTotal);
  await page.waitForTimeout(150);
  async function inspect(label){const row=await page.evaluate(label=>{const t=window.term,c=t.element.parentElement;const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}};return {label,renderer:c.dataset.terminalRenderer,outer:{top:c.scrollTop,left:c.scrollLeft,height:c.clientHeight,scrollHeight:c.scrollHeight,width:c.clientWidth,scrollWidth:c.scrollWidth,rect:rect(c)},xterm:rect(t.element),screen:rect(t.element.querySelector('.xterm-screen')),scrollable:rect(t.element.querySelector('.xterm-scrollable-element')),rows:t.rows,cols:t.cols,baseY:t.buffer.active.baseY,viewportY:t.buffer.active.viewportY,firstLine:t.buffer.active.getLine(t.buffer.active.viewportY)?.translateToString(true),lastLine:t.buffer.active.getLine(t.buffer.active.viewportY+t.rows-1)?.translateToString(true)}},label);results.push(row);return row}
  await inspect(renderer+'-ready');
  const box=await page.locator('.terminal-canvas').boundingBox();await page.mouse.move(box.x+100,box.y+100);await page.mouse.wheel(0,-1000);await page.waitForTimeout(150);await inspect(renderer+'-wheel');
  await page.locator('.terminal-history').evaluate(e=>e.scrollTop=e.scrollHeight);await page.waitForTimeout(100);await inspect(renderer+'-outer-bottom');
  await page.screenshot({path:resolve(artifacts,renderer+'-outer-bottom.png')});
  if(recorded){
   const blank=await page.evaluate(()=>{const b=window.term.buffer.active;let longest={start:0,length:0},start=0,length=0,textRows=0;for(let i=0;i<b.length;i++){if(!b.getLine(i).translateToString(true).trim()){if(length===0)start=i;length++;if(length>longest.length)longest={start,length}}else{length=0;textRows++}}return {longest,textRows,total:b.length}});
   await page.locator('.terminal-history').evaluate(e=>e.scrollTop=0);await page.evaluate(line=>window.term.scrollToLine(line),blank.longest.start);await page.waitForTimeout(100);results.push({renderer,recorded:true,ignoreTiny:process.argv.includes('--ignore-tiny'),blank});await page.screenshot({path:resolve(artifacts,renderer+'-blank-run.png')});
   if(process.argv.includes('--verify')){
    assert.ok(blank.longest.length<10,'historical collapsed grids must not create screenfuls of empty rows');
    await page.getByText('已修正旧记录的异常窗口尺寸',{exact:true}).click();
    await page.getByRole('button',{name:'查看原始回放',exact:true}).click();
    await page.waitForFunction(()=>window.term?.buffer.active.baseY>3000);
    await page.getByText('正在按异常窗口尺寸原样回放',{exact:true}).click();
    await page.getByRole('button',{name:'修正异常尺寸',exact:true}).click();
    await page.waitForFunction(()=>window.term?.buffer.active.baseY>400&&window.term.buffer.active.baseY<1000&&document.querySelector('.terminal-canvas').dataset.terminalOffset===document.querySelector('.terminal-canvas').dataset.terminalTotal);
    assert.equal(await page.locator('.terminal-canvas').getAttribute('data-terminal-offset'),String(output.length));
    assert.equal(await page.evaluate(()=>window.term.cols),181);
    for(const size of [{width:1800,height:1100},{width:1100,height:700}]){
     await page.setViewportSize(size);
     await page.evaluate(size=>{const e=document.getElementById('root');e.style.width=(size.width-80)+'px';e.style.height=(size.height-80)+'px'},size);
     await page.waitForTimeout(80);
     const b=await page.locator('.terminal-canvas').boundingBox();await page.mouse.move(b.x+80,b.y+80);
     for(const delta of [-120,120,-500,500]){await page.mouse.wheel(0,delta);await page.waitForTimeout(60);assert.ok(await page.evaluate(()=>{const b=window.term.buffer.active;for(let i=b.viewportY;i<Math.min(b.length,b.viewportY+20);i++)if(b.getLine(i).translateToString(true).trim())return true;return false}),'wheel retains visible history text');}
    }
    results.push({renderer,verified:true,rawReplayToggle:true,bytesWritten:output.length,resizedAndWheeled:true});
   }
  }
  await context.close();
 }
 console.log(JSON.stringify({artifacts,results}));
}finally{writeFileSync(resolve(artifacts,'result.json'),JSON.stringify(results,null,2));await browser?.close();await server.close()}
