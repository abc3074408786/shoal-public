import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import {fileURLToPath} from 'node:url';
import {realpathSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';

const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const html=`<!doctype html><html><body><button id="outside">Outside</button>
<div id="terminal" style="margin:60px;width:800px;height:440px"></div>
<script type="module" src="/__preposition-entry.js"></script></body></html>`;
const entry=`import {Terminal} from '@xterm/xterm';
import {stabilizeTerminalComposition} from '/src/terminal-ime';
import '@xterm/xterm/css/xterm.css';import '/src/terminal-ime.css';
window.term=new Terminal({cols:80,rows:20,fontSize:14});
term.open(document.getElementById('terminal'));
window.guard=stabilizeTerminalComposition(term);
window.compositionEvents=0;
term.textarea.addEventListener('compositionstart',()=>window.compositionEvents++);
window.write=bytes=>new Promise(resolve=>term.write(bytes,resolve));`;

test('painted TUI caret prepares the native insertion point before composition events',async()=>{
  const server=await createServer({root,configFile:false,plugins:[{
    name:'ime-preposition-fixture',configureServer(server){
      server.middlewares.use((req,res,next)=>{
        if(req.url==='/__preposition'){
          void server.transformIndexHtml(req.url,html).then(value=>{res.setHeader('Content-Type','text/html');res.end(value)});return;
        }
        next();
      });
    },resolveId(id){if(id==='/__preposition-entry.js')return id},
    load(id){if(id==='/__preposition-entry.js')return entry},
  }],server:{host:'127.0.0.1',port:0,fs:{allow:[root,realpathSync(resolve(root,'node_modules'))]}},logLevel:'error'});
  let browser;
  try{
    await server.listen();
    browser=await chromium.launch({channel:'msedge',headless:true});
    const results=[];
    for(const scale of [1,1.5,2]){
      const page=await browser.newPage({viewport:{width:1200,height:800},deviceScaleFactor:scale});
      const errors=[];page.on('pageerror',error=>errors.push(error.message));
      await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__preposition`);
      await page.waitForFunction(()=>!!window.guard);
      const rule='─'.repeat(80);
      await page.evaluate(rule=>window.write(`\x1b[2J\x1b[1;1HClaude Code v2.1.238\x1b[5;1H${rule}\x1b[6;1H❯ ab\x1b[7m \x1b[0m\x1b[7;1H${rule}\x1b[20;80H`),rule);
      const textarea=page.locator('.xterm-helper-textarea');
      await textarea.focus();
      const check=async(column,row,label)=>{
        await page.waitForFunction(({column,row})=>{
          const t=term.textarea,s=term.element.querySelector('.xterm-screen').getBoundingClientRect(),b=t.getBoundingClientRect();
          return Math.abs(b.x-s.x-column*s.width/term.cols)<1&&Math.abs(b.y-s.y-row*s.height/term.rows)<1;
        },{column,row});
        const box=await textarea.boundingBox();
        assert.equal(await page.evaluate(()=>window.compositionEvents),0,label+' must work before compositionstart');
        return box;
      };
      const focused=await check(4,5,'focus');
      // xterm writes the real output cursor's bottom-right coordinates. Its
      // MutationObserver correction must settle without a self-triggering loop.
      await textarea.evaluate(t=>{t.style.left='777px';t.style.top='333px'});
      await check(4,5,'style overwrite');
      await page.evaluate(()=>window.write('\x1b[6;3Habc\x1b[7m \x1b[0m\x1b[20;79H'));
      const rendered=await check(5,5,'output redraw');
      // Real Claude parks at the blank end of this populated footer after a
      // commit. Reproduce it before blur/refocus, without composition events.
      await page.evaluate(()=>window.write('\x1b[8;1H  ⏸ manual mode on\x1b[8;80H'));
      await check(5,5,'populated footer output cursor');
      await page.locator('#outside').focus();
      await textarea.evaluate(t=>{t.style.left='701px';t.style.top='302px'});
      await textarea.focus();
      await check(5,5,'refocus');
      await page.evaluate(()=>{term.options.fontSize=18;term.resize(90,22)});
      const resized=await check(5,5,'font and grid resize');
      // Leaving the recognized screen restores ordinary xterm cursor behavior.
      await page.evaluate(()=>window.write('\x1b[2J\x1b[3;8Hplain terminal'));
      const normal=await check(21,2,'ordinary terminal');
      await textarea.evaluate(t=>{t.style.left='123px';t.style.top='45px'});
      await page.waitForTimeout(30);
      assert.deepEqual(await textarea.evaluate(t=>[t.style.left,t.style.top]),['123px','45px'],'ordinary terminal styles belong to xterm');
      // Disposal removes event subscriptions and observers, including focus.
      await page.evaluate(()=>guard.dispose());
      await page.evaluate(rule=>window.write(`\x1b[2J\x1b[1;1HClaude Code v2.1.238\x1b[5;1H${rule}\x1b[6;1H❯ ab\x1b[7m \x1b[0m\x1b[7;1H${rule}\x1b[22;90H`),rule);
      await page.locator('#outside').focus();await textarea.focus();
      await textarea.evaluate(t=>{t.style.left='111px';t.style.top='42px'});
      await page.waitForTimeout(30);
      assert.deepEqual(await textarea.evaluate(t=>[t.style.left,t.style.top]),['111px','42px'],'disposed guard must not place the textarea');
      assert.deepEqual(errors,[]);
      results.push({scale,focused,rendered,resized,normal,compositionEvents:0,errors});
      await page.close();
    }
    const directory=process.env.VELELLA_TERMINAL_TEST_ARTIFACTS||resolve(root,'../artifacts/terminal-ime-native-geometry-20261017');
    mkdirSync(directory,{recursive:true});
    writeFileSync(resolve(directory,'preposition-regression.json'),JSON.stringify({scope:'real xterm public events; zero composition events; no OS popup',results},null,2));
  }finally{await browser?.close();await server.close()}
});
