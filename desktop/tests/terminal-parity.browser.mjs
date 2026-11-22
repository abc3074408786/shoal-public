import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url)),artifacts=process.env.TERMINAL_QA_OUTPUT||resolve(root,'../artifacts/terminal-parity-20261016');
const html='<html><body><div id="test-tabbar" style="margin:40px 40px 0;width:900px;display:flex;justify-content:space-between"><span>PowerShell</span><div id="terminal-actions" class="terminal-tab-actions"></div></div><div id="root" style="margin:0 40px;width:900px;height:550px;display:flex"></div><script type="module" src="/__parity-entry.tsx"></script></body></html>';
const entry=`import React,{StrictMode} from 'react';import {createRoot} from 'react-dom/client';
import {Terminal} from '@xterm/xterm';import TerminalView from '/src/TerminalView';import {TooltipProvider} from '/src/ui';
import {saveTerminalPreferences,readTerminalPreferences} from '/src/terminal-preferences';
import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';
const open=Terminal.prototype.open;Terminal.prototype.open=function(element){open.call(this,element);window.term=this};
const root=createRoot(document.getElementById('root'));
window.preferences=value=>saveTerminalPreferences({...readTerminalPreferences(),...value});
window.mount=(id='one',locked=false)=>root.render(<StrictMode><TooltipProvider><TerminalView actionsTarget={document.getElementById('terminal-actions')} workspace={{projectId:'fixture',workspaceId:'fixture',id,status:{exit:locked?0:null,input_protocol:1}}}/></TooltipProvider></StrictMode>);window.mount();`;

