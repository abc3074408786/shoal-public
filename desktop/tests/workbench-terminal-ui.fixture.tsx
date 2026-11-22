// Development-only full WorkspaceApp fixture. Never forwards RPC or launches a CLI.
// Open /tests/workbench-terminal-ui.html?scenario=ready (see the scenario links below).
import React from 'react';
import {createRoot} from 'react-dom/client';
import type {Project,Task} from '../src/api';
import type {Layout,Session,Workspace} from '../src/workbench-types';
import {agentCatalog} from '../src/agent-preferences';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/motion.css';

type RequestBody = Record<string,unknown>;
type Result = {ok:boolean;error?:string;[key:string]:unknown};
const scenarios=['ready','direct','unavailable','reprobe-failure','launch-failure',
  'existing-worktree','existing-launch-failure','layout-failure','primary-folder',
  'task-owned','task-active','existing-unavailable','existing-folder',
  'existing-launch-recorded-failure','layout-persisted-failure'] as const;
const scenario=new URLSearchParams(location.search).get('scenario')||'ready';
const shellPreview=document.documentElement.hasAttribute('data-project-shell-preview');
const layoutPreview=shellPreview&&['tabs','columns'].includes(new URLSearchParams(location.search).get('layout')||'');
const initialTheme=document.documentElement.dataset.previewTheme==='light'?'light':'dark';
const taskOwned=['task-owned','task-active'].includes(scenario);
const taskPhase=new URLSearchParams(location.search).get('phase')||
  (scenario==='task-active'?'running':'cancelled');
const existingWorktree=['existing-worktree','existing-launch-failure','layout-failure','task-owned','task-active',
  'existing-unavailable','existing-launch-recorded-failure','layout-persisted-failure'].includes(scenario);
const projects:Project[]=[
  {id:'fixture-git',name:shellPreview?'鱼群 Velella':'浏览器测试 Git 项目',path:'/fixture-only/git-project',setup:'',direct:false},
  {id:'fixture-direct',name:layoutPreview?'哼哼 OA':'浏览器测试普通文件夹',path:'/fixture-only/plain-folder',setup:'',direct:true},
];
const workspaces:Workspace[]=projects.map((p,i)=>({
  id:`main:${p.id}`,project_id:p.id,name:'默认工作区',path:p.path,
  kind:p.direct?'folder':'primary',archived:false,favorite:false,position:i,
}));
const existingWorkspace:Workspace|undefined=existingWorktree?{
  id:taskOwned?'task:fixture-task':'fixture-existing-worktree',
  project_id:'fixture-git',name:taskOwned?'任务保留的工作树':'已存在的 Git 工作树',
  path:'/fixture-only/worktrees/existing',kind:'worktree',archived:false,favorite:false,position:2,
}:undefined;
if(existingWorkspace)workspaces.push(existingWorkspace);
// A stopped task still owns its historical worktree for restricted-launch
// checks, while native launch remains an explicit user choice.
const tasks:Task[]=taskOwned?[{
  id:'fixture-task',project_id:'fixture-git',phase:taskPhase,reason:`夹具：${taskPhase} 任务保留此工作树。`,
  verified:false,created_at:'2026-10-20T00:00:00Z',worktree:existingWorkspace!.path,
  branch:'fixture-task',native_id:null,agent:null,command:null,shell:null,pr_url:null,checks:[],
  status:null,spec:{name:'保留工作树的测试任务',agent:'codex',instruction:'仅浏览器夹具，不执行。',
    base:'main',policy:{budget:{wall_clock:'10m'}}},
}]:[];
const initialWorkspaceIds=workspaces.map(w=>w.id);
const initialWorkspaceId=existingWorkspace?.id||
  (['direct','existing-folder'].includes(scenario)?'main:fixture-direct':'main:fixture-git');
