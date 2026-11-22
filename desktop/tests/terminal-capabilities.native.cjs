// A real Codex startup, through the actual runtime host and TerminalView.
// No task is submitted and no existing terminal is targeted.
const {createRequire}=require('node:module');
const {pathToFileURL}=require('node:url');
const {spawn}=require('node:child_process');
const fs=require('node:fs'),path=require('node:path'),net=require('node:net'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const root=path.resolve(__dirname,'..'),conpty=require('../scripts/conpty-runtime.cjs'),{chromium}=require('playwright');
const output=process.env.TERMINAL_CAPABILITIES_OUTPUT||path.resolve(root,'../artifacts/terminal-reply-parity-20261028/native');
const runtime=process.env.VELELLA_TEST_RUNTIME||path.resolve(root,'../target/release/velella-runtime.exe');
const codex=process.env.VELELLA_TEST_CODEX;
assert.equal(process.platform,'win32');assert.ok(codex,'VELELLA_TEST_CODEX must identify the fixed CLI binary');
conpty.verify(path.dirname(runtime));fs.mkdirSync(output,{recursive:true});
const session=fs.mkdtempSync(path.join(output,'session-')),endpoint='velella-capabilities-'+crypto.randomUUID();
const gate=path.join(session,'start'),descriptor={id:crypto.randomUUID(),endpoint,directory:session,spawn:{program:'powershell',args:['-NoLogo','-NoProfile','-Command','while (!(Test-Path -LiteralPath $env:VELELLA_TEST_START_GATE)) { Start-Sleep -Milliseconds 10 }; & $env:VELELLA_TEST_CODEX'],cwd:process.env.VELELLA_TEST_CWD||root,env:{VELELLA_TEST_START_GATE:gate,VELELLA_TEST_CODEX:codex},adapter:'generic',sandbox:{mode:'native'}}};
const launch=path.join(session,'launch.json');fs.writeFileSync(launch,JSON.stringify(descriptor));
function rpc(q){return new Promise((resolve,reject)=>{let text='';const socket=net.createConnection('\\\\.\\pipe\\'+endpoint);socket.setTimeout(10000);socket.on('connect',()=>socket.write(JSON.stringify(q)+'\n'));socket.on('timeout',()=>socket.destroy(Error('host request timeout')));socket.on('error',reject);socket.on('data',chunk=>{text+=chunk;if(text.includes('\n')){socket.end();try{resolve(JSON.parse(text.slice(0,text.indexOf('\n'))))}catch(e){reject(e)}}})})}
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function ready(){for(let i=0;i<100;i++){try{const r=await rpc({cmd:'status'});if(r.ok)return}catch{}await delay(30)}throw Error('isolated host did not start')}
let host,browser,server;
(async()=>{
 const requireHere=createRequire(path.join(root,'package.json'));
 const {createServer}=await import(pathToFileURL(requireHere.resolve('vite'))),react=(await import(pathToFileURL(requireHere.resolve('@vitejs/plugin-react')))).default;
 const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';import {AppearanceProvider} from '/src/Appearance';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.addEventListener('velella:terminal-created',e=>window.term=e.detail.terminal);createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id:'capabilities',program:'powershell',status:{exit:null,input_protocol:1}}}/></TooltipProvider></AppearanceProvider>);`;
 const html='<html><body style="margin:0"><div id="root" style="height:100vh;display:flex"></div><script type="module" src="/__capabilities.tsx"></script></body></html>';
 server=await createServer({root,configFile:false,plugins:[react(),{name:'native-capabilities',configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/capabilities'){void s.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return}next()})},resolveId(id){if(id==='/__capabilities.tsx')return id},load(id){if(id==='/__capabilities.tsx')return entry}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 await server.listen();
 host=spawn(runtime,['host',launch],{windowsHide:true,stdio:'ignore'});await ready();
 browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1440,height:900}});page.setDefaultTimeout(30000);
 const sent=[],errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/_velella',async route=>{const q=route.request().postDataJSON(),cmd=q.cmd.split('.').at(-1);if(cmd==='input')sent.push({kind:q.input_kind,text:q.text});const response=await rpc({...q,cmd});await route.fulfill({json:response})});
 await page.goto('http://127.0.0.1:'+server.httpServer.address().port+'/capabilities');
 await page.waitForFunction(()=>window.term&&Number(document.querySelector('.terminal-canvas')?.dataset.terminalOffset)>=0);
 fs.writeFileSync(gate,'start');
 // The startup splash already has a composer. Wait for the actual CLI footer,
 // after initialization and capability probing, before asserting native style.
 await page.waitForFunction(()=>{const b=window.term.buffer.active;return Array.from({length:b.length},(_,i)=>b.getLine(i)?.translateToString(true)||'').join('\n').includes('? for shortcuts')});
 // On Windows, Codex defers palette probing until app-server startup and all
 // protected startup decisions have settled (rust-v0.164.0 app/startup.rs).
 // A visible footer is not that readiness signal. Observe the protocol itself.
 const paletteDeadline=Date.now()+30000;
 while(Date.now()<paletteDeadline&&!sent.some(q=>q.kind==='protocol'&&q.text.startsWith('\x1b]11;rgb:')))await delay(50);
 await page.waitForTimeout(300);
 const raw=fs.readFileSync(path.join(session,'terminal.bin'),'utf8');
 const measured=await page.evaluate(()=>{const t=window.term,b=t.buffer.active,canvas=document.querySelector('.terminal-canvas').getBoundingClientRect(),screen=document.querySelector('.xterm-screen').getBoundingClientRect();return {text:Array.from({length:b.length},(_,i)=>b.getLine(i)?.translateToString(true)||'').join('\n'),rows:t.rows,cols:t.cols,canvasBottom:canvas.bottom,screenBottom:screen.bottom,footerInside:screen.bottom<=canvas.bottom+1}});
 const paletteReplies=sent.filter(q=>q.kind==='protocol'&&/^\x1b\](10|11);rgb:/.test(q.text));
 const result={runtime,codex,session,protocolReplies:paletteReplies.map(q=>q.text),hasNativeBackground:/\x1b\[(?:[0-9;]*;)?48;(?:2|5);/.test(raw),geometry:{rows:measured.rows,cols:measured.cols,canvasBottom:measured.canvasBottom,screenBottom:measured.screenBottom},footerInside:measured.footerInside,initialDrivingInputs:sent.filter(q=>q.kind!=='protocol').length,errors};
 fs.writeFileSync(path.join(output,'startup-result.json'),JSON.stringify(result,null,2));await page.screenshot({path:path.join(output,'native-codex-startup.png')});
 assert.equal(paletteReplies.some(q=>q.text.startsWith('\x1b]10;')),true,'foreground palette reply must reach the real CLI');
 assert.equal(paletteReplies.some(q=>q.text.startsWith('\x1b]11;')),true,'background palette reply must reach the real CLI');
 assert.equal(result.hasNativeBackground,true,'native CLI composer background must be emitted');assert.equal(result.footerInside,true);assert.equal(result.initialDrivingInputs,0);assert.deepEqual(errors,[]);
 const area=page.locator('.xterm-helper-textarea');await area.focus();await area.press('Control+c');await area.press('Control+c');
 for(let i=0;i<100;i++){if((await rpc({cmd:'status'}).catch(()=>({status:{exit:0}}))).status?.exit!==null)break;await delay(30)}
 result.interruptInputs=sent.filter(q=>q.kind==='user'&&q.text==='\x03').length;assert.ok(result.interruptInputs>=1);fs.writeFileSync(path.join(output,'startup-result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
})().catch(error=>{console.error(error);process.exitCode=1}).finally(async()=>{await browser?.close();if(host&&host.exitCode===null){try{await rpc({cmd:'kill'})}catch{}await delay(100);if(host.exitCode===null)host.kill()}await server?.close()});
