import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
const require=createRequire(import.meta.url),{chromium}=require('playwright'),{PNG}=require('pngjs');
const root=fileURLToPath(new URL('..',import.meta.url));
const output=process.env.CURSOR_QA_OUTPUT||resolve(root,'../artifacts/terminal-cursor');
const requested=(process.env.CURSOR_QA_SCENARIOS||'dom:1,dom:1.5,dom:2,webgl:1,webgl:1.5,webgl:2').split(',');
assert(requested.length>0&&requested.every(value=>/^(dom|webgl):(1|1\.5|2)$/.test(value)),'Invalid scenario selection');
const html='<!doctype html><html><body><button id="outside">Outside terminal</button><div id="root"></div><script type="module" src="/__cursor-entry.tsx"></script></body></html>';
const entry=`import React,{useState} from 'react';import {createRoot} from 'react-dom/client';
import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';
import {AppearanceProvider,useAppearance} from '/src/Appearance';
import {readTerminalPreferences,saveTerminalPreferences} from '/src/terminal-preferences';
import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';
window.terms={};window.created=0;
window.addEventListener('velella:terminal-created',e=>{window.terms[e.detail.sessionId]=e.detail.terminal;window.created++});
window.preferences=patch=>saveTerminalPreferences({...readTerminalPreferences(),...patch});
function App(){const [active,setActive]=useState('one'),{setTheme}=useAppearance();window.switchTab=setActive;window.setTheme=setTheme;
return <>{['one','two'].map(id=><div key={id} data-session={id} style={{display:active===id?'flex':'none',width:800,height:360,margin:20}}>
<TerminalView visible={active===id} workspace={{projectId:'fixture',workspaceId:'fixture',id,program:'powershell',status:{exit:null,input_protocol:1}}}/></div>)}</>}
const root=createRoot(document.getElementById('root'));window.unmount=()=>root.unmount();
root.render(<AppearanceProvider><TooltipProvider><App/></TooltipProvider></AppearanceProvider>);`;
let escapedRpc=0,browser,server;
const results=[],failures=[],samples=[];
function coverage(png,dark){
 const decoded=PNG.sync.read(png);let count=0;
 // The selected mint cursor averages 197; distinguish its filled cell from the
 // gray-green background (37) without requiring the former pure-white cursor.
 for(let i=0;i<decoded.data.length;i+=4){const avg=(decoded.data[i]+decoded.data[i+1]+decoded.data[i+2])/3;if(dark?avg>150:avg<100)count++}
 return count/(decoded.width*decoded.height);
}
async function cursorClip(page){
 return page.evaluate(()=>{const t=window.terms.one,renderer=t._core._renderService._renderer.value,screen=t.element.querySelector('.xterm-screen').getBoundingClientRect(),canvas=renderer._canvas?.getBoundingClientRect(),cached=t._core._renderService.dimensions.css.cell;
 const bounds=canvas||screen,cell=canvas?{width:canvas.width/t.cols,height:canvas.height/t.rows}:cached,row=t.buffer.active.baseY+t.buffer.active.cursorY-t.buffer.active.viewportY;
 return {clip:{x:bounds.x+t.buffer.active.cursorX*cell.width,y:bounds.y+row*cell.height,width:cell.width,height:cell.height},geometry:{screen:{x:screen.x,y:screen.y,width:screen.width,height:screen.height},canvas:canvas?{x:canvas.x,y:canvas.y,width:canvas.width,height:canvas.height}:null,cached:{...cached},renderedCell:cell,cols:t.cols,rows:t.rows,row,cursorX:t.buffer.active.cursorX,dpr:devicePixelRatio,documentFocused:document.hasFocus(),terminalFocused:document.activeElement===t.textarea}}});
}
async function sample(page,label,dark=true){
 const frames=[],geometry=[];
 for(let i=0;i<8;i++){const measured=await cursorClip(page);geometry.push(measured.geometry);const png=await page.screenshot({clip:measured.clip,animations:'allow'});frames.push({ratio:coverage(png,dark),png});await page.waitForTimeout(180)}
 const ordered=[...frames].sort((a,b)=>a.ratio-b.ratio);
 await writeFile(join(output,label+'-off.png'),ordered[0].png);
 await writeFile(join(output,label+'-on.png'),ordered.at(-1).png);
 samples.push({label,ratios:frames.map(x=>x.ratio),geometry});
 return frames.map(x=>x.ratio);
}
function blinking(ratios,label){assert(Math.min(...ratios)<.08,label+' must visibly turn off');assert(Math.max(...ratios)>.55,label+' must visibly fill the cell')}
try{
 await mkdir(output,{recursive:true});
 server=await createServer({root,configFile:false,logLevel:'error',plugins:[react(),{
  name:'cursor-fixture',configureServer(s){s.middlewares.use((req,res,next)=>{
   if(req.url==='/__cursor'){void s.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return}
   if(req.url==='/__cursor-entry.tsx'){void s.transformRequest(req.url).then(value=>{res.setHeader('Content-Type','text/javascript');res.end(value.code)});return}
   if(req.url==='/_velella'){escapedRpc++;res.statusCode=500;res.end('Fixture RPC only');return}next();
  })},resolveId:id=>id==='/__cursor-entry.tsx'?id:null,load:id=>id==='/__cursor-entry.tsx'?entry:null
 }],server:{host:'127.0.0.1',port:0,fs:{allow:[root,'D:/orcas']}}});
 await server.listen();
 browser=await chromium.launch({channel:'msedge',headless:true,args:['--enable-unsafe-swiftshader']});
 for(const renderer of ['dom','webgl'])for(const scale of [1,1.5,2]){
  if(!requested.includes(renderer+':'+scale))continue;
  const label=renderer+'-'+scale,context=await browser.newContext({viewport:{width:1100,height:650},deviceScaleFactor:scale});
  const page=await context.newPage();page.setDefaultTimeout(10000);
  const errors=[],requests=[],sent=[];
  page.on('pageerror',e=>errors.push(e.message));
  let bytes=Buffer.from(Array.from({length:40},(_,i)=>'history '+i+' 中文\r\n').join('')+'PS fixture> ');
  await page.addInitScript(renderer=>localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer,fontSize:14})),renderer);
  await page.route('**/_velella',async route=>{
   const r=route.request().postDataJSON();requests.push(r);let response={ok:true};
   if(r.cmd.endsWith('.output')){const from=Math.min(r.since??0,bytes.length);response={ok:true,from,end:bytes.length,total:bytes.length,data:bytes.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:90,rows:25,sequence:1}]:[]}}
   if(r.cmd.endsWith('.input'))sent.push(r.text);
   await route.fulfill({json:response});
  });
  try{
   await page.goto(server.resolvedUrls.local[0]+'__cursor');
   await page.waitForFunction(()=>window.terms.one?.buffer.active.getLine(window.terms.one.buffer.active.baseY+window.terms.one.buffer.active.cursorY)?.translateToString(true)==='PS fixture> ');
   await page.waitForFunction(renderer=>document.querySelector('[data-session=one] [data-terminal-renderer]')?.dataset.terminalRenderer===renderer,renderer);
   const textarea=page.locator('[data-session=one] .xterm-helper-textarea');await page.bringToFront();await textarea.focus();
   await page.waitForFunction(()=>document.hasFocus()&&document.activeElement===window.terms.one.textarea);
   const defaultOptions=await page.evaluate(()=>({style:window.terms.one.options.cursorStyle,blink:window.terms.one.options.cursorBlink,inactive:window.terms.one.options.cursorInactiveStyle,theme:window.terms.one.options.theme}));
   assert.equal(defaultOptions.style,'block');assert.equal(defaultOptions.blink,true);assert.equal(defaultOptions.inactive,'outline');
   assert.equal(defaultOptions.theme.cursor,'#a7d7bd');assert.equal(defaultOptions.theme.cursorAccent,defaultOptions.theme.background);
   const dark=await sample(page,label+'-dark');blinking(dark,label+' dark');
   await page.locator('#outside').focus();await page.waitForTimeout(150);
   const outline=await sample(page,label+'-outline');
   assert(Math.min(...outline)>.02&&Math.max(...outline)<.5,label+' blurred cursor must be an outline');
   assert(Math.max(...outline)-Math.min(...outline)<.03,label+' outline must stay steady');
   await page.evaluate(()=>{window.identity=window.terms.one;window.createdBefore=window.created;window.historyBefore=Array.from({length:window.terms.one.buffer.active.length},(_,i)=>window.terms.one.buffer.active.getLine(i)?.translateToString(true)).join('\n');window.switchTab('two')});
   await page.waitForFunction(()=>window.terms.one.options.cursorBlink===false);
   await page.evaluate(()=>window.switchTab('one'));await textarea.focus();
   await page.waitForFunction(renderer=>document.querySelector('[data-session=one] [data-terminal-renderer]')?.dataset.terminalRenderer===renderer,renderer);
   const revealed=await sample(page,label+'-revealed');blinking(revealed,label+' revealed');
   assert.equal(await page.evaluate(()=>window.terms.one===window.identity&&window.created===window.createdBefore),true);
   assert.equal(await page.evaluate(()=>Array.from({length:window.terms.one.buffer.active.length},(_,i)=>window.terms.one.buffer.active.getLine(i)?.translateToString(true)).join('\n')===window.historyBefore),true);
   await page.evaluate(()=>window.preferences({cursorBlink:false}));await page.waitForTimeout(100);
   const steady=await sample(page,label+'-steady');assert(Math.min(...steady)>.55,'explicit steady preference must remain steady');
   await page.evaluate(()=>window.preferences({cursorBlink:true}));
   await page.evaluate(()=>new Promise(done=>window.terms.one.write('\x1b[2 q',done)));
   const appSteady=await sample(page,label+'-app-steady');assert(Math.min(...appSteady)>.55,'application steady request must override blinking default');
   await page.evaluate(()=>new Promise(done=>window.terms.one.write('\x1b[0 q',done)));
   const reset=await sample(page,label+'-app-reset');blinking(reset,'reset returns to user default');
   await page.evaluate(()=>window.setTheme('light'));await page.waitForFunction(()=>window.terms.one.options.theme.cursor==='#171717');
   await textarea.focus();const light=await sample(page,label+'-light',false);blinking(light,label+' light');
   await page.evaluate(()=>window.setTheme('dark'));await page.waitForFunction(()=>window.terms.one.options.theme.cursor==='#a7d7bd');
   await textarea.focus();
   await textarea.evaluate(e=>{e.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));e.value='ni';e.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'ni'}))});
   await page.waitForFunction(()=>document.querySelector('.terminal-ime-preedit')?.textContent.includes('ni'));
   const anchor=await textarea.boundingBox();await page.waitForTimeout(750);const after=await textarea.boundingBox();
   assert(Math.abs(after.x-anchor.x)<1&&Math.abs(after.y-anchor.y)<1,'blink must not move IME anchor');
   await textarea.evaluate(e=>{e.value='你';e.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'你'}))});
   await page.waitForTimeout(100);assert.deepEqual(sent,['你']);
   await textarea.press('Enter');await page.waitForTimeout(100);assert.deepEqual(sent,['你','\r']);
   bytes=Buffer.concat([bytes,Buffer.from('\r\n中文\r\n下一行> ')]);
   await page.waitForFunction(()=>{const t=window.terms.one;return t.buffer.active.getLine(t.buffer.active.baseY+t.buffer.active.cursorY)?.translateToString(true)==='下一行> '});
   assert.equal(await page.evaluate(()=>window.terms.one.buffer.active.cursorX),8,'CJK and newline keep the character grid');
   assert.equal(await page.evaluate(()=>window.terms.one===window.identity&&window.created===window.createdBefore),true);
   assert(!requests.some(r=>/\.(stop|recover|open|create)$/.test(r.cmd)),'no session lifecycle mutation');
   assert.deepEqual(errors,[]);
   results.push({renderer,scale,defaultOptions,dark,outline,revealed,steady,appSteady,reset,light,sessionRetained:true,historyRetained:true,syntheticIme:true,cjkNewline:true,errors});
   await page.screenshot({path:join(output,label+'-terminal.png')});
  }catch(error){failures.push({renderer,scale,error:String(error),errors});await page.screenshot({path:join(output,label+'-failure.png')}).catch(()=>{});throw error}
  finally{await page.evaluate(()=>window.unmount?.()).catch(()=>{});await page.waitForTimeout(100);await context.close()}
 }
 assert.equal(escapedRpc,0);
 assert.equal(results.length,requested.length);
}catch(error){console.error(error);process.exitCode=1}
finally{
 await browser?.close();await server?.close();
 await writeFile(join(output,'result.json'),JSON.stringify({status:process.exitCode?'failed':'passed',scope:'Actual TerminalView in isolated Edge; mocked runtime and synthetic IME, not native PowerShell or OS IME UAT.',requested,results,failures,samples,escapedRpc},null,2));
}
if(!process.exitCode)console.log('Passed cursor rendering, focus, visibility, override and IME checks for '+results.length+' renderer/DPI scenarios.');