const layouts=new Map<string,Layout>();
const sessions=new Map<string,Session[]>();
if(layoutPreview)for(const w of workspaces){
  const previewSessions:Session[]=['codex','powershell'].map(program=>({id:`preview-${w.id}-${program}`,title:program==='codex'?'Codex':'PowerShell',kind:'terminal',program,native_chat:false,execution_mode:'native',task_id:null,native_id:null,status:{exit:null,state:'waiting_input',input_protocol:1},recoverable:false}));
  sessions.set(w.id,previewSessions);
  layouts.set(w.id,{revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:`tab-${previewSessions[0].id}`,tabs:previewSessions.map(s=>({id:`tab-${s.id}`,kind:'terminal',target:s.id,title:s.title}))}],closed:[]});
}
const creates=new Map<string,{workspace:Workspace;signature:string}>();
const opens=new Map<string,{session:Session;signature:string}>();
const requests:{sequence:number;request:RequestBody;ok:boolean;error?:string}[]=[];
const counts:Record<string,number>={};
let probes=0,launchFailures=0,layoutFailures=0,blockedFetches=0,fatalError:string|null=null;
let sessionAtLayoutFailure:string|null=null,sessionAtLaunchFailure:string|null=null;
let layoutReadsAfterFailure=0,layoutPersistedBeforeError=false;

function memoryStorage():Storage {
  const data=new Map<string,string>();
  const api={
    get length(){return data.size},
    clear(){data.clear()},
    getItem(key:string){return data.get(String(key))??null},
    key(index:number){return [...data.keys()][index]??null},
    removeItem(key:string){data.delete(String(key))},
    setItem(key:string,value:string){data.set(String(key),String(value))},
  };
  return new Proxy(api,{
    ownKeys(){return [...data.keys()]},
    getOwnPropertyDescriptor(_target,key){
      return typeof key==='string'&&data.has(key)
        ?{configurable:true,enumerable:true,writable:true,value:data.get(key)}
        :undefined;
    },
  });
}

function emptyLayout():Layout {
  return {revision:0,axis:'horizontal',ratio:0.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]};
}

function workspaceFor(request:RequestBody):Workspace {
  const workspace=workspaces.find(w=>w.id===request.workspace_id);
  if(!workspace)throw new Error('夹具拒绝未知工作区。');
  return workspace;
}

function requestKey(request:RequestBody):string {
  if(typeof request.request_id!=='string'||!request.request_id.trim())
    throw new Error('夹具要求非空 request_id，以检查创建和重试。');
  return request.request_id;
}

function record(request:RequestBody,result:Result) {
  const cmd=typeof request.cmd==='string'?request.cmd:'(invalid)';
  counts[cmd]=(counts[cmd]||0)+1;
  requests.push({sequence:requests.length+1,request:structuredClone(request),ok:result.ok,...(result.error?{error:result.error}:{})});
  publish();
}

