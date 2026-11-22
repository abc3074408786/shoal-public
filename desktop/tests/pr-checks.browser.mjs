import {test} from 'node:test';import assert from 'node:assert/strict';import {createRequire} from 'node:module';
import {createServer} from 'vite';import react from '@vitejs/plugin-react';import {fileURLToPath} from 'node:url';import {mkdirSync,writeFileSync} from 'node:fs';import {join} from 'node:path';
const {chromium}=createRequire(import.meta.url)('playwright');const root=fileURLToPath(new URL('..',import.meta.url)),output=join(root,'../artifacts/pr-review-checks-20261025/browser');
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {AppearanceProvider} from '/src/Appearance';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);`;
const checked_at='2026-10-25T04:00:00Z';
const ready={state:'ready',message:'',checked_at,branch:'feature',repository:{name:'owner/repo',url:'https://github.com/owner/repo'},pull_requests:[{number:7,title:'实现评审检查',url:'https://github.com/owner/repo/pull/7',state:'OPEN',draft:false,head_sha:'a'.repeat(40),head_branch:'feature',base_branch:'main',updated_at:checked_at,review_decision:'REVIEW_REQUIRED',summary:{success:1,pending:1,failure:1,cancelled:1,skipped:1,unknown:1},checks:[['Build','success','通过'],['Test','pending','进行中'],['Lint','failure','失败'],['Deploy','cancelled','已取消'],['Optional','skipped','已跳过'],['External','unknown','未知']].map(([name,status,label],i)=>({id:String(i),name,status,label,url:'https://github.com/owner/repo/actions/runs/'+(i+1),workflow:'CI',started_at:'',completed_at:''}))}]};
test('checks tab handles real workspace entry, refresh states, links and stale workspace responses',async()=>{
 const server=await createServer({root,configFile:false,plugins:[react(),{name:'pr-checks-fixture',resolveId:id=>id==='/__checks.tsx'?id:null,load:id=>id==='/__checks.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__checks'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__checks.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser;const requests=[],errors=[];let response={state:'not_git',message:'当前文件夹不是 Git 仓库。',checked_at},release,hold=false;
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});const page=await browser.newPage({viewport:{width:1440,height:900}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(()=>{localStorage.setItem('velella:workspace','w');localStorage.setItem('velella:ui:theme',JSON.stringify('dark'));window.velellaDesktop={invoke:async(command,args)=>{if(command==='window')return false;return fetch('/_test',{method:'POST',body:JSON.stringify({command,args})}).then(r=>r.json())},listen:()=>()=>{}}});
  await page.route('**/_test',async route=>{const {command,args}=route.request().postDataJSON();requests.push({command,args});if(command==='pr_checks'){const value=args.workspace_id==='other'?{state:'no_pr',message:'第二工作区尚未关联 PR。',checked_at}:structuredClone(response);if(hold&&args.workspace_id==='w'){hold=false;await new Promise(resolve=>release=resolve)}await route.fulfill({json:value});return}
   if(command==='terminal_open_link'){await route.fulfill({json:{ok:true}});return}
   const q=args?.request||{};let value={ok:true};
   if(q.cmd==='workbench.snapshot')Object.assign(value,{projects:[{id:'p',name:'Project',path:'/fixture'}],workspaces:[{id:'w',project_id:'p',name:'Feature workspace',path:'/fixture/one',kind:'worktree',position:0},{id:'other',project_id:'p',name:'Second workspace',path:'/fixture/two',kind:'worktree',position:1}],tasks:[],source_links:[]});
   if(q.cmd==='workbench.layout')value.layout={revision:1,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]};
   if(q.cmd==='workbench.sessions')value.sessions=[];if(q.cmd==='workbench.files')value.files=[];
   await route.fulfill({json:value});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__checks`);
  await page.getByRole('button',{name:'展开工具栏',exact:true}).click();await page.getByRole('tab',{name:'检查',exact:true}).click();
  await page.getByRole('heading',{name:'尚未打开 Git 仓库'}).waitFor();
  for(const [state,message,heading] of [['no_pr','当前分支还没有关联的 PR。','尚未关联 PR'],['auth_required','请先连接 GitHub。','需要连接 GitHub'],['error','读取检查超时，请稍后刷新。','检查读取失败']]){response={state,message,checked_at};await page.getByRole('button',{name:'刷新评审检查'}).click();await page.getByRole('heading',{name:heading}).waitFor();}
  response=structuredClone(ready);await page.getByRole('button',{name:'刷新评审检查'}).click();await page.getByRole('button',{name:'查看检查 Build'}).waitFor();
  assert.equal(await page.locator('.pr-checks-list li').count(),6);assert.match(await page.locator('.pr-checks-summary').innerText(),/1 失败/);assert.match(await page.locator('.pr-checks-summary').innerText(),/1 进行中/);assert.match(await page.locator('.pr-checks-meta').innerText(),/aaaaaaaa/);
  await page.getByRole('button',{name:'查看检查 Build'}).click();assert(requests.some(r=>r.command==='terminal_open_link'&&r.args.url==='https://github.com/owner/repo/actions/runs/1'));
  mkdirSync(output,{recursive:true});await page.screenshot({path:join(output,'checks-1440.png')});await page.setViewportSize({width:900,height:760});await page.screenshot({path:join(output,'checks-900.png')});
  assert(await page.locator('.pr-checks-panel').evaluate(e=>e.scrollWidth<=e.clientWidth+1));
  response={...ready,pull_requests:[{...ready.pull_requests[0],checks:[],summary:{success:0,pending:0,failure:0,cancelled:0,skipped:0,unknown:0}}]};await page.getByRole('button',{name:'刷新评审检查'}).click();await page.getByRole('heading',{name:'尚无检查结果'}).waitFor();
  response=structuredClone(ready);hold=true;const request=page.waitForRequest(r=>r.url().endsWith('/_test')&&r.postDataJSON().command==='pr_checks');await page.getByRole('button',{name:'刷新评审检查'}).click();await request;
  await page.getByRole('button',{name:'Second workspace',exact:true}).click();await page.getByText('第二工作区尚未关联 PR。',{exact:true}).waitFor();release();await page.waitForTimeout(100);assert.equal(await page.locator('.pr-checks-list li').count(),0);
  assert.equal(requests.some(r=>/\.(stop|open|recover|commit|pr)$/.test(r.args?.request?.cmd||'')),false);assert.deepEqual(errors,[]);
  writeFileSync(join(output,'result.json'),JSON.stringify({status:'passed',entry:true,refreshStates:true,links:true,currentHead:true,emptyChecks:true,staleWorkspaceIgnored:true,responsive:true,noRemoteWrites:true,errors},null,2));
 }finally{release?.();await browser?.close();await server.close()}
});
