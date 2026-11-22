import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';

const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
let server,browser,url;
before(async()=>{
 const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {AppearanceProvider} from '/src/Appearance';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';window.addEventListener('velella:terminal-created',e=>window.term=e.detail.terminal);window.addEventListener('velella:terminal-frame-created',e=>window.frameTerm=e.detail.terminal);createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);`;
 const built=process.env.VELELLA_BROWSER_BUILD;
 server=await createServer({root:built||root,configFile:false,logLevel:'error',plugins:built?[]:[react(),{
  name:'session-history-fixture',resolveId:id=>id==='/__history.tsx'?id:null,load:id=>id==='/__history.tsx'?entry:null,
  configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__history'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__history.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})},
 }],server:{host:'127.0.0.1',port:0}});
 await server.listen();url=`http://127.0.0.1:${server.httpServer.address().port}/${built?'':'__history'}`;
 browser=await chromium.launch({channel:'msedge',headless:true});
});
after(async()=>{await browser?.close();await server?.close()});

async function fixture(run){
 const context=await browser.newContext(),requests=[];
 const session={id:'saved-codex',title:'Codex',kind:'terminal',program:'codex',task_id:null,native_id:null,recoverable:true,status:{pid:42,exit:0,state:'exited',input_protocol:1}};
 let layout={revision:1,axis:'horizontal',ratio:.5,closed:[],panes:[{id:'main',active:'t',tabs:[{id:'t',target:session.id,kind:'terminal',title:'Codex'}]}]};
 // This entire transcript arrives in ONE page: snapshots after a write cannot
 // recover the alternate screen once the CLI exits and clears it.
 const output=Buffer.from('\x1b[?1049h\x1b[2J\x1b[HUSER_SAVED_QUESTION\r\nASSISTANT_SAVED_ANSWER\x1b[?1049l\x1b[2J\x1b[H');
 await context.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));window.addEventListener('velella:terminal-created',e=>window.term=e.detail.terminal);window.addEventListener('velella:terminal-frame-created',e=>window.frameTerm=e.detail.terminal);});
 await context.route('**/_velella',async route=>{
  const q=route.request().postDataJSON();requests.push(q);let data={ok:true};
  switch(q.cmd){
   case 'workbench.snapshot':Object.assign(data,{projects:[{id:'p',name:'History fixture',path:'/fixture',direct:true}],workspaces:[{id:'w',project_id:'p',name:'History fixture',kind:'folder',path:'/fixture',archived:false,favorite:false,position:0}],tasks:[],source_links:[]});break;
   case 'workbench.layout':Object.assign(data,{layout,split_layout_version:1});break;
   case 'workbench.layout.save':layout={...q.layout,revision:layout.revision+1};data.layout=layout;break;
   case 'workbench.sessions':data.sessions=[session];break;
   case 'workbench.files':data.files=[];break;
   case 'workbench.git':Object.assign(data,{branch:'',status:''});break;
   case 'workbench.terminal.activity':Object.assign(data,{enabled:false,events:[],next:0,total:0});break;
   case 'workbench.terminal.output':{const from=Math.min(q.since??0,output.length);Object.assign(data,{from,end:output.length,total:output.length,data:output.subarray(from).toString('base64'),geometry:from===0?[{offset:0,cols:100,rows:30,sequence:0}]:[]});break;}
   default:throw Error(`Unexpected RPC ${q.cmd}`);
  }
  await route.fulfill({json:data});
 });
 const page=await context.newPage();
 const loaded=async p=>{await p.goto(url);await p.waitForFunction(total=>document.querySelector('.terminal-canvas')?.dataset.terminalOffset===String(total),output.length)};
 try{await loaded(page);await run({page,context,requests,output,layout:()=>layout})}finally{await context.close()}
}

test('closed terminal remains discoverable from a visible history entry after a new app page opens',async()=>fixture(async({page,context,requests,output,layout})=>{
 await page.getByRole('button',{name:'关闭标签 Codex',exact:true}).click();
 await page.getByRole('tab',{name:'Codex',exact:true}).waitFor({state:'detached'});
 assert.equal(layout().closed[0].target,'saved-codex');
 const history=page.getByRole('button',{name:'会话记录',exact:true});
 assert.equal(await history.isVisible(),true,'closing a tab must leave a visible history entry outside the new-terminal menu');
 await page.close();const reopened=await context.newPage();await reopened.goto(url);
 await reopened.getByRole('button',{name:'会话记录',exact:true}).click();
 await reopened.getByRole('dialog').getByRole('button').filter({hasText:'Codex'}).click();
 await reopened.waitForFunction(total=>document.querySelector('.terminal-canvas')?.dataset.terminalOffset===String(total),output.length);
 assert.equal(requests.filter(q=>['workbench.terminal.open','workbench.terminal.recover','workbench.terminal.input'].includes(q.cmd)).length,0,'viewing retained history must not launch a new conversation');
}));

test('exiting a CLI retains the conversation screen through tab close and app reload',async()=>fixture(async({page,output,requests})=>{
 async function assertSavedScreen(){
  await page.getByRole('button',{name:'查看对话历史',exact:true}).click();
  await page.getByRole('dialog',{name:'历史画面',exact:true}).waitFor();
  assert.equal(await page.getByText('尚无可查看的历史画面。',{exact:true}).count(),0,'the saved transcript must reconstruct the conversation before the CLI clears its screen');
  await page.waitForFunction(()=>window.frameTerm?.element?.isConnected);
  const text=await page.evaluate(()=>{const t=window.frameTerm;return Array.from({length:t.rows},(_,i)=>t.buffer.active.getLine(i)?.translateToString(true)).join('\n')});
  assert.match(text,/USER_SAVED_QUESTION/);assert.match(text,/ASSISTANT_SAVED_ANSWER/);
  await page.getByRole('button',{name:'关闭对话框',exact:true}).click();
 }
 await assertSavedScreen();
 await page.getByRole('button',{name:'关闭标签 Codex',exact:true}).click();
 await page.getByRole('button',{name:'会话记录',exact:true}).click();
 await page.getByRole('dialog').getByRole('button').filter({hasText:'Codex'}).click();
 await page.waitForFunction(total=>document.querySelector('.terminal-canvas')?.dataset.terminalOffset===String(total),output.length);
 await assertSavedScreen();await page.reload();
 await page.getByRole('button',{name:'会话记录',exact:true}).click();
 await page.getByRole('dialog',{name:'会话记录',exact:true}).getByRole('button').filter({hasText:'Codex'}).click();
 await page.waitForFunction(total=>document.querySelector('.terminal-canvas')?.dataset.terminalOffset===String(total),output.length);
 await assertSavedScreen();
 assert.equal(requests.filter(q=>['workbench.terminal.open','workbench.terminal.recover','workbench.terminal.input'].includes(q.cmd)).length,0);
}));
