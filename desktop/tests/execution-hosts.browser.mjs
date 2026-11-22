import {test} from 'node:test';import assert from 'node:assert/strict';import {createRequire} from 'node:module';
import {createServer} from 'vite';import react from '@vitejs/plugin-react';import {fileURLToPath} from 'node:url';import {mkdirSync,writeFileSync} from 'node:fs';import {join} from 'node:path';
const {chromium}=createRequire(import.meta.url)(process.env.PLAYWRIGHT_MODULE||'playwright');
const root=fileURLToPath(new URL('..',import.meta.url)),output=join(root,'../artifacts/execution-hosts-20261029/browser');
const hostId='018fa61b-4db2-7000-8000-0000000000aa',serverId='018fa61b-4db2-7000-8000-0000000000bb',inspectionId='018fa61b-4db2-7000-8000-0000000000ee';
const remoteTarget={kind:'ssh',host_id:hostId,server_id:serverId};
const identity={protocol:1,server_id:serverId,data_root:'/srv/中文 项目',os:'linux',runtime_version:'0.2.0',persistence:'prestarted_unverified',capabilities:['identity-v1','multiplex-v1']};
const profile={name:'测试主机',host:'fixture.test',user:'tester',port:2222};
const hostRow={id:hostId,revision:3,name:'测试主机',ssh_profile_id:'profile-1',data_root:identity.data_root,confirmation:null};
const trustedHost={...hostRow,confirmation:{server_id:serverId,data_root:identity.data_root,os:identity.os,profile_fingerprint:'fp-1'}};
const connection={host_id:hostId,inspection_id:inspectionId,state:'connected',identity,target:remoteTarget};
const entry=`import React from 'react';import {createRoot} from 'react-dom/client';import WorkspaceApp from '/src/WorkspaceApp';import {AppearanceProvider} from '/src/Appearance';import {TooltipProvider} from '/src/ui';import {ConfirmationProvider} from '/src/ConfirmAction';import '/src/style.css';import '/src/theme.css';import '/src/orca-shell.css';createRoot(document.getElementById('root')).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);`;
const localSnapshot={ok:true,projects:[{id:'p-local',name:'本机项目',path:'D:/Projects/仓库',setup:'',default_base:null,setup_default:'skip'}],
  workspaces:[{id:'w-local',project_id:'p-local',name:'local-feature',path:'D:/Projects/仓库-local',kind:'worktree',branch:'codex/local',position:0}],tasks:[],source_links:[]};
const remoteSnapshot={ok:true,projects:[{id:'p-remote',name:'远端项目',path:'/srv/中文 项目/仓库',setup:'npm ci',default_base:'main',setup_default:'skip'}],
  workspaces:[{id:'w-root',project_id:'p-remote',name:'主工作区',path:'/srv/中文 项目/仓库',kind:'primary',branch:'main',position:0},
    {id:'w-child',project_id:'p-remote',name:'远端子工作树',path:'/srv/中文 项目/仓库-child',kind:'worktree',branch:'velella/child',position:1,parent_workspace_id:'w-root'}],tasks:[],source_links:[]};