function publish() {
  const openRequests=requests.filter(r=>r.request.cmd==='workbench.terminal.open');
  const createRequests=requests.filter(r=>r.request.cmd==='workbench.create');
  const newWorkspaces=[...creates.values()].map(entry=>entry.workspace);
  const allSessions=[...sessions.values()].flat();
  const retryComplete=['launch-failure','existing-launch-failure'].includes(scenario)&&launchFailures===1&&allSessions.length===1&&openRequests.length===2;
  const layoutRequests=requests.filter(r=>r.request.cmd==='workbench.layout.save');
  const savedTerminalIds=[...layouts.values()].flatMap(layout=>layout.panes.flatMap(pane=>
    pane.tabs.filter(tab=>tab.kind==='terminal').map(tab=>tab.target)));
  const layoutFailureSequence=layoutRequests.find(r=>!r.ok)?.sequence??Infinity;
  const viewedAfterLayoutFailure=requests.some(r=>r.sequence>layoutFailureSequence&&
    r.request.cmd==='workbench.terminal.output'&&r.request.session_id===sessionAtLayoutFailure);
  const layoutRetryComplete=layoutFailures===1&&sessionAtLayoutFailure!==null&&savedTerminalIds.includes(sessionAtLayoutFailure)&&
    (scenario!=='layout-persisted-failure'||layoutReadsAfterFailure>0&&(
      viewedAfterLayoutFailure||layoutRequests.some(r=>r.ok&&r.sequence>layoutFailureSequence)));
  const recordedLaunchRecovered=sessionAtLaunchFailure!==null&&savedTerminalIds.includes(sessionAtLaunchFailure);
  const taskNativeBlocked=tasks.some(task=>!['queued','cancelled','delivered'].includes(task.phase));
  const summary={
    scenario,scope:'UI fixture only; no runtime, model, authentication, or filesystem execution',
    realRpcForwarded:0,persistentStorageAccess:false,fatalError,blockedFetches,
    initialWorkspaceId,initialWorkspaceIds,taskOwned,taskPhase:taskOwned?taskPhase:null,taskNativeBlocked,
    counters:counts,probeCalls:probes,createCalls:createRequests.length,createdWorkspaceCount:newWorkspaces.length,
    openCalls:openRequests.length,uniqueOpenRequestIds:new Set(openRequests.map(r=>r.request.request_id)).size,
    simulatedSessionCount:allSessions.length,launchFailures,layoutFailures,
    layoutSaveCalls:layoutRequests.length,layoutReadsAfterFailure,layoutPersistedBeforeError,
    layoutRevisions:Object.fromEntries([...layouts].map(([id,layout])=>[id,layout.revision])),
    sessionAtLayoutFailure,sessionAtLaunchFailure,savedTerminalIds,
    lastOpenRequest:openRequests.at(-1)?.request??null,
    assertions:{
      everySuccessfulSessionUsesTerminal:allSessions.filter(s=>s.status!==null).every(s=>s.native_chat===false&&s.kind==='terminal'&&s.status?.input_protocol===1),
      everySessionRecordIsTerminal:allSessions.every(s=>s.native_chat===false&&s.kind==='terminal'),
      everySavedTabIsTerminal:[...layouts.values()].every(layout=>layout.panes.every(pane=>pane.tabs.every(tab=>tab.kind==='terminal'))),
      noChatRpc:requests.every(r=>!String(r.request.cmd).startsWith('workbench.chat.')),
      everyRestrictedSessionHasWorktree:[...sessions.entries()].every(([id,list])=>
        list.every(s=>s.execution_mode!=='restricted'||workspaces.some(w=>w.id===id&&w.kind==='worktree'))),
      retryCompleted:retryComplete,
      retryDidNotRecreate:retryComplete?(scenario==='launch-failure'
        ?createRequests.length===1&&newWorkspaces.length===1:createRequests.length===0&&newWorkspaces.length===0):null,
      retryUsedSameRequestId:retryComplete?
        openRequests.every(r=>r.request.request_id===(scenario==='launch-failure'
          ?createRequests[0]?.request.request_id:openRequests[0]?.request.request_id)):null,
      retryKeptRestrictedMode:retryComplete?openRequests.every(r=>
        (r.request.sandbox as {mode?:string}|undefined)?.mode==='restricted'):null,
      existingLaunchDidNotCreateWorkspace:existingWorktree||['primary-folder','existing-folder'].includes(scenario)
        ?createRequests.length===0&&workspaces.length===initialWorkspaceIds.length:null,
      layoutFailureRetainedSession:layoutFailures>0
        ?allSessions.some(s=>s.id===sessionAtLayoutFailure):null,
      layoutRetryCompleted:layoutRetryComplete,
      layoutRetryDidNotReopen:layoutRetryComplete?openRequests.length===1&&allSessions.length===1:null,
      layoutRetryUsesOriginalSession:layoutRetryComplete
        ?savedTerminalIds.length===1&&savedTerminalIds[0]===sessionAtLayoutFailure:null,
      persistedLayoutWasReread:scenario==='layout-persisted-failure'&&layoutFailures>0
        ?layoutReadsAfterFailure>0:null,
      persistedLayoutHasSingleOriginalTab:scenario==='layout-persisted-failure'&&layoutFailures>0
        ?savedTerminalIds.length===1&&savedTerminalIds[0]===sessionAtLayoutFailure:null,
      recordedLaunchFailureRetainedSession:sessionAtLaunchFailure!==null
        ?allSessions.length===1&&allSessions[0].id===sessionAtLaunchFailure&&allSessions[0].status===null:null,
      recordedLaunchFailureMatchesRequestId:sessionAtLaunchFailure!==null
        ?sessionAtLaunchFailure===openRequests[0]?.request.request_id:null,
      recordedLaunchRecoveryCompleted:recordedLaunchRecovered,
      recordedLaunchRecoveryDidNotReopen:recordedLaunchRecovered
        ?openRequests.length===1&&allSessions.length===1:null,
      recordedLaunchRecoveryUsesOriginalSession:recordedLaunchRecovered
        ?savedTerminalIds.length===1&&savedTerminalIds[0]===sessionAtLaunchFailure:null,
      activeTaskNeverOpened:taskNativeBlocked?openRequests.length===0:null,
      deniedRestrictedNeverOpened:['primary-folder','existing-folder','task-owned','task-active','existing-unavailable'].includes(scenario)
        ?openRequests.every(r=>(r.request.sandbox as {mode?:string}|undefined)?.mode!=='restricted'):null,
      nativeRequestOmitsSandbox:openRequests.some(r=>r.ok&&!(r.request.sandbox as {mode?:string}|undefined)?.mode)
        ?openRequests.filter(r=>r.ok).every(r=>!Object.hasOwn(r.request,'sandbox')):null,
    },
    createdWorkspaces:newWorkspaces,simulatedSessions:allSessions,requests,
  };
  document.getElementById('qa-summary')!.textContent=JSON.stringify(summary,null,2);
  document.getElementById('qa-status')!.textContent=
    `scenario=${scenario} · probe=${probes} · create=${createRequests.length} · new worktrees=${newWorkspaces.length} · open=${openRequests.length} · layout=${layoutRequests.length}/${layoutFailures}失败 · 模拟会话=${allSessions.length} · real RPC=0`+
    (retryComplete?` · 重试未重复创建=${summary.assertions.retryDidNotRecreate}`:'')+
    (fatalError?` · 错误：${fatalError}`:'');
}

