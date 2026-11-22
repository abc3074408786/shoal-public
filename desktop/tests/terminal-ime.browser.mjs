import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {realpathSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';

// Playwright may be supplied through NODE_PATH by the desktop workspace runtime.
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const html=`<!doctype html><html data-theme="dark"><body>
<div id="root" style="margin:80px 100px;width:800px;height:440px;display:flex"></div>
<script type="module" src="/__ime-entry.tsx"></script></body></html>`;
const entry=`import React from 'react';
import {createRoot} from 'react-dom/client';
import TerminalView from '/src/TerminalView';
import {TooltipProvider} from '/src/ui';
import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';
const root=createRoot(document.getElementById('root'));
window.addEventListener('velella:terminal-created',event=>{window.imeTerminal=event.detail.terminal});
window.mountTerminal=(id='fixture')=>root.render(<TooltipProvider><TerminalView workspace={{projectId:'fixture',workspaceId:'fixture',id,status:{exit:null,input_protocol:1}}}/></TooltipProvider>);
window.mountTerminal();`;

test('terminal IME anchor stays at the input position while background output redraws',async()=>{
  const server=await createServer({root,configFile:false,plugins:[react(),{
    name:'terminal-ime-fixture',configureServer(server){
      server.middlewares.use((req,res,next)=>{
        if(req.url==='/__ime'){void server.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return;}
        if(req.url==='/__ime-entry.tsx'){
          void server.transformRequest('/__ime-entry.tsx').then(value=>{res.setHeader('Content-Type','text/javascript');res.end(value.code)});return;
        }
        next();
      });
    },resolveId(id){if(id==='/__ime-entry.tsx')return id;},load(id){if(id==='/__ime-entry.tsx')return entry;},
  }],optimizeDeps:{force:true},server:{host:'127.0.0.1',port:0,fs:{allow:[root,realpathSync(resolve(root,'node_modules'))]}},logLevel:'error'});
  let browser;
  try{
    await server.listen();
    browser=await chromium.launch({channel:'msedge',headless:true});
    const results=[];
    for(const scenario of [{scale:1,font:12,theme:'dark'},{scale:1.5,font:14,theme:'dark'},{scale:2,font:18,theme:'light'}]){
    const context=await browser.newContext({viewport:{width:1200,height:800},deviceScaleFactor:scenario.scale});
    const page=await context.newPage();
    page.setDefaultTimeout(10000);
    await page.addInitScript(({font,theme})=>{
      localStorage.setItem('velella:terminal-font-size',String(font));
      localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({fontSize:font,renderer:'dom',cursorBlink:false}));
      document.addEventListener('DOMContentLoaded',()=>document.documentElement.dataset.theme=theme);
    },scenario);
    let output=Buffer.from('\x1b[5;12H'),sent=[];
    const errors=[];page.on('pageerror',error=>{errors.push(error.message);console.log('pageerror:',error.message)});
    await page.route('**/_velella',async route=>{
      const request=route.request().postDataJSON();let response={ok:true};
      if(request.cmd.endsWith('terminal.output')){
        const from=request.since??0;
        response={ok:true,from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:80,rows:20,sequence:1}]:[]};
      }
      if(request.cmd.endsWith('terminal.input'))sent.push(request.text);
      await route.fulfill({json:response});
    });
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__ime`);
    const textarea=page.locator('.xterm-helper-textarea');
    await textarea.waitFor({state:'attached'});
    await page.waitForFunction(()=>parseFloat(document.querySelector('.xterm-helper-textarea').style.top)>0);
    await textarea.focus();
    await page.waitForFunction(()=>{const element=document.querySelector('.xterm-cursor');return element?.isConnected&&getComputedStyle(element).animationName.length>0});
    const cursorAnimation=await page.evaluate(()=>getComputedStyle(document.querySelector('.xterm-cursor')).animationName);
    assert.equal(cursorAnimation,'none','the input cursor must remain visible without a blink animation');
    // Replay the two native Sogou paths captured on 2026-10-17. In the failing
    // path keydown(229) is followed by keypress and input for the same commit,
    // without any composition event. Exercise TerminalView's actual RPC seam.
    const nativeCommits=[];
    const commit=async({text='你',imeKeydown=true,keypress=true,delayInput=false,input=true}={})=>{
      const start=sent.length;
      await textarea.evaluate((element,{text,imeKeydown,keypress,delayInput,input})=>{
        if(imeKeydown)element.dispatchEvent(new KeyboardEvent('keydown',{key:'Process',code:'Space',keyCode:229,bubbles:true,cancelable:true}));
        if(keypress)element.dispatchEvent(new KeyboardEvent('keypress',{key:text,charCode:text.charCodeAt(0),keyCode:text.charCodeAt(0),which:text.charCodeAt(0),bubbles:true,cancelable:true}));
        const finish=()=>{
          element.dispatchEvent(new InputEvent('beforeinput',{data:text,inputType:'insertText',bubbles:true,composed:true,cancelable:true}));
          element.dispatchEvent(new InputEvent('textInput',{data:text,bubbles:true,composed:true}));
          element.value+=text;element.setSelectionRange(element.value.length,element.value.length);
          if(input)element.dispatchEvent(new InputEvent('input',{data:text,inputType:'insertText',bubbles:true,composed:true}));
          element.dispatchEvent(new KeyboardEvent('keyup',{key:' ',code:'Space',keyCode:32,bubbles:true}));
        };
        if(delayInput)setTimeout(finish,20);else finish();
      },{text,imeKeydown,keypress,delayInput,input});
      await page.waitForTimeout(60);
      const actual=sent.slice(start);
      nativeCommits.push({text,imeKeydown,keypress,delayInput,input,actual});
      assert.deepEqual(actual,[text],'one native-style IME commit must produce exactly one terminal.input request');
    };
    await commit();
    await commit(); // Intentional repeated characters must remain two commits.
    assert.deepEqual(sent,['你','你']);
    await textarea.evaluate(element=>element.blur());await textarea.focus();
    await commit({keypress:false});
    await commit({imeKeydown:false,keypress:false});
    await commit({keypress:false,delayInput:true}); // Async input after an empty textarea-diff timer.
    await commit({keypress:false,input:false}); // Textarea-diff fallback remains supported.
    const beforeAscii=sent.length;await textarea.press('a');await page.waitForTimeout(60);
    assert.deepEqual(sent.slice(beforeAscii),['a'],'ordinary keypress/input still sends once');
    // Start the existing composition/Enter/cancel checks with a clean fixture.
    await textarea.evaluate(element=>element.blur());await textarea.focus();sent=[];
    await textarea.evaluate(element=>{
      element.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
      element.value='ni';element.setSelectionRange(2,2);
      element.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'ni'}));
    });
    await page.waitForFunction(()=>document.querySelector('.terminal-ime-preedit')?.textContent.includes('ni'));
    const before=await textarea.boundingBox();
    const preeditBefore=await page.locator('.terminal-ime-preedit:not([hidden])').boundingBox();
    output=Buffer.concat([output,Buffer.from('\x1b[1;1Hbackground status\x1b[14;42H')]);
    // Observe the known VT destination, not an inline style sampled after the
    // output was made available (which can already contain the new position).
    await page.waitForFunction(()=>window.imeTerminal.buffer.active.cursorY===13&&window.imeTerminal.buffer.active.cursorX===41);
    const after=await textarea.boundingBox();
    const preeditAfter=await page.locator('.terminal-ime-preedit:not([hidden])').boundingBox();
    assert.ok(Math.abs(after.x-before.x)<1&&Math.abs(after.y-before.y)<1,
      `IME anchor jumped from (${before.x},${before.y}) to (${after.x},${after.y}) during composition`);
    assert.ok(Math.abs(preeditAfter.x-preeditBefore.x)<1&&Math.abs(preeditAfter.y-preeditBefore.y)<1,'preedit must stay with the IME anchor');
    assert.deepEqual(sent,[],'unfinished composition must not reach the PTY');
    await textarea.evaluate(element=>{
      element.value='你';element.setSelectionRange(1,1);
      element.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'你'}));
    });
    const submitted=page.waitForResponse(response=>response.url().endsWith('/_velella')&&response.request().postDataJSON().cmd.endsWith('terminal.input'));
    await textarea.evaluate(element=>element.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'你'})));
    await submitted;
    assert.deepEqual(sent,['你'],'confirmed Chinese is delivered exactly once without an Enter');
    await textarea.evaluate(element=>{
      element.dispatchEvent(new KeyboardEvent('keypress',{key:'你',charCode:20320,bubbles:true,cancelable:true}));
      element.value='你';
      element.dispatchEvent(new InputEvent('input',{data:'你',inputType:'insertFromComposition',bubbles:true}));
    });
    await page.waitForTimeout(60);
    assert.deepEqual(sent,['你'],'trailing keypress/input after compositionend is the same commit');
    const entered=page.waitForResponse(response=>response.url().endsWith('/_velella')&&response.request().postDataJSON().cmd.endsWith('terminal.input'));
    await textarea.press('Enter');await entered;
    assert.deepEqual(sent,['你','\r'],'a later Enter still submits normally');
    const beforeFollow=await textarea.boundingBox();
    output=Buffer.concat([output,Buffer.from('\x1b[8;20H')]);
    await page.waitForFunction(previous=>{
      const box=document.querySelector('.xterm-helper-textarea').getBoundingClientRect();
      return Math.abs(box.x-previous.x)>1&&Math.abs(box.y-previous.y)>1;
    },beforeFollow);
    // A second composition starts at the current cursor, not the previous pin.
    await textarea.evaluate(element=>{
      element.value='';element.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
      element.value='hao';element.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'hao'}));
    });
    const nextBefore=await textarea.boundingBox();
    output=Buffer.concat([output,Buffer.from('\x1b[2;3H')]);
    await page.waitForFunction(()=>parseFloat(document.querySelector('.xterm-helper-textarea').style.top)<30);
    const nextAfter=await textarea.boundingBox();
    assert.ok(Math.abs(nextAfter.x-nextBefore.x)<1&&Math.abs(nextAfter.y-nextBefore.y)<1,'next composition uses its own stable anchor');
    await textarea.evaluate(element=>element.blur());
    const afterBlur=await textarea.boundingBox();
    assert.ok(Math.abs(afterBlur.y-nextAfter.y)>1,'leaving the input releases the old anchor');
    await textarea.focus();
    await textarea.evaluate(element=>{
      element.value='好';element.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'好'}));
      element.dispatchEvent(new InputEvent('input',{data:'好',inputType:'insertFromComposition',bubbles:true}));
    });
    await page.waitForTimeout(60);
    assert.deepEqual(sent,['你','\r'],'refocusing does not revive a candidate canceled by blur');
    output=Buffer.concat([output,Buffer.from('\x1b[6;80H')]);
    await page.waitForFunction(()=>parseFloat(document.querySelector('.xterm-helper-textarea').style.left)>500);
    await textarea.evaluate(element=>{
      element.value='';element.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
      element.value='这是靠近右边缘的很长的中文组合输入';element.setSelectionRange(element.value.length,element.value.length);
      element.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:element.value}));
    });
    await page.waitForFunction(()=>document.querySelector('.terminal-ime-preedit')?.textContent.includes('右边缘'));
    const edge=await textarea.boundingBox(),screen=await page.locator('.xterm-screen').boundingBox();
    assert.ok(edge.x>=screen.x-1&&edge.x+edge.width<=screen.x+screen.width+1,'long preedit candidate anchor stays inside terminal bounds');
    await textarea.evaluate(element=>{element.value='';element.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',code:'Escape',keyCode:27,bubbles:true,cancelable:true}));element.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:''}))});
    await textarea.evaluate(element=>{
      element.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
      element.value='再';element.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'再'}));
    });
    await page.waitForTimeout(60);
    assert.deepEqual(sent,['你','\r','再'],'a new composition works immediately after Escape');
    await page.evaluate(()=>window.imeTerminal.options.disableStdin=true);
    await textarea.evaluate(element=>{
      element.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
      element.value='禁';element.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'禁'}));
    });
    await page.waitForTimeout(60);
    assert.deepEqual(sent,['你','\r','再'],'read-only viewers cannot submit IME commits');
    await page.evaluate(()=>window.imeTerminal.options.disableStdin=false);
    const oldTextarea=await textarea.elementHandle();
    await page.evaluate(()=>window.mountTerminal('next-session'));
    await page.waitForFunction(element=>!element.isConnected,oldTextarea);
    await page.waitForFunction(()=>document.querySelector('.xterm-helper-textarea')&&document.activeElement!==document.querySelector('.xterm-helper-textarea'));
    assert.deepEqual(sent,['你','\r','再'],'cancel and session switch must not send stale composition');
    assert.deepEqual(errors,[]);
    results.push({...scenario,nativeCommits,before,after,preeditBefore,preeditAfter,afterBlur,sent,errors});
    await context.close();
    }
    const directory=process.env.VELELLA_TERMINAL_TEST_ARTIFACTS||resolve(root,'../artifacts/terminal-parity-20261016');mkdirSync(directory,{recursive:true});
    writeFileSync(resolve(directory,'browser-regression.json'),JSON.stringify({engine:'headless Edge',scope:'real TerminalView, simulated composition events; no OS candidate popup',results},null,2));
    console.log(JSON.stringify(results));
  }finally{await browser?.close();await server.close();}
});