test('whole TerminalView retains clipboard ownership, scrolling, renderer and live preferences',async()=>{
 const server=await createServer({root,configFile:false,plugins:[react(),{
  name:'parity-fixture',configureServer(server){server.middlewares.use((req,res,next)=>{
   if(req.url==='/__parity'){void server.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return}
   if(req.url==='/__parity-entry.tsx'){void server.transformRequest(req.url).then(value=>{res.setHeader('Content-Type','text/javascript');res.end(value.code)});return}next();
  })},resolveId(id){if(id==='/__parity-entry.tsx')return id},load(id){if(id==='/__parity-entry.tsx')return entry}
 }],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser;
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true,args:['--enable-unsafe-swiftshader']});
  const results=[];
  for(const scale of [1,1.5,2]){
   const context=await browser.newContext({viewport:{width:1200,height:850},deviceScaleFactor:scale});
   const page=await context.newPage();page.setDefaultTimeout(8000);
   await page.addInitScript(()=>{
    localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false,fontSize:14}));
    window.clipText='粘贴\n第二行';window.copies=[];window.reads=0;
    Object.defineProperty(navigator,'clipboard',{value:{readText:()=>{window.reads++;return window.pendingRead||Promise.resolve(window.clipText)},writeText:text=>{window.copies.push(text);return Promise.resolve()}}});
   });
   const errors=[];page.on('pageerror',error=>errors.push(error.message));
   let output=Buffer.from(Array.from({length:180},(_,i)=>`line ${i} 中文\r\n`).join('')+'\x1b[?2004h');
   const sent=[],requests=[];
   await page.route('**/_velella',async route=>{
    const request=route.request().postDataJSON();requests.push(request);let response={ok:true};
    if(request.cmd.endsWith('terminal.output')){
     const from=Math.min(request.since??0,output.length);
     response={ok:true,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:90,rows:25,sequence:1}]:[]};
    }
    if(request.cmd.endsWith('terminal.input'))sent.push(request);
    await route.fulfill({json:response});
   });
   await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__parity`);
   await page.waitForFunction(()=>window.term?.buffer.active.baseY>100).catch(async error=>{console.log('initial terminal diagnostic',{errors,requests:requests.slice(0,3),state:await page.evaluate(()=>({rows:window.term?.rows,baseY:window.term?.buffer.active.baseY,text:document.querySelector('.terminal-error')?.textContent}))});throw error});
   assert.equal(await page.evaluate(()=>{const bar=document.querySelector('#test-tabbar .terminal-toolbar-actions'),screen=document.querySelector('.xterm-screen');return !!bar&&!document.querySelector('.terminal-toolbar,.terminal-caption')&&bar.getBoundingClientRect().bottom<=screen.getBoundingClientRect().top}),true,'terminal actions share the tab bar without an extra heading or output overlay');
   assert.equal(await page.evaluate(()=>getComputedStyle(document.querySelector('.terminal-wrap')).backgroundColor===getComputedStyle(document.querySelector('.xterm-viewport')).backgroundColor),true,'terminal padding and canvas use the same surface');
   assert.deepEqual(await page.evaluate(()=>{const style=getComputedStyle(document.querySelector('.terminal-canvas'));return {background:style.backgroundColor,padding:style.padding,font:window.term.options.fontFamily,size:window.term.options.fontSize,weight:window.term.options.fontWeight,bold:window.term.options.fontWeightBold,lineHeight:window.term.options.lineHeight,scrollbar:window.term.options.scrollbar.width}}),{background:'rgb(35, 45, 42)',padding:'16px',font:'"Cascadia Mono", "Consolas", "DejaVu Sans Mono", "Liberation Mono", "Velella Nerd Font Symbols", "Symbols Nerd Font Mono", "MesloLGS Nerd Font", "JetBrainsMono Nerd Font", "Hack Nerd Font", monospace',size:14,weight:'400',bold:'600',lineHeight:1.16,scrollbar:14},'rendered terminal follows the selected Velella Mist direction');
   const terminal=page.locator('.xterm-helper-textarea');await terminal.focus();
   await terminal.press('Control+v');
   await page.waitForFunction(()=>window.reads===1).catch(async error=>{console.log('first-paste diagnostic',await page.evaluate(()=>({reads:window.reads,active:document.activeElement?.className,disabled:window.term.options.disableStdin})),errors,sent);throw error});
   await page.waitForTimeout(220);
   assert.equal(sent.at(-1).text,'\x1b[200~粘贴\r第二行\x1b[201~');
   const duplicateCount=sent.length;
   await terminal.evaluate(element=>element.dispatchEvent(new KeyboardEvent('keydown',{key:'v',code:'KeyV',ctrlKey:true,repeat:true,bubbles:true,cancelable:true})));
   await terminal.evaluate(element=>{const data=new DataTransfer();data.setData('text/plain','duplicate');element.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:data}))});
   await page.waitForTimeout(30);assert.equal(sent.length,duplicateCount,'native paste following the chord is deduplicated');
   await page.waitForTimeout(160);
   await page.evaluate(()=>window.clipText='单行');
   for(const chord of ['Control+Shift+v','Shift+Insert']){
    const before=sent.length;await terminal.press(chord);await page.waitForTimeout(30);assert.equal(sent.length,before+1,chord);assert.equal(sent.at(-1).text,'\x1b[200~单行\x1b[201~');
   }
   // A regular Ctrl+C still interrupts. Selection and Ctrl+Shift+C copy only.
   await terminal.press('Control+c');await page.waitForTimeout(30);assert.equal(sent.at(-1).text,'\x03');
   await page.evaluate(()=>window.term.select(0,0,8));
   const selected=await page.evaluate(()=>window.term.getSelection());const copyCount=sent.length;
   await terminal.press('Control+c');await terminal.press('Control+Shift+c');
   assert.deepEqual(await page.evaluate(()=>window.copies),[selected,selected]);assert.equal(sent.length,copyCount);
   await page.locator('.xterm-screen').click({button:'right'});
   await page.waitForFunction(()=>window.copies.length===3&&!window.term.hasSelection());
   const rightCount=sent.length;await page.locator('.xterm-screen').click({button:'right'});await page.waitForTimeout(30);
   assert.equal(sent.length,rightCount+1,'right-click without selection pastes');
   await terminal.press('Shift+Enter');await page.waitForTimeout(30);assert.equal(sent.at(-1).text,'\x1b\r','unnegotiated Shift+Enter fallback');
   output=Buffer.concat([output,Buffer.from('\x1b[>1u')]);
   await page.waitForFunction(()=>window.term._core.coreService.kittyKeyboard.flags===1);
   await terminal.press('Shift+Enter');await page.waitForTimeout(30);assert.equal(sent.at(-1).text,'\x1b[13;2u','negotiated Shift+Enter');
   output=Buffer.concat([output,Buffer.from('\x1b[<u\x1b[?2004h')]);
   await page.waitForFunction(()=>window.term._core.coreService.kittyKeyboard.flags===0);
   // Empty polls and live output keep an older normal-buffer viewport in place.
   await page.evaluate(()=>window.term.scrollToLine(20));
   const viewport=await page.evaluate(()=>window.term.buffer.active.viewportY);
   await page.waitForTimeout(250);assert.equal(await page.evaluate(()=>window.term.buffer.active.viewportY),viewport);
   output=Buffer.concat([output,Buffer.from('more output\r\n')]);
   await page.waitForTimeout(200);assert.equal(await page.evaluate(()=>window.term.buffer.active.viewportY),viewport);
   const screen=page.locator('.xterm-screen'),box=await screen.boundingBox();
   await page.mouse.move(box.x+100,box.y+100);await page.mouse.wheel(0,-120);await page.waitForTimeout(100);
   assert.ok(await page.evaluate(()=>window.term.buffer.active.viewportY)<viewport,'normal wheel scrolls history');
   output=Buffer.concat([output,Buffer.from('\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[2J\x1b[1;1HTUI')]);
   await page.waitForFunction(()=>window.term.buffer.active.type==='alternate'&&window.term.modes.mouseTrackingMode!=='none');
   const wheelStart=sent.length;await page.mouse.wheel(0,80);await page.waitForTimeout(80);
   assert.ok(sent.slice(wheelStart).some(x=>x.input_kind==='pointer'&&/^\x1b\[<65;/.test(x.text)),'tracked TUI wheel retains pointer provenance');
   output=Buffer.concat([output,Buffer.from('\x1b[?1000l\x1b[?1049l\x1b[5 q')]);
   await page.waitForFunction(()=>window.term.buffer.active.type==='normal');
   assert.equal(await page.evaluate(()=>window.term._core.coreService.decPrivateModes.cursorBlink),false,'steady preference also handles an application blinking cursor');
   await page.evaluate(()=>{window.identity=window.term;window.preferences({fontFamily:'Consolas',fontSize:18,cursorStyle:'underline',cursorBlink:true})});
   await page.waitForFunction(()=>window.term.options.fontSize===18&&window.term.options.cursorStyle==='underline');
   assert.equal(await page.evaluate(()=>window.term===window.identity),true,'appearance changes retain the terminal instance');
   assert.equal(await page.evaluate(()=>window.term._core.coreService.decPrivateModes.cursorBlink),true,'re-enabling restores the application blink request');
   await page.evaluate(()=>window.preferences({cursorBlink:false}));
   output=Buffer.concat([output,Buffer.from('\x1b[6 q')]);await page.waitForTimeout(200);
   await page.evaluate(()=>window.preferences({cursorBlink:true}));await page.waitForTimeout(30);
   assert.equal(await page.evaluate(()=>window.term._core.coreService.decPrivateModes.cursorBlink),false,'a later application steady request is retained while the preference parks blink');
   await page.evaluate(()=>window.preferences({cursorBlink:false,renderer:'webgl'}));
   await page.waitForFunction(()=>document.querySelector('[data-terminal-renderer]').dataset.terminalRenderer==='webgl');
   const gpu=await page.evaluate(()=>{const r=window.term._core._renderService._renderer.value;return {canvas:!!r._canvas,ratio:devicePixelRatio}});
   assert.ok(gpu.canvas,'a real WebGL renderer is attached');
   await page.evaluate(()=>window.term._core._renderService._renderer.value._gl.getExtension('WEBGL_lose_context').loseContext());
   await page.waitForFunction(()=>document.querySelector('[data-terminal-renderer]').dataset.terminalRenderer==='dom');
   assert.equal(await page.evaluate(()=>window.term===window.identity),true,'context-loss fallback retains session and buffer');
   // Search and settings keep their keyboard focus; they never paste into PTY.
   await terminal.press('Control+Shift+f');const search=page.getByRole('textbox',{name:'查找终端文本'});await search.fill('line 30');
   assert.equal(await page.evaluate(()=>document.querySelector('.terminal-find').getBoundingClientRect().bottom<=document.querySelector('.xterm-screen').getBoundingClientRect().top),true,'search never overlays terminal output');
   const searchCount=sent.length;await search.press('Control+v');await page.waitForTimeout(50);assert.equal(sent.length,searchCount);
   await search.press('Escape');
   await screen.click({button:'right',modifiers:['Control']});await page.getByRole('menuitem',{name:'终端设置',exact:true}).click();
   const fontSize=page.getByLabel('字号',{exact:true});await fontSize.fill('');await fontSize.pressSequentially('16');await fontSize.press('Tab');await page.waitForFunction(()=>window.term.options.fontSize===16);
   const lineHeight=page.getByLabel('行高',{exact:true});
   assert.equal(await lineHeight.evaluate(e=>e.value==='1.16'&&e.validity.valid),true,'selected Mist line height is a valid settings value');
   await lineHeight.fill('1.17');await lineHeight.press('Tab');await page.waitForFunction(()=>window.term.options.lineHeight===1.17);
   await lineHeight.fill('1.16');await lineHeight.press('Tab');await page.waitForFunction(()=>window.term.options.lineHeight===1.16);
   assert.equal(await lineHeight.evaluate(e=>e.validity.valid),true,'hundredth line-height adjustments remain valid after saving');
   assert.equal(await page.evaluate(()=>window.term===window.identity),true);
   await page.getByRole('button',{name:'关闭对话框'}).click();
   await terminal.focus();
   await page.evaluate(()=>{window.pendingRead=new Promise(resolve=>window.resolveRead=resolve)});
   const slowCount=sent.length,readCount=await page.evaluate(()=>window.reads);
   await terminal.press('Control+v');await terminal.press('Control+v');
   await page.waitForFunction(readCount=>window.reads===readCount+2,readCount);
   await page.evaluate(()=>{window.resolveRead('two intentional gestures');window.pendingRead=null});
   await page.waitForTimeout(60);assert.equal(sent.length,slowCount+2,'slow reads preserve both independent paste gestures');
   // An outstanding clipboard read is owned by the old viewer, including images
   // (image lifetime/forced bracketing also has its dedicated regression suite).
   await terminal.focus();await page.evaluate(()=>{window.pendingRead=new Promise(resolve=>window.resolveRead=resolve)});
   await terminal.press('Control+v');const pendingCount=sent.length;
   await page.evaluate(()=>window.mount('two'));await page.waitForFunction(()=>window.term!==window.identity);
   await page.evaluate(()=>{window.resolveRead('must not reach either PTY');window.pendingRead=null});await page.waitForTimeout(80);
   assert.equal(sent.length,pendingCount,'old-tab clipboard completion cannot reach the next tab');
   assert.equal(await page.evaluate(()=>document.activeElement===window.term.textarea),false,'new viewer does not steal focus');
   await page.evaluate(()=>window.mount('two',true));await page.waitForFunction(()=>window.term.options.disableStdin===true);
   const lockedCount=sent.length;await page.locator('.xterm-helper-textarea').focus();await page.locator('.xterm-helper-textarea').press('Control+v');await page.waitForTimeout(50);assert.equal(sent.length,lockedCount);
   assert.deepEqual(errors,[]);results.push({scale,clipboard:true,scrollback:true,tuiWheel:true,preferences:true,webgl:gpu,contextLossFallback:true,asyncOwnership:true,strictMode:true,errors});
   if(scale===1)await page.screenshot({path:resolve(artifacts,'browser-terminal.png')});
   await context.close();
  }
  mkdirSync(artifacts,{recursive:true});writeFileSync(resolve(artifacts,'browser-parity.json'),JSON.stringify({engine:'headless Edge',scope:'real TerminalView, mocked runtime and browser clipboard; synthetic composition is tested separately; no native OS-popup/UAT claim',results},null,2));
 }finally{await browser?.close();await server.close()}
});