function respond(request:RequestBody):Result {
  switch(request.cmd){
    case 'protocol':return {ok:true,capabilities:['terminal-permissions-v1']};
    case 'sources.list':return {ok:true,sources:[]};
    case 'workbench.snapshot':
      return {ok:true,projects,workspaces,tasks,source_links:[]};
    case 'sandbox.capabilities': {
      probes++;
      if(scenario==='reprobe-failure'&&probes>1)
        return {ok:false,error:'浏览器验收夹具：重新检查连接失败；已选受限模式应保留并禁止提交。'};
      const available=!['unavailable','existing-unavailable'].includes(scenario);
      return {ok:true,sandbox:{
        platform:'linux',backend:'bubblewrap',available:false,
        reason:'浏览器验收夹具：bubblewrap 不可用，用于验证受限模式独立检测。',
        restricted:{
          platform:'linux',backend:'landlock-seccomp',available,
          reason:available?null:'浏览器验收夹具：Landlock/seccomp 探测不可用。',
          filesystem_allowlist:available,process_namespace:false,
          network_modes:available?['denied']:[],experimental:true,
        },
      }};
    }
    case 'workbench.git': {
      const workspace=workspaceFor(request);
      return {ok:true,git:workspace.kind!=='folder',branch:workspace.kind==='folder'?'':'fixture-main',status:'',diff:'',staged:''};
    }
    case 'workbench.files':
      workspaceFor(request);
      return {ok:true,files:[]};
    case 'task.files':
      if(!tasks.some(task=>task.id===request.task_id))throw new Error('夹具拒绝未知任务。');
      return {ok:true,files:[]};
    case 'workbench.sessions':
      workspaceFor(request);
      return {ok:true,sessions:sessions.get(String(request.workspace_id))||[]};
    case 'workbench.layout': {
      const workspace=workspaceFor(request);
      if(layoutFailures>0)layoutReadsAfterFailure++;
      return {ok:true,layout:layouts.get(workspace.id)||emptyLayout()};
    }
    case 'workbench.layout.save': {
      const workspace=workspaceFor(request);
      const layout=structuredClone(request.layout) as Layout;
      if(!layout||!Array.isArray(layout.panes)||!layout.panes.length)
        throw new Error('夹具拒绝非法布局。');
      if(['layout-failure','layout-persisted-failure'].includes(scenario)&&layoutFailures===0){
        const created=(sessions.get(workspace.id)||[]).find(session=>
          layout.panes.some(pane=>pane.tabs.some(tab=>tab.target===session.id)));
        if(created){
          layoutFailures++;
          sessionAtLayoutFailure=created.id;
          if(scenario==='layout-persisted-failure'){
            layout.revision=(layouts.get(workspace.id)?.revision||0)+1;
            layouts.set(workspace.id,layout);
            layoutPersistedBeforeError=true;
            return {ok:false,error:'浏览器验收夹具：标签布局已经保存，但首次响应失败；重试应重读布局并复用原标签。'};
          }
          return {ok:false,error:'浏览器验收夹具：终端已创建，首次保存标签布局失败；重试只应打开原会话。'};
        }
      }
      layout.revision=(layouts.get(workspace.id)?.revision||0)+1;
      layouts.set(workspace.id,layout);
      return {ok:true,layout};
    }
    case 'workbench.create': {
      const project=projects.find(p=>p.id===request.project_id);
      if(!project||project.direct)throw new Error('夹具只在 Git 项目中创建工作树。');
      const key=requestKey(request);
      const signature=JSON.stringify([request.project_id,request.name,request.base]);
      const existing=creates.get(key);
      if(existing){
        if(existing.signature!==signature)throw new Error('夹具拒绝相同 request_id 改变创建参数。');
        return {ok:true,workspace:existing.workspace,existing:true};
      }
      if(typeof request.name!=='string'||!request.name.trim())
        throw new Error('夹具拒绝空工作树名称。');
      const workspace:Workspace={
        id:`fixture-worktree-${creates.size+1}`,project_id:project.id,name:request.name,
        path:`/fixture-only/worktrees/work-${creates.size+1}`,kind:'worktree',
        archived:false,favorite:false,position:workspaces.length,
      };
      workspaces.push(workspace);
      creates.set(key,{workspace,signature});
      return {ok:true,workspace,existing:false};
    }
    case 'workbench.terminal.open': {
      const workspace=workspaceFor(request);
      const selectedAgent=agentCatalog.find(agent=>agent.id===request.launch);
      if(!selectedAgent)throw new Error('夹具仅模拟目录中的已知 Agent 终端。');
      const sandbox=request.sandbox as Record<string,unknown>|undefined;
      if(sandbox!==undefined&&(!sandbox||typeof sandbox!=='object'||Array.isArray(sandbox)||
        Object.keys(sandbox).length!==1||!['native','restricted'].includes(String(sandbox.mode))))
        throw new Error('夹具拒绝非窄格式 sandbox；仅允许 {mode:native|restricted}。');
      const mode=(sandbox?.mode||'native') as 'native'|'restricted';
      if(tasks.some(task=>task.worktree===workspace.path&&!['queued','cancelled','delivered'].includes(task.phase)))
        throw new Error('夹具拒绝在未结束任务工作树中创建额外终端。');
      if(mode==='restricted'&&(workspace.kind!=='worktree'||tasks.some(task=>task.worktree===workspace.path)||
        ['unavailable','existing-unavailable'].includes(scenario)||
        (scenario==='reprobe-failure'&&probes>1)))
        throw new Error('夹具拒绝工作区或能力不满足条件的受限模式。');
      const key=requestKey(request),signature=JSON.stringify([workspace.id,request.launch,mode,request.permission_mode||'inherit']);
      const existing=opens.get(key);
      if(existing){
        if(existing.signature!==signature)throw new Error('夹具拒绝相同 request_id 改变执行模式。');
        return {ok:true,session_id:existing.session.id,existing:true};
      }
      if(['launch-failure','existing-launch-failure'].includes(scenario)&&launchFailures===0){
        launchFailures++;
        return {ok:false,error:'浏览器验收夹具：Codex 终端首次启动失败；请用页面重试按钮继续，执行模式和 request_id 应保持。'};
      }
      const session:Session={
        id:key,title:mode==='restricted'?'Codex · 工具受限':selectedAgent.name,kind:'terminal',
        native_chat:false,execution_mode:mode,program:selectedAgent.program,task_id:null,native_id:null,
        status:scenario==='existing-launch-recorded-failure'?null:{exit:null,state:'waiting_input',input_protocol:1},recoverable:false,
      };
      opens.set(key,{session,signature});
      sessions.set(workspace.id,[...(sessions.get(workspace.id)||[]),session]);
      if(scenario==='existing-launch-recorded-failure'&&launchFailures===0){
        launchFailures++;
        sessionAtLaunchFailure=session.id;
        return {ok:false,error:'浏览器验收夹具：会话记录已保存，启动响应失败；查看已有会话时不能再次启动。'};
      }
      return {ok:true,session_id:session.id,existing:false};
    }
    case 'agent.check':
      return {ok:true,available:true,authenticated:false,version:'fixture-only; no CLI or login check performed'};
    case 'workbench.terminal.activity': {
      if(!layoutPreview)return {ok:false,error:'浏览器夹具未模拟工作记录。'};
      const workspace=workspaceFor(request);
      if(!sessions.get(workspace.id)?.some(s=>s.id===request.session_id))throw new Error('夹具拒绝未知终端。');
      return {ok:true,enabled:false,events:[],next:0,total:0};
    }
    case 'workbench.terminal.output':
    case 'workbench.terminal.input':
    case 'workbench.terminal.resize':
    case 'workbench.terminal.control': {
      const workspace=workspaceFor(request);
      const session=sessions.get(workspace.id)?.find(s=>s.id===request.session_id);
      if(!session)throw new Error('夹具拒绝未知终端。');
      if(request.cmd==='workbench.terminal.output'){
        const text=layoutPreview?`${projects.find(p=>p.id===workspace.project_id)?.name} / ${session.title}\r\n\r\nUI PREVIEW\r\nNo command is running.\r\n\r\n> `:'Codex TUI / UI FIXTURE ONLY\r\nNo CLI, model, login, command or real RPC is running.\r\n> ';
        const bytes=new TextEncoder().encode(text);
        const from=Math.min(bytes.length,Math.max(0,Number(request.since)||0));
        return {ok:true,from,end:bytes.length,total:bytes.length,data:btoa(String.fromCharCode(...bytes.slice(from))),
          geometry:[{offset:0,rows:24,cols:100,sequence:1}],geometry_error:null};
      }
      // Inputs and resize events are recorded only; nothing is executed or forwarded.
      return {ok:true};
    }
    default:
      return {ok:false,error:`浏览器验收夹具拒绝未模拟的 RPC：${String(request.cmd)}。未连接真实后台。`};
  }
}

