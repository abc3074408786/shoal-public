import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
// MOCK RPC integration only: no provider writes, checkout, setup, terminal or agent actually runs.
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';
import {mkdirSync,writeFileSync,realpathSync} from 'node:fs';
import {resolve} from 'node:path';
const {chromium}=createRequire(import.meta.url)('playwright');
const root=fileURLToPath(new URL('..',import.meta.url));
const evidence=resolve(root,`../artifacts/task-sources-20261018/browser-${Date.now()}`);mkdirSync(evidence,{recursive:true});
const checks=[],errors=[],requests=[],serverEffects=[];
const runtime='MOCK provider RPCs and server creation journal/effects; no real runtime or agents';
const server=await createServer({root,configFile:false,plugins:[react(),{name:'task-sources-no-runtime',configureServer(vite){vite.middlewares.use('/_velella',(_req,res)=>{errors.push('Vite guard: unintercepted RPC');res.statusCode=500;res.end('MOCK ONLY: RPC must be intercepted');});}}],server:{host:'127.0.0.1',port:0,strictPort:false,fs:{allow:[root,realpathSync(resolve(root,'node_modules'))]}},logLevel:'error'});
let browser,page;
try{
  await server.listen();browser=await chromium.launch({channel:'msedge',headless:true});
  page=await browser.newPage({viewport:{width:1440,height:900},serviceWorkers:'block'});page.setDefaultTimeout(12000);page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error'&&m.text().includes('same key'))errors.push(m.text())});
  const projects=[{id:'p',name:'开发项目',path:'C:\\fixture\\project',setup:'',direct:false},{id:'q',name:'其他开发项目',path:'C:\\fixture\\other',setup:'',direct:false}];
  const names={github:'GitHub',gitlab:'GitLab',linear:'Linear',jira:'Jira'};
  const sources=Object.keys(names).map((provider,index)=>({id:`10000000-0000-4000-8000-00000000000${index}`,project_id:'p',provider,name:names[provider],scope:provider==='jira'?'DEV':provider==='linear'?'team-id':'team/repo',base_url:provider==='jira'?'https://example.atlassian.net':provider==='linear'?'https://api.linear.app':`https://${provider}.com`,login:'',jira_kind:'cloud'}));
  const item=s=>({key:s.id+':7',source_id:s.id,project_id:'p',provider:s.provider,remote_id:'7',identifier:['github','gitlab'].includes(s.provider)?'#7':'DEV-7',title:names[s.provider]+' 中文需求',url:'https://example.test/issues/7',description:'## 需求说明\n实现任务来源。\n\n## 验收\n- [x] 从任务开始开发\n- [ ] 返回任务\n\n```ts\nconst ready = true;\n```\n\n**验收负责人** · [需求说明](https://example.test/spec)\n\n<img src=x onerror=alert(1)> [危险链接](javascript:alert%281%29)',state:'待处理',closed:false,updated_at:'2026-10-18',metadata:{assignees:['任务负责人'],creator:'需求创建人',priority:['linear','jira'].includes(s.provider)?'High':'',labels:['功能需求','待验收'],project:'产品研发',milestone:'十月迭代',created_at:'2026-10-01',due_at:'2026-10-31'}});
  const workspaces=[],links=[],created=[],journals={},layouts={},sessions={},receipts={};let legacyMetadata=false;let commentMode="normal";
  const lookup=key=>[...sources.map(item),...created].find(i=>i.key===key);
  const githubId=()=>links.find(l=>l.source_id===sources[0].id)?.workspace_id;
  const githubDraftKey=()=>`velella:source-draft:${githubId()}:${journals[githubId()].steps.find(s=>s.kind==='launch').descriptor.id}`;
  await page.route('**/_velella',async route=>{
    const req=route.request().postDataJSON();requests.push(req);const s=sources.find(s=>s.id===req.source_id);let response={ok:true};
    try{switch(req.cmd){
      case 'workbench.snapshot':response={ok:true,projects,workspaces,tasks:[],backups:[],source_links:links};break;
      case 'sources.sync':case 'sources.list':response={ok:true,sources,links,repositories:[{project_id:'p',project_name:'开发项目',provider:'github',host:'github.com',scope:'team/repo'}]};break;
      case 'sources.scopes':response={ok:true,choices:[{id:s.scope,name:s.provider==='linear'?'开发团队':'开发项目'},{id:s.provider==='linear'?'team-b':'OPS',name:'第二个范围'}]};break;
      case 'sources.states':response={ok:true,states:[{id:'closed',name:'关闭任务'}]};break;
      case 'sources.comments':{
        const first={id:'c1',body:'**讨论内容**',author:'Fixture',created:'2026-10-19'};
        response=commentMode==='error'?{ok:false,error:'评论读取暂时失败'}:commentMode==='empty'?{ok:true,comments:[],next:null}:{ok:true,comments:req.cursor?[first,{id:'c2',body:'第二条评论',author:'Reviewer',created:'2026-10-20'}]:[first],next:commentMode==='paged'&&!req.cursor?'page-2':null};break;}

      case 'sources.mutate':response={ok:true};break;
      case 'sources.items':response={ok:true,items:req.cursor?[{...item(s),key:s.id+':8',identifier:'#8',title:'第二页任务'}]:[item(s)],next:req.cursor?null:'2'};break;
      case 'sources.detail':case 'sources.cached':{const found=lookup(req.key);response={ok:true,item:legacyMetadata?{...found,metadata:undefined,updated_at:'invalid date'}:found};break;}
      case 'sources.create_options':response={ok:true,options:req.type_id?{types:[],fields:[{fieldId:'customfield_123',name:'验收负责人说明',required:true,schema:{type:'string'}},{fieldId:'customfield_tags',name:'交付类型',required:true,schema:{type:'array',items:'string'},allowedValues:['功能','修复']}]}:{types:[{id:'10001',name:'任务'}],fields:[]}};break;
      case 'sources.create':{const task={...item(s),key:s.id+':99',remote_id:'99',identifier:'#99',title:req.fields.title,description:req.fields.description};created.push(task);response={ok:true,item:task};break;}
      case 'workbench.create':{
        assert.match(req.request_id,/^[0-9a-f-]{36}$/i);
        const task=lookup(req.source_key);assert(task);
        const existing=links.find(l=>l.key===req.source_key);
        let w=workspaces.find(w=>w.id===req.request_id||w.id===existing?.workspace_id);
        if(!w){
          w={id:req.request_id,project_id:req.project_id,name:req.name,parent_workspace_id:req.parent_workspace_id,path:'C:\\fixture\\worktree\\'+req.request_id,kind:'worktree',archived:false,favorite:false,position:0};workspaces.push(w);
          links.push({...task,project_id:req.project_id,workspace_id:w.id});
          const descriptor=req.launch?{id:'mock-server-session:'+w.id,directory:'C:/fixture/logs'}:null;
          journals[w.id]={schema:1,request_id:req.request_id,request:structuredClone(req),revision:1,frozen:{workspace:structuredClone(w),setup:{decision:'skip',script:''}},steps:[
            {kind:'checkout',state:'succeeded'}, {kind:'setup',state:'skipped'},
            {kind:'launch',state:req.launch?'succeeded':'skipped',descriptor}, {kind:'prompt',state:'skipped'},
          ]};
          if(req.launch){
            assert.equal(req.initial_prompt.mode,'draft');assert.equal(req.launch,'codex');assert.deepEqual(req.launch_options,{});
            sessions[w.id]=[{id:descriptor.id,title:'Codex',kind:'terminal',program:'codex',status:{state:'waiting_input',exit:null,input_protocol:1,input_sequence:0}}];
            serverEffects.push({mockOnly:true,kind:'launch',request_id:req.request_id,session_id:descriptor.id});
          }else assert(!Object.hasOwn(req,'initial_prompt'));
        }else if(w.id===req.request_id)assert.deepEqual(req,journals[w.id].request,'replay must preserve frozen source/prompt/options');
        response={ok:true,workspace:w,creation:journals[w.id],existing:!!existing};break;
      }
      case 'workbench.creation.status':{
        assert.deepEqual(req,{cmd:'workbench.creation.status',request_id:req.request_id});
        assert(journals[req.request_id],'restored UUID workspace must have its immutable creation request');
        response={ok:true,creation:journals[req.request_id],retry_ready:false};break;
      }
      case 'workbench.source.attach':{
        assert.equal(links.find(l=>l.key===req.source_key)?.workspace_id,req.workspace_id,'source link must retain server-created workspace identity');break;
      }
      case 'workbench.layout':response={ok:true,layout:layouts[req.workspace_id]||{revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]}};break;
      case 'workbench.layout.save':layouts[req.workspace_id]=req.layout;response={ok:true,layout:req.layout};break;
      case 'agent.check':response={ok:true,available:true,authenticated:true,version:'fixture'};break;
      case 'sandbox.capabilities':response={ok:true,sandbox:{platform:'win32',backend:'mock',available:false,reason:'MOCK: native-only source tests'}};break;
      case 'worktree.list':response={ok:true,branches:['main'],default_base:{resolved:{reference:'main',oid:'a'.repeat(40),inferred:true}}};break;
      case 'workbench.terminal.open':throw Error('normal source creation MUST NOT call terminal.open; launch belongs to mock server journal');
      case 'workbench.sessions':response={ok:true,sessions:sessions[req.workspace_id]||[]};break;
      case 'workbench.terminal.output':response={ok:true,data:'',from:0,end:0,total:0,geometry:[],geometry_missing:false};break;
      case 'workbench.feedback.send':
        assert.equal(req.draft,true,'client may paste a draft, never submit a creation-owned prompt');
        assert.equal(req.session_id,journals[req.workspace_id].steps.find(s=>s.kind==='launch').descriptor.id);
        assert.equal(req.text,journals[req.workspace_id].request.initial_prompt.text);
        assert.equal(req.source,journals[req.workspace_id].request.source_key);
        if(req.initial)assert.equal(req.expected_input_sequence,0);
        receipts[req.request_id]={state:'drafted'};response={ok:true,receipt:receipts[req.request_id]};break;
      case 'workbench.feedback.status':response={ok:true,receipt:receipts[req.request_id]||null};break;
      case 'workbench.git':response={ok:true,status:'',branch:'codex/task'};break;
      case 'workbench.files':response={ok:true,files:[]};break;
      case 'workbench.terminal.resize':break;
      case 'workbench.terminal.activity':response={ok:true,enabled:true,events:[],next:0,total:0};break;
      default:throw Error('unallowlisted RPC BLOCKED: '+req.cmd);
    }}catch(error){errors.push(String(error));response={ok:false,error:'MOCK BLOCKED: '+String(error)};}
    await route.fulfill({json:response});
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/`);
  await page.getByRole('button',{name:'任务来源',exact:true}).click();
  if(process.env.SOURCE_DIAGNOSTIC){await page.waitForTimeout(1000);console.log(JSON.stringify({requests,body:await page.locator('body').innerText(),errors}));await page.screenshot({path:resolve(evidence,'diagnostic.png')});await browser.close();await server.close();process.exit(0);}
  for(const provider of Object.keys(names)){
    await page.getByRole('tab',{name:names[provider],exact:true}).click();
    await page.locator('.source-list').getByRole('button',{name:new RegExp(names[provider]+' 中文需求')}).click();
    await page.getByRole('heading',{name:names[provider]+' 中文需求'}).waitFor();
    assert.match(await page.locator('.source-description').innerText(),/验收/);
    await page.locator('.source-description h4').getByText('需求说明',{exact:true}).waitFor();
    assert.equal(await page.locator('.source-description input[type=checkbox]').count(),2);
    assert.equal(await page.locator('.source-description input[type=checkbox]').first().isChecked(),true);
    assert.match(await page.locator('.source-description pre').innerText(),/const ready/);
    assert.equal(await page.locator('.source-description img,.source-description script,.source-description a[href^="javascript:"]').count(),0);
    assert.equal(await page.locator('.source-description a').getAttribute('href'),'https://example.test/spec');
    await page.getByRole('region',{name:'任务评论'}).getByText('讨论内容',{exact:true}).waitFor();
    if(provider==='github')assert.equal(requests.filter(r=>r.cmd==='sources.mutate').length,0,'reading a task must not publish anything');
    const info=page.locator('.source-task-info');await info.getByText('任务负责人',{exact:true}).waitFor();await info.getByText('需求创建人',{exact:true}).waitFor();await info.getByText('功能需求',{exact:true}).waitFor();await info.getByText('产品研发',{exact:true}).waitFor();
    assert.match(await info.innerText(),/2026-10-18/);assert.match(await page.locator('.source-row.selected').innerText(),/任务负责人/);
    if(provider==='linear'||provider==='jira')assert.match(await info.innerText(),/High/);
    checks.push(provider+': metadata and list summary are visible');
    await page.getByRole('button',{name:'开始开发',exact:true}).click();
    const composer=page.getByRole('dialog',{name:'新建工作树'});await composer.waitFor();
    assert.match(await composer.getByLabel('工作区名称',{exact:true}).inputValue(),new RegExp(names[provider]));
    await composer.getByLabel('创建后',{exact:true}).selectOption(provider==='github'?'codex':'none');
    if(provider==='github')assert.match(await composer.getByLabel('任务指令',{exact:true}).inputValue(),/Complete https:/);
    if(provider==='jira')await composer.getByLabel('开发项目',{exact:true}).selectOption('q');
    await composer.getByRole('button',{name:'创建并打开',exact:true}).click();
    await page.locator('.source-workspace-links').getByRole('button').filter({hasText:names[provider]}).waitFor();
    if(provider==='github'){
      await page.waitForFunction(key=>JSON.parse(localStorage.getItem(key)||'null')?.state==='drafted',githubDraftKey());
      const draft=await page.evaluate(key=>JSON.parse(localStorage.getItem(key)),githubDraftKey());
      assert.equal(draft.creation_id,githubId());assert.equal(draft.source,journals[githubId()].request.source_key);assert.equal(draft.auto_submit,false);
      assert.equal(draft.session_id,journals[githubId()].steps.find(s=>s.kind==='launch').descriptor.id);
      assert.equal(requests.filter(r=>r.cmd==='workbench.feedback.send').length,1);assert.equal(requests.filter(r=>r.cmd==='workbench.terminal.open').length,0);
      checks.push('mock server launch descriptor receives one guarded UI draft; creation_id/source link preserved, no automatic submission');
    }else{
      const req=requests.filter(r=>r.cmd==='workbench.create').at(-1);
      assert(!Object.hasOwn(req,'launch'),'none must omit launch');assert(!Object.hasOwn(req,'launch_options'));assert(!Object.hasOwn(req,'initial_prompt'));
    }
    if(provider==='jira')assert.equal(workspaces.at(-1).project_id,'q');
    const count=workspaces.length,createCount=requests.filter(r=>r.cmd==='workbench.create').length,effectCount=serverEffects.length;
    await page.locator('.source-workspace-links').getByRole('button').filter({hasText:names[provider]}).click();
    await page.getByRole('button',{name:'打开开发工作区',exact:true}).click();
    assert.equal(workspaces.length,count,'reopening task must not create another workspace');
    assert.equal(requests.filter(r=>r.cmd==='workbench.create').length,createCount);assert.equal(serverEffects.length,effectCount);
    assert.equal(requests.filter(r=>r.cmd==='workbench.terminal.open').length,0,'source reopening cannot launch again');
    checks.push(provider+': task -> composer -> workspace -> task -> existing workspace');
    await page.getByRole('button',{name:'任务来源',exact:true}).click();
    await page.getByRole('tab',{name:names[provider],exact:true}).click();
    if(provider==='linear'){await page.locator('.source-scope-filter summary').click();await page.getByRole('checkbox',{name:'第二个范围',exact:true}).check();await page.locator('.source-scope-filter summary').click();}
    await page.locator('.source-list').getByRole('button',{name:new RegExp(names[provider]+' 中文需求')}).click();
    const activity=page.getByRole('region',{name:'任务操作与讨论'});
    await activity.getByLabel('来源状态',{exact:true}).selectOption('closed');await activity.getByRole('button',{name:'更新状态',exact:true}).click();await activity.getByText('已保存到来源平台',{exact:true}).waitFor();
    await activity.getByRole('region',{name:'任务评论'}).getByText('讨论内容',{exact:true}).waitFor();await activity.getByLabel('评论',{exact:true}).fill('已完成开发验证');await activity.getByRole('button',{name:'发布评论',exact:true}).click();await activity.getByText('已保存到来源平台',{exact:true}).waitFor();
    checks.push(provider+': source status and comment actions');
    await page.getByRole('button',{name:'新建任务',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:`在 ${names[provider]} 新建任务`});
    await dialog.getByLabel('任务标题',{exact:true}).fill(names[provider]+' 新任务');
    await dialog.getByLabel('需求与验收条件',{exact:true}).fill('中文需求\n验收可检查');
    if(provider==='linear')await dialog.getByLabel('所属团队',{exact:true}).selectOption('team-id');
    if(provider==='jira'){await dialog.getByLabel('事项类型',{exact:true}).selectOption('10001');await dialog.getByLabel('验收负责人说明',{exact:true}).fill('项目负责人验收');await dialog.getByLabel('交付类型',{exact:true}).selectOption(['0','1']);}
    await dialog.getByRole('button',{name:'创建任务',exact:true}).click();
    await page.getByRole('heading',{name:names[provider]+' 新任务',exact:true}).waitFor();
    checks.push(provider+': explicit issue creation');
  }
  assert.equal(requests.filter(r=>r.cmd==='sources.create').length,4);assert.deepEqual(requests.filter(r=>r.cmd==='sources.create').at(-1).fields.extra.customfield_tags,['功能','修复']);
  await page.getByRole('button',{name:'加载更多',exact:true}).click();await page.getByRole('button',{name:/第二页任务/}).waitFor();checks.push('pagination appends another page');
  await page.getByRole('button',{name:'连接设置',exact:true}).click();const settings=page.getByRole('dialog',{name:'编辑 Jira'});await settings.waitFor();assert.equal(await settings.locator('input[type=password]').inputValue(),'');await settings.getByRole('button',{name:'取消',exact:true}).click();checks.push('settings never returns stored token');
  await page.evaluate(key=>{const d=JSON.parse(localStorage.getItem(key));localStorage.setItem(key,JSON.stringify({...d,request_id:crypto.randomUUID(),state:'waiting',expires_at:Date.now()-1}));},githubDraftKey());
  await page.locator('.ide-workspace').filter({hasText:'GitHub 中文需求'}).click();
  await page.getByRole('button',{name:'复制提示词',exact:true}).waitFor();
  assert.equal(await page.getByRole('button',{name:'粘贴提示词草稿',exact:true}).count(),0,'expired initial prompt is copy-only, not a second delivery attempt');
  assert.equal(await page.evaluate(key=>JSON.parse(localStorage.getItem(key)).state,githubDraftKey()),'manual');
  assert.equal(requests.filter(r=>r.cmd==='workbench.feedback.send').length,1);
  await page.getByRole('button',{name:'任务来源',exact:true}).click();await page.locator('.ide-workspace').filter({hasText:'GitHub 中文需求'}).click();
  await page.getByRole('button',{name:'复制提示词',exact:true}).waitFor();assert.equal(requests.filter(r=>r.cmd==='workbench.feedback.send').length,1);
  assert.equal(await page.evaluate(key=>JSON.parse(localStorage.getItem(key)).state,githubDraftKey()),'manual');checks.push('expired initial draft remains copy-only and manual after remount without another delivery');
  await page.getByRole('button',{name:'任务来源',exact:true}).click();sessions[githubId()][0].status.input_sequence=1;
  // Legacy non-initial draft retains manual paste behavior; this is NOT a creation-owned auto-submit.
  await page.evaluate(key=>{const d=JSON.parse(localStorage.getItem(key));localStorage.setItem(key,JSON.stringify({...d,request_id:crypto.randomUUID(),state:'waiting',initial:false,expires_at:Date.now()+120000}));},githubDraftKey());
  await page.locator('.ide-workspace').filter({hasText:'GitHub 中文需求'}).click();await page.getByRole('button',{name:'粘贴提示词草稿',exact:true}).waitFor();assert.equal(requests.filter(r=>r.cmd==='workbench.feedback.send').length,1);
  await page.getByRole('button',{name:'粘贴提示词草稿',exact:true}).click();await page.waitForFunction(key=>JSON.parse(localStorage.getItem(key)).state==='drafted',githubDraftKey());assert.equal(requests.filter(r=>r.cmd==='workbench.feedback.send').length,2);assert.equal(requests.filter(r=>r.cmd==='workbench.feedback.send').at(-1).expected_input_sequence,undefined);checks.push('existing user input blocks auto-paste; explicit legacy manual paste remains available');
  const githubRequest=structuredClone(journals[githubId()].request),beforeRestore=requests.length;
  await page.evaluate(key=>localStorage.removeItem(key),githubDraftKey());
  await page.reload({waitUntil:'domcontentloaded'});
  await page.getByRole('button',{name:'复制提示词',exact:true}).waitFor();
  await page.waitForFunction(key=>JSON.parse(localStorage.getItem(key)||'null')?.state==='manual',githubDraftKey());
  assert.equal(await page.getByRole('button',{name:'粘贴提示词草稿',exact:true}).count(),0,'recovered initial prompt must not expose a repaste action');
  const recovered=await page.evaluate(key=>JSON.parse(localStorage.getItem(key)),githubDraftKey());
  assert.equal(recovered.text,githubRequest.initial_prompt.text);assert.equal(recovered.creation_id,githubId());assert.equal(recovered.source,githubRequest.source_key);assert.equal(recovered.initial,true);
  assert.equal(recovered.session_id,journals[githubId()].steps.find(s=>s.kind==='launch').descriptor.id);
  assert.equal(recovered.state,'manual','recovery must respect existing user input, not repaste immutable prompt');
  assert(requests.slice(beforeRestore).some(r=>r.cmd==='workbench.creation.status'&&r.request_id===githubId()));
  assert(!requests.slice(beforeRestore).some(r=>['workbench.create','workbench.terminal.open','workbench.feedback.send'].includes(r.cmd)));
  assert.deepEqual(journals[githubId()].request,githubRequest);
  await page.locator('.source-workspace-links').getByRole('button').filter({hasText:'GitHub 中文需求'}).waitFor();
  checks.push('reload without local draft recovers immutable source prompt and server session by UUID, retains source link and never repastes over user input');
  await page.getByRole('button',{name:'任务来源',exact:true}).click();await page.getByRole('tab',{name:'Jira',exact:true}).click();await page.locator('.source-list').getByRole('button',{name:/Jira 中文需求/}).click();
  const discussion=page.getByRole('region',{name:'任务评论'});
  commentMode='empty';await discussion.getByRole('button',{name:'刷新评论',exact:true}).click();await discussion.getByText('暂无评论，可以补充问题、进展或验收结果。',{exact:true}).waitFor();
  commentMode='error';await discussion.getByRole('button',{name:'刷新评论',exact:true}).click();await discussion.getByText('评论读取暂时失败',{exact:true}).waitFor();assert.equal(await discussion.getByText('暂无评论，可以补充问题、进展或验收结果。',{exact:true}).count(),0);
  commentMode='paged';await discussion.getByRole('button',{name:'重试加载评论',exact:true}).click();await discussion.getByText('讨论内容',{exact:true}).waitFor();await discussion.getByRole('button',{name:'更多评论',exact:true}).click();await discussion.getByText('第二条评论',{exact:true}).waitFor();assert.equal(await discussion.locator('article').count(),2);
  checks.push('comments load automatically; empty, error, retry and deduplicated pagination work; Markdown renders without executable HTML');
  await discussion.scrollIntoViewIfNeeded();await page.screenshot({path:resolve(evidence,'task-comments-dark.png'),fullPage:true});
  await page.locator('.source-detail').evaluate(e=>{e.scrollTop=0});
  await page.screenshot({path:resolve(evidence,'task-page-dark.png'),fullPage:true});
  await page.getByRole('button',{name:'切换浅色主题',exact:true}).click();await page.setViewportSize({width:1100,height:700});await page.screenshot({path:resolve(evidence,'task-page-light.png'),fullPage:true});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await page.setViewportSize({width:900,height:700});
  assert.equal(await page.locator('.source-task-info').evaluate(e=>e.scrollWidth>e.clientWidth),false);
  await page.screenshot({path:resolve(evidence,'task-page-narrow.png'),fullPage:true});
  legacyMetadata=true;await page.locator('.source-list').getByRole('button',{name:/Jira 中文需求/}).click();
  await page.waitForFunction(()=>document.querySelector('.source-task-info')?.textContent.includes('未提供')&&!document.querySelector('.source-task-info')?.textContent.includes('任务负责人'));
  assert.doesNotMatch(await page.locator('.source-task-info').innerText(),/Invalid Date|NaN|invalid date/);
  assert.match(await page.locator('.source-description').innerText(),/验收/);
  checks.push('legacy metadata and invalid dates display honest fallbacks; narrow detail does not overflow');
  assert.deepEqual(errors,[]);
  assert.equal(requests.filter(r=>r.cmd==='workbench.terminal.open').length,0);
  assert.equal(serverEffects.length,1,'only one mock server launch for the GitHub native draft');
  writeFileSync(resolve(evidence,'result.json'),JSON.stringify({status:'passed',runtime,checks,sourceMocks:true,errors,requests,serverEffects},null,2));console.log(JSON.stringify({status:'passed',checks,evidence}));
}catch(error){if(page)await page.screenshot({path:resolve(evidence,'failure.png')}).catch(()=>{});writeFileSync(resolve(evidence,'result.json'),JSON.stringify({status:'failed',runtime,checks,errors,requests,serverEffects,error:String(error)},null,2));throw error}
finally{await browser?.close();await server.close()}
