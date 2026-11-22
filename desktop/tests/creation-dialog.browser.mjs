// Run: node tests/creation-dialog.browser.mjs. All RPCs are intercepted; no real projects or agents are changed.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdirSync,writeFileSync,realpathSync} from 'node:fs';
import {resolve} from 'node:path';
const {chromium}=createRequire(import.meta.url)(process.env.PLAYWRIGHT_MODULE||'playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const evidence=resolve(root,`../artifacts/creation-dialog-workbench-20261024/browser-${Date.now()}`);
mkdirSync(evidence,{recursive:true});
const server=await createServer({root,configFile:false,plugins:[react(),{name:'fixture-no-runtime',configureServer(server){server.middlewares.use('/_velella',(_req,res)=>{res.statusCode=500;res.end('RPC must be intercepted by the fixture');});}}],server:{host:'127.0.0.1',port:0,strictPort:false,fs:{allow:[root,realpathSync(resolve(root,'node_modules'))]}},logLevel:'error'});
const checks=[],errors=[],requests=[],serverEffects=[];
const runtime='MOCK server journal/effects only; real React UI, no runtime, setup scripts, terminal processes or agents';
let browser,page;
try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});
  page=await browser.newPage({viewport:{width:1440,height:1000}});page.setDefaultTimeout(12000);
  page.on('pageerror',e=>errors.push(e.message));
  const projects=[{id:'p',name:'oa',path:'D:/oa',setup:'',direct:false},{id:'q',name:'鱼群',path:'D:/orcas',setup:'',direct:false},{id:'folder',name:'文档',path:'D:/documents',setup:'',direct:true}];
  const workspaces=projects.map(p=>({id:'main:'+p.id,project_id:p.id,name:p.name,path:p.path,kind:p.direct?'folder':'primary',position:0,archived:false,favorite:false}));
  const sources=['github','linear','gitlab','jira'].map(provider=>({id:provider,project_id:'p',provider,name:provider,scope:'team/repo',base_url:'https://example.test',login:'',jira_kind:'cloud'}));
  const items=sources.map(s=>({key:s.id+':7',source_id:s.id,project_id:'p',provider:s.provider,remote_id:'7',identifier:'#7',title:s.provider+' 修复项目创建',url:'https://example.test/issues/7',description:'选择项目后创建工作树。',state:'open',closed:false,updated_at:'2026-10-24'}));
  const links=[],layouts={},sessions={},tasks=[],journals={},controls={};
  let failLayout=false,failCreate=false,loseCreateResponse=false,omitCreation=false;
  let nextSetupState='pending',nextLaunchState='succeeded',nextSessionState='waiting_input',capabilities={model:true,effort:true,evidence:'target_cli_help'};
  const step=(creation,kind)=>creation.steps.find(s=>s.kind===kind);
  const effect=(id,kind,details={})=>serverEffects.push({mockOnly:true,request_id:id,kind,...details});
  // These are simulated SERVER effects, never real checkouts, scripts, agents or prompt delivery.
  function advance(creation){
    const id=creation.request_id,control=controls[id],req=creation.request;
    const setup=step(creation,'setup'),launch=step(creation,'launch'),prompt=step(creation,'prompt');
    if(!control)return;
    if(setup.state!=='skipped'){
      setup.state=control.setup;setup.error=['failed','uncertain'].includes(setup.state)?'模拟准备脚本失败；日志已保留':null;
      setup.descriptor={id:'mock-setup:'+id,directory:'D:/fixture/logs'};
    }
    if(!['succeeded','skipped'].includes(setup.state))return;
    if(req.launch){
      launch.state=control.launch;
      launch.error=['failed','uncertain'].includes(launch.state)?'模拟服务端启动失败；不会重新启动':null;
      if(launch.state==='succeeded'&&!launch.descriptor){
        const sid='mock-server-session:'+id;
        launch.descriptor={id:sid,directory:'D:/fixture/logs'};
        sessions[id]=[{id:sid,kind:'terminal',title:req.launch,program:req.launch,status:{state:control.session,exit:null,input_protocol:1,input_sequence:0}}];
        effect(id,'launch',{session_id:sid,launch:req.launch,launch_options:structuredClone(req.launch_options)});
      }
    }
    if(prompt.state==='pending'&&control.prompt==='succeeded'){
      assert.equal(req.initial_prompt?.mode,'submit');assert.equal(launch.state,'succeeded');
      assert.equal(sessions[id][0].status.state,'waiting_input','mock server delivery requires input readiness');
      prompt.state='succeeded';effect(id,'prompt',{text:req.initial_prompt.text,session_id:launch.descriptor.id});
    }
    creation.revision++;
  }
  function journal(req,workspace){
    const project=projects.find(p=>p.id===req.project_id),decision=req.setup==='inherit'?project.setup_default||'skip':req.setup;
    controls[req.request_id]={setup:nextSetupState,launch:nextLaunchState,session:nextSessionState,prompt:'pending',retryReady:false,dropStatus:0};
    const creation={schema:1,request_id:req.request_id,request:structuredClone(req),revision:1,frozen:{workspace:structuredClone(workspace),setup:{decision,script:project.setup}},steps:[
      {kind:'checkout',state:'succeeded'},
      {kind:'setup',state:decision==='run'&&project.setup.trim()?'pending':'skipped'},
      {kind:'launch',state:req.launch?'pending':'skipped'},
      {kind:'prompt',state:req.initial_prompt?.mode==='submit'?'pending':'skipped'},
    ].map(s=>({...s,attempt:0,error:null,descriptor:null}))};
    effect(req.request_id,'checkout');advance(creation);return creation;
  }
  await page.route('**/_velella',async route=>{
    const req=route.request().postDataJSON();requests.push(req);let data={};
    try{switch(req.cmd){
      case 'workbench.snapshot':data={projects,workspaces,tasks,source_links:links};break;
      case 'sources.list':case 'sources.sync':data={sources,links,repositories:[]};break;
      case 'worktree.list':data={branches:req.project_id==='p'?['main','origin/main','feature/login']:['main','release'],default_base:req.project_id==='q'?{error:'默认引用无法唯一推断'}:{resolved:{reference:'origin/main',oid:'a'.repeat(40),inferred:true}}};break;
      case 'sources.items':data={items:items.filter(i=>i.source_id===req.source_id),next:null};break;
      case 'sources.detail':data={item:items.find(i=>i.key===req.key)};break;
      case 'sandbox.capabilities':data={sandbox:{platform:'linux',backend:'fixture',available:false,reason:'fixture',restricted:{platform:'linux',backend:'landlock-seccomp',available:true,reason:null,filesystem_allowlist:true,process_namespace:false,network_modes:['denied'],experimental:true}}};break;
      case 'agent.check':data={available:true,authenticated:true,version:'fixture',launch_capabilities:capabilities};break;
      case 'project.create':{const p={id:'new-project',name:req.name,path:req.parent+'/'+req.name,setup:'',direct:false};projects.push(p);workspaces.push({id:'main:'+p.id,project_id:p.id,name:p.name,path:p.path,kind:'primary',position:0,archived:false,favorite:false});data={project:p};break;}
      case 'workbench.create':{
        if(failCreate){failCreate=false;await route.fulfill({json:{ok:false,error:'模拟接受前校验失败'}});return;}
        let w=workspaces.find(w=>w.id===req.request_id);if(!w){w={id:req.request_id,project_id:req.project_id,name:req.name,parent_workspace_id:req.parent_workspace_id,path:'D:/fixture/worktree/'+req.request_id,kind:'worktree',position:0,archived:false,favorite:false};workspaces.push(w);journals[req.request_id]=journal(req,w);}else assert.deepEqual(req,journals[req.request_id].request,'same ID must replay the exact frozen request');
        if(loseCreateResponse){loseCreateResponse=false;await route.fulfill({json:{ok:false,error:'模拟创建响应丢失'}});return;}
        data={workspace:w,...(!omitCreation?{creation:journals[req.request_id]}:{})};omitCreation=false;break;
      }
      case 'workbench.creation.status':{
        const creation=journals[req.request_id],control=controls[req.request_id];
        assert.deepEqual(req,{cmd:'workbench.creation.status',request_id:req.request_id});
        if(control?.dropStatus){control.dropStatus--;await route.fulfill({json:{ok:false,error:'模拟创建状态响应丢失'}});return;}
        if(creation)advance(creation);
        data={creation:creation||null,retry_ready:control?.retryReady===true};break;
      }
      case 'workbench.creation.retry':{
        const creation=journals[req.request_id],control=controls[req.request_id];
        assert.deepEqual(req,{cmd:'workbench.creation.retry',request_id:req.request_id,confirmed:true,reviewed_setup:creation.frozen.setup.script});
        assert.equal(step(creation,'setup').state,'failed');assert.equal(control.retryReady,true);
        effect(req.request_id,'setup-retry');control.setup='succeeded';control.retryReady=false;advance(creation);
        data={creation,retry_ready:false};break;
      }
      case 'task.create':{
        assert.match(req.request_id,/^[0-9a-f-]{36}$/i);let task=tasks.find(t=>t.id===req.request_id);
        if(!task){const p=projects.find(p=>p.id===req.project_id);task={id:req.request_id,project_id:p.id,direct:p.direct,parent_workspace_id:req.parent_workspace_id,creation_id:req.request_id,spec:req.spec,phase:'queued',reason:'',verified:false,created_at:'2026-10-24',worktree:p.path,branch:'fixture',native_id:null,agent:null,command:null,shell:null,pr_url:null,checks:[],status:null};tasks.push(task);journals[req.request_id]={request:structuredClone(req),frozen:{task},steps:[]};}else assert.deepEqual(req,journals[req.request_id].request);
        if(loseCreateResponse){loseCreateResponse=false;await route.fulfill({json:{ok:false,error:'模拟创建响应丢失'}});return;}data={task};break;
      }
      case 'workbench.source.attach':{const previous=links.find(l=>l.key===req.source_key);if(previous)previous.workspace_id=req.workspace_id;else links.push({...items.find(i=>i.key===req.source_key),workspace_id:req.workspace_id});break;}
      case 'workbench.terminal.open':{
        assert.equal(req.workspace_id,'main:folder','normal composer MUST NOT call terminal.open');
        const id='mock-direct-session:'+req.request_id;
        assert(!sessions[req.workspace_id]?.some(s=>s.id===id),'direct session must not be launched twice on layout retry');
        sessions[req.workspace_id]=[{id,kind:'terminal',title:req.launch,program:req.launch,status:{state:nextSessionState,exit:null,input_protocol:1,input_sequence:0}}];
        effect(req.request_id,'direct-launch',{session_id:id});data={session_id:id};break;
      }
      case 'workbench.sessions':data={sessions:sessions[req.workspace_id]||[]};break;
      case 'workbench.layout':data={layout:layouts[req.workspace_id]||{revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]}};break;
      case 'workbench.layout.save':if(failLayout){failLayout=false;await route.fulfill({json:{ok:false,error:'模拟布局保存失败'}});return;}layouts[req.workspace_id]=req.layout;data={layout:req.layout};break;
      case 'workbench.git':case 'task.diff':data={branch:'main',status:'',diff:'',staged:''};break;
      case 'workbench.files':case 'task.files':data={files:[]};break;
      case 'task.events':data={events:[]};break;
      case 'workbench.terminal.output':data={data:'',from:0,end:0,total:0,geometry:[]};break;
      case 'workbench.terminal.activity':data={enabled:true,events:[],next:0,total:0};break;
      case 'workbench.feedback.send':
        assert.equal(req.draft,true,'server-owned auto-submit must NEVER invoke feedback.send');
        assert.equal(req.initial,true);assert.equal(req.expected_input_sequence,0);
        assert.equal(sessions[req.workspace_id]?.find(s=>s.id===req.session_id)?.status.state,'waiting_input');
        data={receipt:{state:'drafted'}};break;
      case 'workbench.feedback.status':data={receipt:{state:'drafted'}};break;
      case 'workbench.terminal.resize':break;
      default:throw Error('Unexpected fixture RPC: '+req.cmd);
    }}catch(error){errors.push(String(error));await route.fulfill({json:{ok:false,error:'MOCK BLOCKED: '+String(error)}});return;}
    await route.fulfill({json:{ok:true,...data}});
  });
  await page.addInitScript(()=>{localStorage.setItem('velella:ui:theme',JSON.stringify('dark'));localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));});
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/`,{waitUntil:'domcontentloaded',timeout:60000});
  const open=async()=>{await page.locator('.ide-project-heading').getByRole('button',{name:'新建工作区',exact:true}).click();await page.getByRole('dialog').waitFor();await page.getByRole('dialog').getByLabel('开发项目').selectOption('p');await page.getByRole('dialog',{name:'新建工作树'}).waitFor();};
  const dialog=()=>page.getByRole('dialog');
  const create=()=>dialog().getByRole('button',{name:'创建并打开',exact:true});
  const creates=()=>requests.filter(r=>r.cmd==='workbench.create');
  await open();await dialog().getByLabel('创建后',{exact:true}).selectOption('codex');
  await dialog().getByLabel('基准分支',{exact:true}).selectOption('origin/main');
  await page.screenshot({path:resolve(evidence,'worktree-dark.png'),animations:'disabled'});
  await dialog().screenshot({path:resolve(evidence,'worktree-dialog.png'),animations:'disabled'});
  assert.equal(await dialog().locator('details').getAttribute('open'),null);
  const darkShell=await page.locator('.ide-shell').evaluate(el=>getComputedStyle(el).backgroundColor);
  assert.equal(await dialog().evaluate(el=>getComputedStyle(el).colorScheme),'light','creation form uses the shared workbench paper editor');
  await page.keyboard.press('Escape');await page.getByRole('button',{name:'切换浅色主题',exact:true}).click();await open();
  await dialog().getByLabel('创建后',{exact:true}).selectOption('codex');await dialog().getByLabel('基准分支').selectOption('origin/main');
  assert.notEqual(await page.locator('.ide-shell').evaluate(el=>getComputedStyle(el).backgroundColor),darkShell);
  assert.equal(await dialog().evaluate(el=>getComputedStyle(el).colorScheme),'light');
  await page.screenshot({path:resolve(evidence,'worktree-light.png'),animations:'disabled'});
  await page.setViewportSize({width:390,height:720});
  const bounds=await dialog().boundingBox();assert(bounds.x>=0&&bounds.width<=390&&bounds.y>=0&&bounds.y+bounds.height<=720);
  await page.screenshot({path:resolve(evidence,'worktree-narrow.png'),animations:'disabled'});
  await page.setViewportSize({width:1440,height:1000});
  await page.keyboard.press('Escape');assert.equal(await dialog().count(),0);await page.getByRole('button',{name:'切换深色主题',exact:true}).click();checks.push('dark/light/narrow layouts and Escape');

  await open();await dialog().getByLabel('搜索创建来源').fill('界面调整');await page.keyboard.press('Control+Enter');
  await page.waitForFunction(()=>!document.querySelector('.creation-dialog'));
  assert.equal(creates().length,1);assert.equal(creates()[0].name,'界面调整');assert(!Object.hasOwn(creates()[0],'base'));assert.equal(creates()[0].setup,'inherit');assert.deepEqual(creates()[0].execution,{mode:'native'});assert.equal(creates()[0].parent_workspace_id,null);
  checks.push('sidebar entry and Ctrl+Enter create exactly one workspace');

  await open();await dialog().getByRole('tab',{name:'分支',exact:true}).click();
  await dialog().getByRole('button',{name:'feature/login',exact:true}).click();
  assert.equal(await dialog().getByLabel('基准分支').inputValue(),'feature/login');
  await dialog().getByRole('switch',{name:'创建更多'}).check();await create().click();
  await dialog().getByRole('status').filter({hasText:'可以继续创建'}).waitFor();
  assert.equal(await dialog().getByLabel('搜索创建来源').inputValue(),'');
  await dialog().getByLabel('搜索创建来源').fill('第二个工作树');
  await dialog().getByRole('switch',{name:'创建更多'}).uncheck();await create().click();
  await page.waitForFunction(()=>!document.querySelector('.creation-dialog'));
  assert.equal(creates().length,3);assert.notEqual(creates()[1].request_id,creates()[2].request_id);assert.equal(creates()[1].base,'feature/login');assert(!Object.hasOwn(creates()[2],'base'));
  assert.equal(creates()[2].name,'第二个工作树');checks.push('branch selection and create-more reset with distinct request IDs');

  await open();await dialog().getByLabel('基准分支').selectOption('origin/main');
  await dialog().getByLabel('开发项目').selectOption('q');assert.equal(await dialog().getByLabel('基准分支').inputValue(),'');
  await dialog().getByText('项目默认基准错误：默认引用无法唯一推断',{exact:true}).waitFor();
  assert.match(await dialog().locator('.creation-location').innerText(),/D:\/orcas/);
  await dialog().getByLabel('基准分支').selectOption('release');
  await dialog().getByRole('tab',{name:'名称',exact:true}).click();await dialog().getByRole('textbox',{name:'名称',exact:true}).fill('其他项目任务');
  await create().click();await page.waitForFunction(()=>!document.querySelector('.creation-dialog'));
  assert.equal(creates().at(-1).project_id,'q');checks.push('project switch resets base and uses selected project');

  for(const provider of ['github','linear','gitlab','jira']){
    await open();await dialog().getByRole('tab',{name:{github:'GitHub',linear:'Linear',gitlab:'GitLab',jira:'Jira'}[provider],exact:true}).click();
    await dialog().getByRole('button').filter({hasText:`#7 · ${provider} 修复项目创建`}).click();
    await dialog().locator('.creation-selected').waitFor();
    if(provider==='github'){
      await dialog().getByLabel('创建后').selectOption('codex');nextLaunchState='running';
      const count=creates().length;await create().click();await dialog().getByRole('status').filter({hasText:'启动：运行中'}).waitFor();
      const id=creates().at(-1).request_id;
      assert.equal(creates().length,count+1);assert(await dialog().getByLabel('开发项目').isDisabled());
      assert(await dialog().getByRole('button',{name:'等待创建步骤…',exact:true}).isDisabled());
      assert(!requests.some(r=>r.cmd==='workbench.layout.save'&&r.workspace_id===id),'layout waits for server launch');
      controls[id].launch='succeeded';nextLaunchState='succeeded';
      await page.waitForFunction(()=>!document.querySelector('.creation-dialog'));
      assert.equal(creates().length,count+1,'observing launch completion must not recreate workspace');
      assert.equal(serverEffects.filter(e=>e.request_id===id&&e.kind==='launch').length,1);
    }else{await create().click();await page.waitForFunction(()=>!document.querySelector('.creation-dialog'));}
    const req=creates().at(-1);assert.equal(req.source_key,provider+':7');
    assert.equal(links.find(l=>l.key===req.source_key)?.workspace_id,req.request_id);
    assert.equal(requests.filter(r=>r.cmd==='workbench.terminal.open').length,0);
  }
  assert.equal(requests.filter(r=>['workbench.terminal.input','sources.mutate','task.create'].includes(r.cmd)).length,0);
  checks.push('all four sources attach; running server launch is observed without client launch or task auto-submission');

  await open();await dialog().getByLabel('开发项目').selectOption('folder');
  assert(await dialog().getByRole('switch').isDisabled());const before=creates().length;
  await dialog().getByRole('button',{name:'打开工作区',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.creation-dialog'));
  assert.equal(creates().length,before);checks.push('ordinary folder opens without attempting worktree creation');

  const advanced=async()=>{if(await dialog().locator('details').getAttribute('open')===null)await dialog().locator('summary').click();};
  const closed=()=>page.waitForFunction(()=>!document.querySelector('.creation-dialog'));
  const launches=()=>requests.filter(r=>r.cmd==='workbench.terminal.open');
  const managed=()=>requests.filter(r=>r.cmd==='task.create');
  const draftFor=id=>page.evaluate(id=>{const prefix='velella:source-draft:'+id;const key=Object.keys(localStorage).find(key=>key===prefix||key.startsWith(prefix+':'));return key?JSON.parse(localStorage.getItem(key)):null;},id);
  await open();await dialog().getByLabel('创建后').selectOption('codex');await advanced();
  assert.equal(await dialog().getByLabel('智能体运行方式').inputValue(),'native');
  assert(await dialog().getByLabel('输入就绪后自动提交初始提示词；绝不处理权限确认对话框。',{exact:true}).isDisabled(),'Codex readiness is unverified; auto-submit remains blocked');
  assert.deepEqual(await dialog().getByLabel('推理强度',{exact:true}).locator('option').evaluateAll(options=>options.map(o=>o.value)),['','none','minimal','low','medium','high','xhigh']);
  await dialog().getByLabel('工作目标',{exact:true}).fill('原生提示词，不创建受管任务');
  await dialog().getByLabel('模型',{exact:true}).fill('requested-model');await dialog().getByLabel('推理强度',{exact:true}).selectOption('xhigh');
  await dialog().getByLabel('父级工作区',{exact:true}).selectOption('main:q');
  await dialog().getByLabel('自定义基准引用',{exact:true}).fill('');
  failLayout=true;const launchBefore=launches().length;await create().click();await dialog().getByRole('alert').filter({hasText:'模拟布局保存失败'}).waitFor();
  const nativeId=creates().at(-1).request_id;assert.equal(await draftFor(nativeId),null,'draft waits until session layout persists');
  await dialog().getByRole('button',{name:'继续打开',exact:true}).click();await closed();
  assert.equal(managed().length,0);assert.equal(launches().length,launchBefore);
  const nativeRequest=creates().at(-1);
  assert.equal(nativeRequest.parent_workspace_id,'main:q');assert(!Object.hasOwn(nativeRequest,'base'));
  assert.equal(nativeRequest.launch,'codex');assert.deepEqual(nativeRequest.launch_options,{model:'requested-model',effort:'xhigh'});
  assert.deepEqual(nativeRequest.initial_prompt,{text:'原生提示词，不创建受管任务',mode:'draft'});
  assert.equal(serverEffects.filter(e=>e.request_id===nativeId&&e.kind==='launch').length,1);
  assert.equal(creates().filter(r=>r.request_id===nativeId).length,1);
  const nativeDraft=await draftFor(nativeId);assert.equal(nativeDraft.text,nativeRequest.initial_prompt.text);assert.equal(nativeDraft.source,'initial:'+nativeId);assert(!nativeDraft.auto_submit);
  assert.equal(nativeDraft.creation_id,nativeId);assert.equal(nativeDraft.session_id,step(journals[nativeId],'launch').descriptor.id);
  assert.equal(layouts[nativeId].panes[0].tabs.filter(t=>t.target===nativeDraft.session_id).length,1);
  checks.push('native prompt/options belong to create; layout retry uses journal descriptor once and draft retains creation_id');

  await open();await dialog().getByLabel('创建后').selectOption('claude-code');await advanced();
  assert.deepEqual(await dialog().getByLabel('推理强度',{exact:true}).locator('option').evaluateAll(options=>options.map(o=>o.value)),['','low','medium','high','xhigh','max']);
  capabilities={model:false,effort:false,evidence:'target_cli_help'};
  await dialog().getByRole('button',{name:'检查 CLI 与登录',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('input[aria-label="模型"]')?.disabled);
  assert(await dialog().getByLabel('推理强度',{exact:true}).isDisabled());
  await create().click();await closed();assert.deepEqual(creates().at(-1).launch_options,{});assert.equal(creates().at(-1).launch,'claude-code');
  assert.equal(launches().length,launchBefore);
  capabilities={model:true,effort:true,evidence:'target_cli_help'};checks.push('CLI help evidence disables unsupported server launch flags');

  await open();await dialog().getByLabel('创建后').selectOption('claude-code');await advanced();
  await dialog().getByLabel('智能体运行方式').selectOption('managed');assert.equal(await dialog().getByLabel('准备脚本策略').inputValue(),'run');assert(await create().isDisabled());
  await dialog().getByLabel('工作目标',{exact:true}).fill('明确受管目标');await dialog().getByLabel('推理强度',{exact:true}).selectOption('max');
  await dialog().getByLabel('父级工作区',{exact:true}).selectOption('main:q');
  await dialog().getByLabel('我确认本次目标、原生权限、准备脚本和验证设置。',{exact:true}).check();
  loseCreateResponse=true;await create().click();await dialog().getByRole('alert').filter({hasText:'模拟创建响应丢失'}).waitFor();
  assert(await dialog().getByLabel('工作目标',{exact:true}).isDisabled());await create().click();await closed();
  assert.equal(managed().length,2);assert.deepEqual(managed()[0],managed()[1]);assert.equal(managed()[0].parent_workspace_id,'main:q');assert(!Object.hasOwn(managed()[0].spec,'base'));
  assert.deepEqual(managed()[0].spec.launch,{effort:'max'});assert.equal(managed()[0].spec.policy.permissions,'ask');assert.equal(managed()[0].spec.on_done,'review');
  assert.equal(await draftFor('task:'+managed()[0].request_id),null);checks.push('explicit managed task request frozen across retries with native ask/review and no draft');

  await open();await dialog().getByLabel('搜索创建来源').fill('原请求');failCreate=true;await create().click();
  await dialog().getByRole('alert').filter({hasText:'模拟接受前校验失败'}).waitFor();const rejectedId=creates().at(-1).request_id;
  await dialog().getByRole('button',{name:'重新编辑并开始新请求',exact:true}).click();await dialog().getByLabel('搜索创建来源').fill('新请求');await create().click();await closed();
  assert.notEqual(creates().at(-1).request_id,rejectedId);checks.push('explicit new request after pre-acceptance rejection');

  await open();await dialog().getByLabel('搜索创建来源').fill('冻结的普通创建');await dialog().getByLabel('创建后').selectOption('codex');await advanced();
  const frozenText='  冻结原始提示词\n保留空白与选项  ';
  await dialog().getByLabel('工作目标',{exact:true}).fill(frozenText);await dialog().getByLabel('模型',{exact:true}).fill('frozen-model');await dialog().getByLabel('推理强度',{exact:true}).selectOption('high');
  failCreate=true;await create().click();await dialog().getByRole('alert').filter({hasText:'模拟接受前校验失败'}).waitFor();
  const originalCreate=structuredClone(creates().at(-1));
  for(const label of ['工作目标','模型','推理强度','创建后','开发项目'])assert(await dialog().getByLabel(label,{exact:true}).isDisabled());
  assert.deepEqual(originalCreate.initial_prompt,{text:frozenText,mode:'draft'});assert.deepEqual(originalCreate.launch_options,{model:'frozen-model',effort:'high'});
  // The next response is lost AFTER the mock server accepted and launched this exact request.
  loseCreateResponse=true;await create().click();await dialog().getByRole('alert').filter({hasText:'模拟创建响应丢失'}).waitFor();
  await create().click();await closed();
  const replays=creates().filter(r=>r.request_id===originalCreate.request_id);assert.equal(replays.length,3);for(const replay of replays)assert.deepEqual(replay,originalCreate);
  assert.equal(serverEffects.filter(e=>e.request_id===originalCreate.request_id&&e.kind==='launch').length,1);
  await open();await dialog().getByLabel('搜索创建来源').fill('响应丢失后查询原记录');await dialog().getByLabel('创建后').selectOption('terminal');
  loseCreateResponse=true;const lostBefore=creates().length;await create().click();
  await dialog().getByRole('alert').filter({hasText:'模拟创建响应丢失'}).waitFor();await dialog().getByRole('button',{name:'重新编辑并开始新请求',exact:true}).click();
  await dialog().getByRole('alert').filter({hasText:'原请求已被接受'}).waitFor();await dialog().getByRole('button',{name:'检查原请求状态',exact:true}).click();await closed();assert.equal(creates().length,lostBefore+1);
  assert.equal(launches().length,launchBefore);
  checks.push('frozen prompt/options replay unchanged before and after acceptance; lost response observes original journal without relaunch');

  const observe=id=>page.waitForResponse(response=>response.request().postDataJSON()?.cmd==='workbench.creation.status'&&response.request().postDataJSON()?.request_id===id);
  const noPromptWrites=id=>assert(!requests.some(r=>['workbench.feedback.send','workbench.feedback.status','workbench.terminal.input'].includes(r.cmd)&&r.workspace_id===id),'creation-owned auto-submit is a read-only journal observer');
  await open();await dialog().getByLabel('创建后').selectOption('claude-code');await advanced();
  await dialog().getByLabel('工作目标',{exact:true}).fill('仅在输入就绪时提交');
  await dialog().getByLabel('输入就绪后自动提交初始提示词；绝不处理权限确认对话框。',{exact:true}).check();nextSessionState='waiting_permission';await create().click();await closed();
  const autoId=creates().at(-1).request_id,autoDraft=await draftFor(autoId);assert.equal(autoDraft.auto_submit,true);assert.equal(autoDraft.creation_id,autoId);
  assert.deepEqual(creates().at(-1).initial_prompt,{text:autoDraft.text,mode:'submit'});
  await observe(autoId);noPromptWrites(autoId);assert.equal(step(journals[autoId],'prompt').state,'pending');
  sessions[autoId][0].status.state='waiting_input';await observe(autoId);noPromptWrites(autoId);
  assert.equal((await draftFor(autoId)).state,'waiting','ready input is not permission for a client submission');
  controls[autoId].dropStatus=1;await observe(autoId);await observe(autoId);noPromptWrites(autoId);
  // Persisted local draft is deliberately removed: a new UI process must recover immutable server intent.
  await page.evaluate(id=>{for(const key of Object.keys(localStorage))if(key.startsWith('velella:source-draft:'+id))localStorage.removeItem(key);},autoId);
  await page.reload({waitUntil:'domcontentloaded'});
  await page.waitForFunction(id=>Object.keys(localStorage).some(key=>key.startsWith('velella:source-draft:'+id)),autoId);
  const recoveredAuto=await draftFor(autoId);assert.equal(recoveredAuto.text,journals[autoId].request.initial_prompt.text);assert.equal(recoveredAuto.session_id,step(journals[autoId],'launch').descriptor.id);assert.equal(recoveredAuto.creation_id,autoId);assert.equal(recoveredAuto.auto_submit,true);
  await observe(autoId);noPromptWrites(autoId);
  // Explicit MOCK SERVER effect, not an agent run or a UI feedback.send.
  controls[autoId].prompt='succeeded';advance(journals[autoId]);controls[autoId].dropStatus=1;
  await observe(autoId);await page.waitForFunction(id=>{const prefix='velella:source-draft:'+id;return Object.keys(localStorage).some(key=>key.startsWith(prefix)&&JSON.parse(localStorage.getItem(key)).state==='submitted');},autoId);
  noPromptWrites(autoId);assert.equal(serverEffects.filter(e=>e.request_id===autoId&&e.kind==='prompt').length,1);
  const reopenedObservation=observe(autoId);await page.reload({waitUntil:'domcontentloaded'});await reopenedObservation;noPromptWrites(autoId);
  assert.equal(creates().filter(r=>r.request_id===autoId).length,1);assert.equal(launches().length,launchBefore);nextSessionState='waiting_input';
  checks.push('auto-submit only observes immutable creation journal across permission/ready states, dropped responses and reload recovery; zero UI delivery writes');

  const managedItem={...items[0],key:'github:8',remote_id:'8',identifier:'#8',title:'显式受管来源'};items.push(managedItem);
  await open();await dialog().getByRole('tab',{name:'GitHub',exact:true}).click();await dialog().getByRole('button').filter({hasText:'#8 · 显式受管来源'}).click();
  await dialog().getByLabel('创建后').selectOption('codex');await dialog().getByLabel('智能体运行方式').selectOption('managed');await advanced();
  await dialog().getByLabel('我确认本次目标、原生权限、准备脚本和验证设置。',{exact:true}).check();await create().click();await closed();
  assert.equal(managed().at(-1).source_key,'github:8');assert.equal(links.find(l=>l.key==='github:8').workspace_id,'task:'+managed().at(-1).request_id);
  assert.equal(await draftFor('task:'+managed().at(-1).request_id),null);checks.push('source item explicitly enters managed task and retains source link');

  await open();await dialog().getByLabel('开发项目').selectOption('folder');await dialog().getByLabel('创建后').selectOption('claude-code');await advanced();
  await dialog().getByLabel('工作目标',{exact:true}).fill('直接文件夹初始草稿');await dialog().getByLabel('模型',{exact:true}).fill('folder-model');
  const directCreateBefore=creates().length,directLaunchBefore=launches().length;failLayout=true;
  await dialog().getByRole('button',{name:'打开工作区',exact:true}).click();await dialog().getByRole('alert').filter({hasText:'模拟布局保存失败'}).waitFor();
  assert.equal(launches().length,directLaunchBefore+1);assert.equal(await draftFor('main:folder'),null);
  const directRequest=launches().at(-1);assert.deepEqual(directRequest,{cmd:'workbench.terminal.open',workspace_id:'main:folder',launch:'claude-code',request_id:directRequest.request_id,launch_options:{model:'folder-model'},initial_prompt:{text:'直接文件夹初始草稿',mode:'draft'}});
  await dialog().getByRole('button',{name:'继续打开',exact:true}).click();await closed();
  assert.equal(creates().length,directCreateBefore);assert.equal(launches().length,directLaunchBefore+1);
  const directDraft=await draftFor('main:folder');assert.equal(directDraft.text,directRequest.initial_prompt.text);assert.equal(directDraft.creation_id,undefined);
  checks.push('direct folder alone uses terminal.open with initial_prompt/options; layout retry never opens a second session');

  projects[2].setup='folder setup fixture';projects[2].setup_default='run';await page.reload({waitUntil:'domcontentloaded'});
  await open();await dialog().getByLabel('开发项目').selectOption('folder');await advanced();
  assert.equal(await dialog().getByLabel('准备脚本策略').inputValue(),'skip');assert(await dialog().getByLabel('准备脚本策略').isDisabled());assert(await dialog().getByLabel('父级工作区').isDisabled());
  await dialog().getByLabel('创建后').selectOption('claude-code');await dialog().getByLabel('智能体运行方式').selectOption('managed');
  assert.equal(await dialog().getByLabel('准备脚本策略').inputValue(),'run');await dialog().getByLabel('工作目标',{exact:true}).fill('直接目录受管任务');
  await dialog().getByLabel('我已审阅本次准备脚本，并同意执行。',{exact:true}).check();await dialog().getByLabel('我确认本次目标、原生权限、准备脚本和验证设置。',{exact:true}).check();
  await dialog().getByRole('button',{name:'打开工作区',exact:true}).click();await closed();assert.equal(managed().at(-1).project_id,'folder');assert.equal(managed().at(-1).parent_workspace_id,null);assert.equal(managed().at(-1).reviewed_setup,'folder setup fixture');
  checks.push('direct folder ordinary setup fixed skip; explicit managed task preserves legacy setup semantics');

  projects[0].setup='  Write-Output "setup fixture"\n';projects[0].setup_default='run';await page.reload({waitUntil:'domcontentloaded'});
  await open();await dialog().getByLabel('创建后').selectOption('codex');await advanced();
  assert(await create().isDisabled());await dialog().getByLabel('我已审阅本次准备脚本，并同意执行。',{exact:true}).check();
  await dialog().getByLabel('准备脚本策略').selectOption('skip');await dialog().getByLabel('准备脚本策略').selectOption('run');assert(await create().isDisabled());
  await dialog().getByLabel('我已审阅本次准备脚本，并同意执行。',{exact:true}).check();
  const pendingBefore=launches().length,countBefore=creates().length;nextSetupState='running';await create().click();
  await dialog().getByRole('status').filter({hasText:'准备脚本：运行中'}).waitFor();assert.equal(launches().length,pendingBefore);
  const setupId=creates().at(-1).request_id;
  assert(await dialog().getByRole('button',{name:'等待创建步骤…',exact:true}).isDisabled());
  assert.equal(serverEffects.filter(e=>e.request_id===setupId&&e.kind==='launch').length,0);
  controls[setupId].setup='failed';await dialog().getByText('模拟准备脚本失败；日志已保留',{exact:true}).waitFor();
  assert.equal(creates().length,countBefore+1);assert.equal(launches().length,pendingBefore);
  assert(await dialog().getByRole('button',{name:'确认重试准备脚本',exact:true}).isDisabled());
  assert(await dialog().getByLabel('我已核对保留日志，并确认重试上述冻结脚本。',{exact:true}).isDisabled());
  await dialog().getByText('等待原脚本进程确认退出后才能重试。',{exact:true}).waitFor();
  controls[setupId].retryReady=true;await observe(setupId);
  await page.waitForFunction(()=>[...document.querySelectorAll('.creation-dialog label')].some(el=>el.textContent.includes('我已核对保留日志')&&el.querySelector('input')?.disabled===false));
  assert(await dialog().getByRole('button',{name:'确认重试准备脚本',exact:true}).isDisabled(),'retry_ready alone is not confirmation');
  await dialog().getByLabel('我已核对保留日志，并确认重试上述冻结脚本。',{exact:true}).check();await dialog().getByRole('button',{name:'确认重试准备脚本',exact:true}).click();await closed();
  assert.equal(launches().length,pendingBefore);assert.equal(creates().length,countBefore+1);
  assert.equal(serverEffects.filter(e=>e.request_id===setupId&&e.kind==='launch').length,1);
  assert.equal(requests.filter(r=>r.cmd==='workbench.creation.retry'&&r.request_id===setupId).length,1);
  assert.equal(requests.filter(r=>r.cmd==='workbench.creation.retry').at(-1).reviewed_setup,projects[0].setup);checks.push('setup failure waits for retry_ready and explicit exact-script confirmation; server resumes once without checkout or client launch replay');

  await open();await dialog().getByLabel('创建后').selectOption('codex');await advanced();
  await dialog().getByLabel('执行模式',{exact:true}).selectOption('restricted');
  assert(await dialog().getByLabel('模型',{exact:true}).isDisabled());assert(await dialog().getByLabel('推理强度',{exact:true}).isDisabled());
  assert(await dialog().getByLabel('智能体运行方式').locator('option[value="managed"]').evaluate(option=>option.disabled));assert(await create().isDisabled());
  await dialog().getByText('受限工具不能运行普通工作区准备脚本，请明确选择 skip（跳过）。',{exact:true}).waitFor();
  await dialog().getByLabel('准备脚本策略').selectOption('skip');await dialog().getByLabel('工作目标',{exact:true}).fill('受限模式不能自动投递');
  assert(await create().isDisabled());assert(await dialog().getByLabel('输入就绪后自动提交初始提示词；绝不处理权限确认对话框。',{exact:true}).isDisabled());
  await dialog().getByRole('alert').filter({hasText:'受限工具模式不支持初始提示词投递'}).waitFor();
  const restrictedBefore=creates().length;await page.keyboard.press('Control+Enter');assert.equal(creates().length,restrictedBefore);
  await dialog().getByLabel('工作目标',{exact:true}).fill('');await create().click();await closed();
  const restrictedRequest=creates().at(-1);
  assert.deepEqual(restrictedRequest.execution,{mode:'restricted'});assert(!Object.hasOwn(restrictedRequest,'sandbox'));assert.deepEqual(restrictedRequest.launch_options,{});assert.equal(restrictedRequest.launch,'codex');assert(!Object.hasOwn(restrictedRequest,'initial_prompt'));
  assert.equal(launches().length,pendingBefore);assert.equal(await draftFor(restrictedRequest.request_id),null);
  assert(!requests.some(r=>r.workspace_id===restrictedRequest.request_id&&['workbench.feedback.send','workbench.terminal.input'].includes(r.cmd)));
  checks.push('restricted creation blocks nonempty goal until cleared, managed/model/effort/auto-submit, and setup until explicit skip; no prompt automation');

  await open();await advanced();await dialog().getByLabel('我已审阅本次准备脚本，并同意执行。',{exact:true}).check();nextSetupState='uncertain';
  const retriesBefore=requests.filter(r=>r.cmd==='workbench.creation.retry').length;await create().click();await dialog().getByRole('status').filter({hasText:'结果不确定'}).waitFor();
  const uncertainId=creates().at(-1).request_id;controls[uncertainId].retryReady=true;
  await dialog().getByRole('button',{name:'检查原请求状态',exact:true}).click();
  assert.equal(await dialog().getByRole('button',{name:'确认重试准备脚本',exact:true}).count(),0,'Uncertain cannot retry even with retry_ready');
  assert.equal(requests.filter(r=>r.cmd==='workbench.creation.retry').length,retriesBefore);
  await dialog().getByRole('button',{name:'打开已保留工作区',exact:true}).click();await closed();checks.push('uncertain setup never offers retry even if server reports retry_ready; retained workspace remains accessible');

  // Close the composer while setup runs, then reload the selected retained UUID workspace.
  // Only the mock server can finish setup/launch; SourceTaskDraft recovers from request + descriptor.
  await open();await dialog().getByLabel('创建后').selectOption('codex');await advanced();
  await dialog().getByLabel('工作目标',{exact:true}).fill('进程重开后恢复的不可变草稿');
  await dialog().getByLabel('我已审阅本次准备脚本，并同意执行。',{exact:true}).check();nextSetupState='running';nextLaunchState='running';
  const reopenStart=requests.length;await create().click();await dialog().getByRole('status').filter({hasText:'准备脚本：运行中'}).waitFor();
  const reopenId=creates().at(-1).request_id,reopenRequest=structuredClone(journals[reopenId].request);
  await dialog().getByRole('button',{name:'打开已保留工作区',exact:true}).click();await closed();
  const firstReopen=observe(reopenId);await page.reload({waitUntil:'domcontentloaded'});await firstReopen;
  assert.equal(await draftFor(reopenId),null);controls[reopenId].setup='succeeded';await observe(reopenId);
  assert.equal(step(journals[reopenId],'launch').state,'running');assert.equal(await draftFor(reopenId),null);
  controls[reopenId].launch='succeeded';
  await page.waitForFunction(id=>Object.keys(localStorage).some(key=>key.startsWith('velella:source-draft:'+id)&&JSON.parse(localStorage.getItem(key)).state==='drafted'),reopenId);
  const reopenedDraft=await draftFor(reopenId);assert.equal(reopenedDraft.creation_id,reopenId);assert.equal(reopenedDraft.text,reopenRequest.initial_prompt.text);assert.equal(reopenedDraft.session_id,step(journals[reopenId],'launch').descriptor.id);
  assert.deepEqual(journals[reopenId].request,reopenRequest);
  assert.equal(requests.slice(reopenStart).filter(r=>r.cmd==='workbench.create').length,1);
  assert(!requests.slice(reopenStart).some(r=>['workbench.terminal.open','workbench.creation.retry'].includes(r.cmd)));
  assert.equal(serverEffects.filter(e=>e.request_id===reopenId&&e.kind==='launch').length,1);
  checks.push('close/reload during setup and launch recovers UUID prompt from immutable request and server descriptor without another creation or launch');

  for(const launchState of ['failed','uncertain']){
    await open();await dialog().getByLabel('创建后').selectOption('codex');await advanced();await dialog().getByLabel('准备脚本策略').selectOption('skip');
    nextLaunchState=launchState;const start=requests.length;await create().click();
    await dialog().getByRole('status').filter({hasText:launchState==='failed'?'启动：失败':'启动：结果不确定'}).waitFor();
    const id=creates().at(-1).request_id;assert(await dialog().getByRole('button',{name:'继续打开',exact:true}).isDisabled());
    await dialog().getByRole('button',{name:'检查原请求状态',exact:true}).click();
    assert.equal(requests.slice(start).filter(r=>r.cmd==='workbench.create').length,1);
    assert(!requests.slice(start).some(r=>['workbench.terminal.open','workbench.creation.retry','workbench.layout.save','workbench.feedback.send'].includes(r.cmd)));
    assert.equal(serverEffects.filter(e=>e.request_id===id&&e.kind==='launch').length,0);
    await dialog().getByRole('button',{name:'打开已保留工作区',exact:true}).click();await closed();
  }
  checks.push('failed and uncertain launch journals are observation-only; UI cannot relaunch or save a guessed session');

  nextLaunchState='succeeded';await open();await dialog().getByLabel('创建后').selectOption('codex');await advanced();await dialog().getByLabel('准备脚本策略').selectOption('skip');
  omitCreation=true;const unknownStart=requests.length;await create().click();await dialog().getByRole('alert').filter({hasText:'原创建记录尚未确认会话'}).waitFor();
  const unknownId=creates().at(-1).request_id;
  assert(!requests.slice(unknownStart).some(r=>r.cmd==='workbench.terminal.open'));
  await dialog().getByRole('button',{name:'检查原请求状态',exact:true}).click();await closed();
  assert.equal(requests.slice(unknownStart).filter(r=>r.cmd==='workbench.create').length,1);assert.equal(serverEffects.filter(e=>e.request_id===unknownId&&e.kind==='launch').length,1);
  checks.push('missing creation response cannot trigger client launch; explicit observation recovers original server session');

  await open();await dialog().getByRole('button',{name:'创建项目文件夹',exact:true}).click();
  await page.getByRole('dialog',{name:'创建项目文件夹'}).waitFor();
  await dialog().getByLabel('项目名称').fill('新项目');await dialog().getByLabel('保存位置').fill('D:/projects');
  await page.screenshot({path:resolve(evidence,'project-dark.png'),animations:'disabled'});
  await dialog().screenshot({path:resolve(evidence,'project-dialog.png'),animations:'disabled'});await page.keyboard.press('Control+Enter');
  await page.getByRole('dialog',{name:'新建工作树'}).waitFor();
  assert.equal(await dialog().getByLabel('开发项目').inputValue(),'new-project');
  assert.equal(requests.filter(r=>r.cmd==='project.create').length,1);checks.push('create-project entry returns to worktree creation with the new project selected');
  assert.deepEqual(errors,[]);assert(checks.length>=18,'preserve all original scenario coverage');
  assert(launches().every(r=>r.workspace_id==='main:folder'),'normal creation never invokes terminal.open');
  assert(requests.filter(r=>r.cmd==='workbench.feedback.send').every(r=>r.draft===true),'all client prompt writes are guarded drafts, never auto-submit');
  writeFileSync(resolve(evidence,'result.json'),JSON.stringify({runtime,checks,errors,requests,serverEffects},null,2));
  console.log(JSON.stringify({ok:true,runtime,checks,evidence}));
}catch(error){if(page)await page.screenshot({path:resolve(evidence,'failure.png')}).catch(()=>{});writeFileSync(resolve(evidence,'failure.json'),JSON.stringify({runtime,error:String(error),checks,errors,requests,serverEffects},null,2));throw error;}
finally{await browser?.close();await server.close();}