async function start() {
  if('__TAURI_INTERNALS__' in window)
    throw new Error('此夹具只允许在普通浏览器中运行，拒绝 Tauri 原生 RPC。');
  if(!scenarios.includes(scenario as typeof scenarios[number]))
    throw new Error(`未知 scenario：${scenario}`);
  // Protect the real application's preferences when this page shares its Vite origin.
  Object.defineProperty(window,'localStorage',{configurable:true,value:memoryStorage()});
  Object.defineProperty(window,'sessionStorage',{configurable:true,value:memoryStorage()});
  localStorage.setItem('velella:workspace',initialWorkspaceId);
  localStorage.setItem('velella:ui:theme',JSON.stringify(initialTheme));
  // Install before importing application modules. No reference to the original fetch is kept.
  window.fetch=async(input,init)=>{
    const target=typeof input==='string'?input:input instanceof URL?input.href:input.url;
    if(target!=='/_velella'||init?.method!=='POST'||typeof init.body!=='string'){
      blockedFetches++;
      const result={ok:false,error:'夹具已阻止非预期 fetch；未发送网络请求。'};
      record({cmd:'(blocked fetch)',target},result);
      return new Response(JSON.stringify(result),{status:403,headers:{'Content-Type':'application/json'}});
    }
    let request:RequestBody;
    try{
      if(init.body.length>131072)throw new Error('请求超出夹具 128 KiB 上限。');
      const parsed:unknown=JSON.parse(init.body);
      if(!parsed||typeof parsed!=='object'||Array.isArray(parsed))throw new Error('请求必须为 JSON 对象。');
      request=parsed as RequestBody;
    }catch(error){
      const result={ok:false,error:String(error)};
      record({cmd:'(invalid body)'},result);
      return new Response(JSON.stringify(result),{status:400,headers:{'Content-Type':'application/json'}});
    }
    let result:Result;
    try{result=respond(request)}catch(error){result={ok:false,error:String(error)}}
    record(request,result);
    return new Response(JSON.stringify(result),{headers:{'Content-Type':'application/json'}});
  };
  document.documentElement.dataset.theme=initialTheme;
  publish();
  const [{default:WorkspaceApp},{AppearanceProvider},{TooltipProvider},{ConfirmationProvider},layoutModule]=await Promise.all([
    import('../src/WorkspaceApp'),import('../src/Appearance'),import('../src/ui'),import('../src/ConfirmAction'),
    layoutPreview&&new URLSearchParams(location.search).get('layout')==='columns'?import('./project-layout-options'):Promise.resolve(null),
  ]);
  // One mount per scenario makes an explicit "重新检查" the second capability request.
  createRoot(document.getElementById('root')!).render(
    <AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp projectSurface={layoutModule?context=><layoutModule.default {...context}/>:undefined}/></ConfirmationProvider></TooltipProvider></AppearanceProvider>,
  );
}
void start().catch(error=>{
  fatalError=String(error);
  document.getElementById('root')!.textContent=`浏览器验收夹具未启动：${fatalError}`;
  publish();
});
