import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const html='<html><body><div id="root"></div><script type="module" src="/__background.tsx"></script></body></html>';
const entry=`import React,{useState} from 'react';import {createRoot} from 'react-dom/client';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.terms={};window.addEventListener('velella:terminal-created',e=>window.terms[e.detail.sessionId]=e.detail.terminal);function App(){const [shown,setShown]=useState(false);window.showBackground=setShown;return <TooltipProvider><div id="foreground" style={{display:'flex',width:900,height:450}}><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id:'foreground',status:{exit:null,input_protocol:1}}}/></div><div id="background" style={{display:shown?'flex':'none',width:900,height:450}}><TerminalView visible={shown} workspace={{projectId:'fixture',workspaceId:'fixture',id:'background',status:{exit:null,input_protocol:1}}}/></div></TooltipProvider>}createRoot(document.getElementById('root')).render(<App/>);`;

async function sample(baseline){
 const server=await createServer({root,configFile:false,plugins:[{
  name:'hidden-stream-fixture',enforce:'pre',
  configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__background'){void s.transformIndexHtml(req.url,html).then(v=>{res.setHeader('Content-Type','text/html');res.end(v)});return}next()})},
  resolveId:id=>id==='/__background.tsx'?id:null,
  load(id){if(id==='/__background.tsx')return entry;if(baseline&&id.replaceAll('\\','/').endsWith('/src/terminal-output.ts'))return baseline},
 },react()],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser;
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage();
  await page.addInitScript(()=>localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'})));
  const buffers={foreground:Buffer.from('FRONT\r\n'),background:Buffer.from('BEGIN\r\n')},sent={foreground:'',background:''},errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  let started,cutoff=Infinity,produced=0,hiddenReads=0,measuring=false;
  const line=i=>`line-${String(i).padStart(4,'0')}:中文✓`;
  function generate(){if(started===undefined)return;const count=Math.floor((Math.min(Date.now(),cutoff)-started)/20);while(produced<count){produced++;buffers.background=Buffer.concat([buffers.background,Buffer.from(`\x1b[32m${line(produced)}\x1b[0m\r\n`)])}}
  await page.route('**/_velella',async route=>{
   const q=route.request().postDataJSON(),id=q.session_id;let r={ok:true};
   if(q.cmd.endsWith('terminal.output')){
    if(id==='background'){if(started===undefined)started=Date.now();generate();if(measuring)hiddenReads++}
    const buffer=buffers[id],from=Math.min(q.since??0,buffer.length),end=Math.min(buffer.length,from+128*1024);
    r={ok:true,from,end,total:buffer.length,data:buffer.subarray(from,end).toString('base64'),geometry:from===0?[{offset:0,cols:90,rows:25,sequence:1}]:[]};
   }
   if(q.cmd.endsWith('terminal.input')){sent[id]+=q.text;if(q.text==='\r')buffers[id]=Buffer.concat([buffers[id],Buffer.from('ECHO:ok\r\n')])}
   await route.fulfill({json:r});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__background`);
  await page.waitForFunction(()=>window.terms.background?.buffer.active.getLine(0)?.translateToString(true)==='BEGIN');
  hiddenReads=0;measuring=true;cutoff=Date.now()+3000;await page.waitForTimeout(3000);measuring=false;generate();
  buffers.background=Buffer.concat([buffers.background,Buffer.from('END\r\n')]);
  const startedReveal=Date.now();await page.evaluate(()=>window.showBackground(true));
  await page.waitForFunction(()=>Array.from({length:terms.background.buffer.active.length},(_,i)=>terms.background.buffer.active.getLine(i)?.translateToString(true)).includes('END'));
  const revealMs=Date.now()-startedReveal;
  const history=await page.evaluate(()=>Array.from({length:terms.background.buffer.active.length},(_,i)=>terms.background.buffer.active.getLine(i)?.translateToString(true)).filter(Boolean));
  assert.deepEqual(history,['BEGIN',...Array.from({length:produced},(_,i)=>line(i+1)),'END'],'hidden batching must retain every ordered Unicode/ANSI line');
  const input=page.locator('#background .xterm-helper-textarea');await input.focus();await input.pressSequentially('ok');const inputStart=Date.now();await input.press('Enter');
  await page.waitForFunction(()=>Array.from({length:terms.background.buffer.active.length},(_,i)=>terms.background.buffer.active.getLine(i)?.translateToString(true)).includes('ECHO:ok'));
  const echoMs=Date.now()-inputStart;assert.equal(sent.background,'ok\r');assert.deepEqual(errors,[]);
  return {hiddenReads,measurementMs:3000,producedLines:produced,exactHistory:true,revealMs,echoMs};
 }finally{await browser?.close();await server.close()}
}

test('hidden active TerminalView batches reads and reveals complete history',async()=>{
 const baselinePath=process.env.TERMINAL_BACKGROUND_BASELINE;
 const baseline=baselinePath?await sample(readFileSync(baselinePath,'utf8')):undefined;
 const candidate=await sample();
 assert.ok(candidate.hiddenReads<=18,JSON.stringify(candidate));
 assert.ok(candidate.revealMs<1000&&candidate.echoMs<1000,JSON.stringify(candidate));
 if(baseline)assert.ok(candidate.hiddenReads<=baseline.hiddenReads*.25,JSON.stringify({baseline,candidate}));
 const out=process.env.TERMINAL_BACKGROUND_OUTPUT||resolve(root,'../artifacts/terminal-background-cadence');mkdirSync(out,{recursive:true});
 const result={baseline,candidate,reduction:baseline?1-candidate.hiddenReads/baseline.hiddenReads:undefined};
 writeFileSync(resolve(out,'browser-result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
});