// A snapshot that could only arrive from the host we already left: it carries a unique project
// name, so applying it in the local workbench would be visible in the sidebar.
const lateSnapshot={...remoteSnapshot,projects:[{...remoteSnapshot.projects[0],id:'p-late',name:'迟到的远端项目'}]};
// Commands that read a path, process, window or repository on this machine. None may run while
// the workbench points at a remote execution host, and none may be sent a remote target.
const LOCAL_ONLY_COMMANDS=['desktop_resources','resource_manager','resource_space_scan','resource_space_clean','project_clone','project_clone_cancel','pr_checks','browser_command','browser_layout','terminal_clipboard','terminal_performance_export'];
test('the workbench follows one execution host and keeps host-local services on the local machine',async()=>{
 const server=await createServer({root,configFile:false,plugins:[react(),{name:'execution-hosts-fixture',resolveId:id=>id==='/__hosts.tsx'?id:null,load:id=>id==='/__hosts.tsx'?entry:null,configureServer(s){s.middlewares.use((req,res,next)=>{if(req.url==='/__hosts'){void s.transformIndexHtml(req.url,'<html><body><div id="root"></div><script type="module" src="/__hosts.tsx"></script></body></html>').then(html=>{res.setHeader('Content-Type','text/html');res.end(html)});return}next()})}}],server:{host:'127.0.0.1',port:0},logLevel:'error'});
 let browser,page,release;let connected=false,holdLate=false;const errors=[];
 const requests=[];mkdirSync(output,{recursive:true});
 const snapshot=r=>r.command==='rpc'&&r.args?.request?.cmd==='workbench.snapshot';
 const projectRow=pattern=>page.locator('.ide-project-name').filter({hasText:pattern});
 const answer=({command,args})=>{
  if(command==='window')return false;
  if(command==='execution_hosts')switch(args?.action){
   case 'inspect':return {ok:true,host_id:hostId,inspection_id:inspectionId,state:'awaiting_confirmation',identity,profile,target:null,confirmation_valid:false};
   case 'connect':connected=true;return {ok:true,host_id:hostId,inspection_id:args.inspection_id,state:'connected',identity,target:remoteTarget};
   case 'disconnect':connected=false;return {ok:true};
   case 'list':return {ok:true,hosts:[connected?trustedHost:hostRow],connections:connected?[connection]:[]};
   default:return {ok:true,host:hostRow};}
  if(command==='rpc'){const request=args?.request||{};
   if(request.cmd==='ssh.list')return {ok:true,profiles:[{id:'profile-1',...profile}]};
   if(request.cmd==='workbench.snapshot'){
    if(args?.target?.kind!=='ssh')return structuredClone(localSnapshot);
    if(holdLate){holdLate=false;return new Promise(resolve=>{release=()=>resolve(structuredClone(lateSnapshot))});}
    return structuredClone(remoteSnapshot);}
   if(request.cmd==='workbench.layout')return {ok:true,layout:{revision:1,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]}};
   if(request.cmd==='workbench.sessions')return {ok:true,sessions:[]};
   if(request.cmd==='workbench.files')return {ok:true,files:[]};
   if(request.cmd==='workbench.git')return {ok:true,status:'干净的工作区'};
   return {ok:true};}
  // The local PR reader is a machine-local service: answer it so the regression screenshot shows
  // a working panel instead of an error, and so its target argument can be asserted below.
  if(command==='pr_checks')return {state:'ready',message:'',checked_at:new Date().toISOString(),branch:'local-feature',
   pull_requests:[{number:412,title:'本机评审检查',url:'https://example.test/pull/412',state:'OPEN',draft:false,head_sha:'018fa61b00000000',head_branch:'local-feature',base_branch:'main',updated_at:new Date().toISOString(),review_decision:'',
    checks:[{id:'c1',name:'desktop tests',status:'success',label:'通过',url:null,workflow:'ci',started_at:'',completed_at:''}],summary:{success:1,pending:0,failure:0,cancelled:0,skipped:0,unknown:0}}]};
  throw Error('unexpected command '+command);};
 async function until(predicate,message){const start=Date.now();while(!predicate()){if(Date.now()-start>15000)throw Error('Timed out: '+message);await new Promise(resolve=>setTimeout(resolve,50))}}
 try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});page=await browser.newPage({viewport:{width:1440,height:900}});page.setDefaultTimeout(15000);page.on('pageerror',e=>errors.push(e.message));
  await page.exposeFunction('__velellaAnswer',({command,args})=>{requests.push({command,args});return answer({command,args})});
  await page.addInitScript(()=>{localStorage.setItem('velella:workspace','w-local');localStorage.setItem('velella:ui:theme',JSON.stringify('dark'));
   window.velellaDesktop={invoke:async(command,args)=>structuredClone(await window.__velellaAnswer({command,args})),listen:()=>()=>{}};});
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__hosts`);
  await projectRow(/^本机项目/).waitFor();
  assert.equal(await page.getByRole('button',{name:'Agent 编排'}).count(),1,'the local workbench keeps orchestration');

  await page.getByRole('button',{name:'执行主机',exact:true}).click();
  await page.getByRole('heading',{name:'执行主机',exact:true}).waitFor();
  await page.getByText('未连接',{exact:true}).waitFor();
  assert.equal(await page.getByRole('button',{name:'切到此主机'}).count(),0,'an unconnected host offers no target switch');
  await page.getByRole('button',{name:'检查',exact:true}).click();
  await page.getByRole('heading',{name:'连接确认'}).waitFor();
  const review=await page.locator('.capability-section').innerText();
  assert.match(review,new RegExp(serverId));assert.match(review,/\/srv\/中文 项目/);assert.match(review,/prestarted_unverified/);
  assert.match(review,/不会自动启动远端 daemon/);assert.match(review,/切到此主机|信任并连接/);
  assert.equal(await page.getByRole('button',{name:'信任并连接'}).isDisabled(),true,'trust requires an explicit acknowledgement');
  await page.locator('.capability-section .ack input').check();
  await page.getByRole('button',{name:'信任并连接'}).click();
  await until(async()=>(await page.locator('.execution-host-chip').innerText()).includes('测试主机'),'the titlebar names the active execution host');
  await page.getByText('执行主机为远端；资源监控、垃圾清理、浏览器和编排/自动化仅在本机可用。').waitFor();
  await projectRow(/^远端项目/).waitFor();
  assert.equal(await page.getByRole('button',{name:'Agent 编排'}).count(),0,'remote hosts must not expose the local-only pages');
  assert.equal(await page.getByRole('button',{name:'自动化'}).count(),0);
  assert.equal(await page.getByRole('button',{name:'任务来源'}).count(),0);
  assert.equal(await projectRow(/^本机项目/).count(),0,'a remote snapshot must not keep the local project list');
  await page.screenshot({path:join(output,'remote-workbench-1440.png')});

  await page.getByRole('button',{name:'关闭执行主机'}).click();
  await page.getByRole('button',{name:'添加项目'}).click();
  assert.match(await page.locator('.add-project-host').innerText(),/远端执行主机 · 测试主机/);
  assert.equal(await page.getByLabel('远端项目文件夹').count(),1,'remote paths are typed, never picked with a local dialog');
  assert.equal(await page.getByRole('button',{name:'从 URL 克隆'}).count(),0,'cloning stays a local operation');
  await page.getByRole('button',{name:'关闭添加项目'}).click();

  await page.getByRole('button',{name:'新建工作区',exact:true}).click();
  await page.getByRole('heading',{name:'新建工作树'}).waitFor();
  const location=await page.locator('.creation-location').innerText();
  assert.match(location,/远端执行主机 · 测试主机/);assert.match(location,/\/srv\/中文 项目\/仓库/);
  await page.getByText('高级设置').click();
  assert.match(await page.locator('.composer-advanced').innerText(),/执行主机：远端执行主机 · 测试主机；路径：\/srv\/中文 项目\/仓库/);
  await page.getByRole('button',{name:'关闭新建工作区'}).click();

  await page.getByRole('button',{name:'远端子工作树'}).click();
  await page.getByRole('button',{name:'展开工具栏'}).click();
  await page.getByRole('tab',{name:'检查'}).click();
  await page.getByRole('heading',{name:'远端主机暂不支持评审检查'}).waitFor();
  assert.match(await page.locator('.pr-checks-panel').innerText(),/不会用本机结果代替远端/);
  assert.deepEqual(requests.filter(r=>r.command==='pr_checks'),[],'the local PR reader ran for a remote workspace');

  const storage=await page.evaluate(()=>Object.keys(localStorage).filter(key=>key.includes('velella:workspace')).reduce((all,key)=>({...all,[key]:localStorage.getItem(key)}),{}));
  assert.equal(storage['velella:workspace'],'w-local','switching hosts must not rewrite the local selection');
  assert.equal(storage[`ssh:${hostId}:${serverId}:velella:workspace`],'w-child','the remote selection is stored under its own host identity');

  holdLate=true;const mark=requests.length;
  await page.getByRole('button',{name:'执行主机',exact:true}).click();
  await until(()=>requests.slice(mark).some(r=>snapshot(r)&&r.args?.target?.kind==='ssh'),'a remote snapshot request in flight');
  await page.getByRole('button',{name:'切回本机'}).click();
  await page.getByRole('button',{name:'关闭执行主机'}).click();
  await page.waitForTimeout(200);
  assert.equal(await projectRow(/^迟到的远端项目/).count(),0,'the held response arrived before the switch');
  release();
  await projectRow(/^本机项目/).waitFor();
  await page.waitForTimeout(400);
  assert.equal(await projectRow(/^迟到的远端项目/).count(),0,'a late remote response replaced the local workbench');
  assert.equal(await page.getByRole('button',{name:'Agent 编排'}).count(),1,'local pages return after switching back');

  const localChecks=requests.filter(r=>r.command==='pr_checks');
  assert.ok(localChecks.length>0,'the local PR reader must run again once the workbench is local');
  assert.deepEqual(localChecks.filter(r=>r.args?.target?.kind!=='local'),[],'a PR read ran without a local target');
  await until(async()=>/desktop tests/.test(await page.locator('.pr-checks-panel').innerText()),'the local PR panel never rendered');

  const remoteReads=requests.filter(r=>r.command==='rpc'&&r.args?.target?.kind==='ssh');
  assert.ok(remoteReads.length>0,'the remote workspace was never read through the machine channel');
  assert.deepEqual(remoteReads.map(r=>r.args.target).filter(target=>target.host_id!==hostId||target.server_id!==serverId),[],'a request carried a stale host identity');
  for(const command of LOCAL_ONLY_COMMANDS)assert.deepEqual(requests.filter(r=>r.command===command&&r.args?.target?.kind==='ssh'),[],command+' leaked a remote target into the local IPC');
  const localReads=requests.filter(r=>snapshot(r)&&!r.args?.target);
  assert.ok(localReads.length>0,'local reads keep their existing passthrough');
  assert.deepEqual(errors,[]);
  await page.setViewportSize({width:900,height:760});await page.screenshot({path:join(output,'local-after-remote-900.png')});
  writeFileSync(join(output,'result.json'),JSON.stringify({status:'passed',trustRequiresAck:true,remoteGating:true,composerShowsHost:true,prChecksRefusedRemote:true,storageNamespaced:true,lateRemoteResponseDiscarded:true,
    remoteReads:remoteReads.length,localReads:localReads.length,commands:[...new Set(requests.map(r=>r.command))].sort(),errors},null,2));
 }finally{release?.();await browser?.close();await server.close()}
});
