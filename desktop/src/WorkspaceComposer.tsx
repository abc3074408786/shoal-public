import {useRef,useState,useEffect} from 'react';
import {Bot,ChevronDown,Folder,FolderPlus,Monitor,Server,Terminal,X} from 'lucide-react';
import {Button,Input,Textarea,NativeDialog} from './ui';
import {rpc as apiRpc,Project,type LaunchOptions} from './api';
import {currentTarget,sameTarget} from './execution-target';
import {agentCatalog,agentPermission,type AgentId,type AgentPermissionMode} from './agent-preferences';
import {readAgentPreferences,useAgentPreferences} from './useAgentPreferences';
import AgentPermissionField from './AgentPermissionField';
import {permissionLaunchFields} from './agent-permission-launch';
import type {Workspace} from './workbench-types';
import {taskWorkspaceId} from './workspace-context';
import {SourceItem,taskInstruction} from './task-sources';
import {saveSourceDraft} from './SourceTaskDraft';
import {readCapabilities,type SandboxProbe} from './sandbox-policy';
import {terminalExecutionBlockReason,terminalExecutionRequest,restrictedTerminalDescription,type TerminalExecutionMode} from './terminal-execution';
import CreationSourcePicker from './CreationSourcePicker';
import './creation-dialog.css';
import {tabSessions} from './split-layout';

type SetupChoice='run'|'skip'|'inherit';
type LaunchCapabilities={model:boolean;effort:boolean;evidence:'target_cli_help'};
type CreationStep={kind:string;state:string;error?:string|null;descriptor?:{id:string;directory?:string}|null};
type Creation={request_id:string;frozen:{setup?:{decision:'run'|'skip';script:string};workspace?:Workspace};steps:CreationStep[]};
type FrozenRequest={request:Record<string,unknown>;requestId:string;projectId:string;title:string;direct:boolean;managed:boolean;action:string;agent:string;permission:AgentPermissionMode;source?:SourceItem;goal:string;autoSubmit:boolean;launch:LaunchOptions;sandbox?:{mode:'restricted'};setup:{decision:'run'|'skip';script:string}};
const stepNames:Record<string,string>={checkout:'创建工作树',setup:'准备脚本',launch:'启动',prompt:'提示词'};
const stepStates:Record<string,string>={pending:'等待中',running:'运行中',succeeded:'已成功',skipped:'已跳过',failed:'失败',uncertain:'结果不确定'};
const prepared=(creation:Creation)=>['checkout','setup','launch'].every(kind=>creation.steps.some(step=>step.kind===kind&&['succeeded','skipped'].includes(step.state)));
function launchCapabilities(result:any):LaunchCapabilities|undefined{
  const value=result.launch_capabilities;
  return value?.evidence==='target_cli_help'&&typeof value.model==='boolean'&&typeof value.effort==='boolean'?value:undefined;
}
function creationRequest(source?:SourceItem){
  const key=source?`velella:source-create:${source.key}`:'';
  let id=key?localStorage.getItem(key):null;
  if(!id||!(/^[0-9a-f-]{36}$/i.test(id)))id=crypto.randomUUID();
  if(key)localStorage.setItem(key,id);return id;
}

export default function WorkspaceComposer({project:initialProject,projects,workspaces=[],parentWorkspaceId,sourceItem:initialSource,remote=false,hostName='',onClose,onCreated,onNewProject,onSources}:{project:Project;projects?:Project[];workspaces?:Workspace[];parentWorkspaceId?:string|null;sourceItem?:SourceItem;remote?:boolean;hostName?:string;onClose:()=>void;onCreated:(id:string,projectId?:string)=>Promise<void>;onNewProject?:()=>void;onSources?:()=>void}){
  const hostLabel=remote?'远端执行主机 · '+hostName:'本机';
  const [target]=useState(currentTarget);
  const {preferences}=useAgentPreferences(target);
  const [permissionChoice,setPermissionChoice]=useState<'default'|AgentPermissionMode>('default');
  async function rpc(request:Record<string,unknown>){if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，请从原主机继续此请求。');const result=await apiRpc(request,target);if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，请从原主机查看请求结果。');return result;}
  const [autoRecover,setAutoRecover]=useState(false),[autoSubmit,setAutoSubmit]=useState(false);
  const [sourceItem,setSourceItem]=useState(initialSource);
  const launchKey=useRef('');
  if(!launchKey.current)launchKey.current=creationRequest(initialSource);
  const [projectId,setProjectId]=useState(initialProject.id);const project=projects?.find(p=>p.id===projectId)||initialProject;
  const dialog=useRef<HTMLDialogElement>(null),createdId=useRef<string|null>(null),sessionId=useRef<string|null>(null),submitting=useRef(false);
  const frozen=useRef<FrozenRequest|null>(null),creationRef=useRef<Creation|null>(null),checkedCli=useRef(false);
  const [creation,setCreation]=useState<Creation|null>(null),[retryConfirmed,setRetryConfirmed]=useState(false),[retryReady,setRetryReady]=useState(false);
  const [goal,setGoal]=useState(sourceItem?taskInstruction(sourceItem):''),[name,setName]=useState(sourceItem?`${sourceItem.identifier} ${sourceItem.title}`.slice(0,50):''),[action,setAction]=useState(sourceItem?(localStorage.getItem('velella:source-launch')||'codex'):'none'),[base,setBase]=useState(''),[checks,setChecks]=useState(''),[budget,setBudget]=useState('8h'),[allowed,setAllowed]=useState(''),[ack,setAck]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[health,setHealth]=useState('');
  const [agentMode,setAgentMode]=useState<'native'|'managed'>('native'),[model,setModel]=useState(''),[effort,setEffort]=useState(''),[capabilities,setCapabilities]=useState<LaunchCapabilities>();
  const [terminalExecution,setTerminalExecution]=useState<TerminalExecutionMode>('native'),[terminalProbe,setTerminalProbe]=useState<SandboxProbe>({status:'loading'});
  const [setup,setSetup]=useState<SetupChoice>('inherit'),[reviewed,setReviewed]=useState(false),[parentId,setParentId]=useState(parentWorkspaceId||'');
  const [keepCreating,setKeepCreating]=useState(false),[notice,setNotice]=useState(''),[sourceLoading,setSourceLoading]=useState(false),[cycle,setCycle]=useState(0);
  const terminalProbeGeneration=useRef(0),cliGeneration=useRef(0);
  async function checkTerminalExecution(){const generation=++terminalProbeGeneration.current;setTerminalProbe({status:'loading'});try{const result=await rpc({cmd:'sandbox.capabilities'});if(generation===terminalProbeGeneration.current)setTerminalProbe({status:'ready',capabilities:readCapabilities(result.sandbox)})}catch(error){if(generation===terminalProbeGeneration.current)setTerminalProbe({status:'error',error:String(error)})}}
  useEffect(()=>{if(action==='codex')void checkTerminalExecution();return()=>{terminalProbeGeneration.current++}},[action,project.id]);
  useEffect(()=>{setCapabilities(undefined);setHealth('');return()=>{cliGeneration.current++}},[action,project.id]);
  const agent=action==='codex'?'codex':'claude-code',isAgent=['codex','claude-code'].includes(action),supervised=isAgent&&agentMode==='managed';
  const restricted=action==='codex'&&terminalExecution==='restricted';
  const restrictedTerminalBlocked=terminalExecutionBlockReason('restricted',terminalProbe,!project.direct,supervised),selectedTerminalBlocked=action==='codex'?terminalExecutionBlockReason(terminalExecution,terminalProbe,!project.direct,supervised):'';
  const directOrdinary=!!project.direct&&!supervised,setupChoice=directOrdinary?'skip':setup;
  const setupDecision=setupChoice==='inherit'?(project.setup_default||'skip'):setupChoice;
  const needsReview=setupDecision==='run'&&!!project.setup?.trim();
  const setupBlocked=restricted&&setupDecision==='run'?'受限工具不能运行普通工作区准备脚本，请明确选择 skip（跳过）。':'';
  const promptBlocked=restricted&&goal.trim()?'受限工具模式不支持初始提示词投递；请清空此字段，启动后在终端手动输入。':'';
  const host=(project as Project&{host_id?:string|null}).host_id??'local';
  const parents=workspaces.filter(w=>((w as Workspace&{host_id?:string|null}).host_id??'local')===host);
  useEffect(()=>{setReviewed(false);setAck(false)},[project.id,project.setup,setupChoice,setupDecision]);
  useEffect(()=>{if(project.direct||parentId&&!parents.some(w=>w.id===parentId))setParentId('')},[project.id,project.direct,parentId,workspaces]);
  useEffect(()=>{dialog.current?.showModal()},[]);
  const setupStep=creation?.steps.find(s=>s.kind==='setup');
  const waiting=!!creation&&!prepared(creation)&&creation.steps.some(s=>['pending','running'].includes(s.state))&&!creation.steps.some(s=>['failed','uncertain'].includes(s.state));
  const locked=busy||sourceLoading||!!frozen.current||!!createdId.current;
  function recordCreation(value:Creation|null,retryReady=false){
    if(value&&value.request_id!==frozen.current?.requestId)throw new Error('创建记录与原请求不匹配，未启动或重试。');
    creationRef.current=value;setCreation(value);setRetryReady(retryReady);setRetryConfirmed(false);
    if(value?.frozen.workspace&&value.steps.some(step=>step.kind==='checkout'&&step.state==='succeeded'))createdId.current=value.frozen.workspace.id;
    const launch=value?.steps.find(step=>step.kind==='launch');
    if(launch?.state==='succeeded'&&launch.descriptor)sessionId.current=launch.descriptor.id;
  }
  function selectSource(item?:SourceItem){setSourceItem(item);setName(item?`${item.identifier} ${item.title}`.slice(0,50):'');setGoal(item?taskInstruction(item):'');setAck(false);launchKey.current=creationRequest(item)}
  function resetRequest(persistSource=true){frozen.current=null;createdId.current=null;sessionId.current=null;checkedCli.current=false;recordCreation(null);launchKey.current=crypto.randomUUID();if(persistSource&&sourceItem)localStorage.setItem(`velella:source-create:${sourceItem.key}`,launchKey.current);setError('');setReviewed(false);setAck(false)}
  async function finish(id:string){
    const snapshot=frozen.current;
    await onCreated(id,snapshot?.projectId||project.id);
    if(snapshot?.source)localStorage.removeItem(`velella:source-create:${snapshot.source.key}`);
    if(!keepCreating||snapshot?.direct){onClose();return}
    setNotice(`已打开“${snapshot?.title||'新工作区'}”，可以继续创建。`);
    resetRequest(false);selectSource(undefined);setCycle(value=>value+1);setBase('');setAutoSubmit(false);
    setHealth('');setAck(false);
    requestAnimationFrame(()=>dialog.current?.querySelector<HTMLInputElement>('.creation-search input')?.focus());
  }
  function freeze():FrozenRequest{
    const permission=!supervised&&!restricted&&agentCatalog.some(value=>value.id===action)?(permissionChoice==='default'?agentPermission(readAgentPreferences(target),action as AgentId):permissionChoice):'inherit';
    const sandbox=action==='codex'?terminalExecutionRequest(terminalExecution,terminalProbe,!project.direct,supervised):undefined;
    const launch:LaunchOptions=restricted?{}:{...(model.trim()&&capabilities?.model!==false?{model:model.trim()}:{}),...(effort&&capabilities?.effort!==false?{effort}:{})};
    const title=name.trim()||goal.trim().split('\n')[0].slice(0,40)||'新工作区';
    const shared={project_id:project.id,request_id:launchKey.current,parent_workspace_id:project.direct?null:parentId||null,...(sourceItem?{source_key:sourceItem.key}:{}),setup:setupChoice,...(needsReview&&reviewed?{reviewed_setup:project.setup}:{})};
    const reference=base.trim()?{base:base.trim()}:{};
    const initialPrompt=isAgent&&!restricted&&goal.trim()?{initial_prompt:{text:goal,mode:autoSubmit?'submit':'draft'}}:{};
    const request=supervised?{cmd:'task.create',...shared,auto_recover:agent==='codex'&&autoRecover,allowed_commands:agent==='claude-code'?allowed.split('\n').map(s=>s.trim()).filter(Boolean):[],spec:{name:title,agent,...reference,instruction:goal.trim(),launch,policy:{permissions:'ask',budget:{wall_clock:budget}},done:checks.split('\n').map(s=>s.trim()).filter(Boolean).map(run=>({run})),verify:{retries:1},on_done:'review'}}:{cmd:'workbench.create',...shared,name:title,...reference,execution:{mode:terminalExecution},...(action!=='none'?{launch:action,launch_options:launch,...initialPrompt}:{})};
    if(permission!=='inherit')Object.assign(request,{permission_mode:permission});
    return {request,requestId:launchKey.current,projectId:project.id,title,direct:!!project.direct,managed:supervised,action,agent,permission,source:sourceItem,goal,autoSubmit,launch,sandbox,setup:{decision:setupDecision,script:project.setup||''}};
  }
  async function continueRequest(){
    const snapshot=frozen.current;if(!snapshot||submitting.current)return;
    submitting.current=true;setBusy(true);setError('');setNotice('');
    try{
      let id=createdId.current;
      if(!id)await permissionLaunchFields(snapshot.permission,target);
      if(!id&&!checkedCli.current&&['codex','claude-code'].includes(snapshot.action)&&!snapshot.sandbox){
        const h=await rpc({cmd:'agent.check',agent:snapshot.agent});const supported=launchCapabilities(h);setCapabilities(supported);
        if(!h.available)throw new Error('未找到所选 CLI，请安装后重试，或开始新请求选择仅创建。');
        if(snapshot.launch.model&&supported?.model===false||snapshot.launch.effort&&supported?.effort===false)throw new Error('目标 CLI 帮助未提供所选启动参数。请开始新请求并移除不支持的参数。');
        checkedCli.current=true;
      }
      if(!id&&snapshot.managed){const r=await rpc(snapshot.request);id=taskWorkspaceId(r.task)}
      else if(!id&&snapshot.direct){id=`main:${snapshot.projectId}`}
      else if(!id){
        const r=await rpc(snapshot.request);id=r.workspace.id;createdId.current=id;
        recordCreation(r.creation||null);
        if(r.existing&&snapshot.source){await finish(id!);return}
        if(!r.creation&&snapshot.setup.decision==='run'&&snapshot.setup.script.trim())throw new Error('未收到准备脚本创建记录，暂不启动。请检查原请求状态。');
      }
      createdId.current=id;
      // Once accepted, only observe the original journal; never create again to poll setup.
      if(!snapshot.managed&&!snapshot.direct){
        if(creationRef.current&&!prepared(creationRef.current))return;
        if(!creationRef.current&&snapshot.setup.decision==='run'&&snapshot.setup.script.trim())throw new Error('准备脚本结果未知，请检查原请求状态；不会自动重试。');
      }
      if(snapshot.source)await rpc({cmd:'workbench.source.attach',workspace_id:id,source_key:snapshot.source.key});
      if(!snapshot.managed&&snapshot.action!=='none'){
        if(!sessionId.current){
          if(!snapshot.direct)throw new Error('原创建记录尚未确认会话；请检查原请求，不会从界面另行启动。');
          const r=await rpc({cmd:'workbench.terminal.open',workspace_id:id,launch:snapshot.action,request_id:snapshot.requestId,...(snapshot.permission!=='inherit'?{permission_mode:snapshot.permission}:{}),...(snapshot.sandbox?{sandbox:snapshot.sandbox}:{}),launch_options:snapshot.launch,...(snapshot.request.initial_prompt?{initial_prompt:snapshot.request.initial_prompt}:{})});
          if(!r.session_id)throw new Error('未收到会话标识，请使用原请求继续。');sessionId.current=r.session_id;
        }
        const r=await rpc({cmd:'workbench.layout',workspace_id:id});const layout=r.layout;
        const pane=layout.panes.find((p:any)=>p.tabs.some((t:any)=>tabSessions(t).includes(sessionId.current!)))||layout.panes[0];
        let tab=pane.tabs.find((t:any)=>tabSessions(t).includes(sessionId.current!));
        if(!tab){tab={id:crypto.randomUUID(),kind:'terminal',target:sessionId.current,title:snapshot.action==='terminal'?'PowerShell':snapshot.action==='codex'?(snapshot.sandbox?'Codex · 工具受限':'Codex'):'Claude'};pane.tabs.push(tab)}
        pane.active=tab.id;await rpc({cmd:'workbench.layout.save',workspace_id:id,layout});
        if(['codex','claude-code'].includes(snapshot.action)&&snapshot.goal.trim()&&!snapshot.sandbox)saveSourceDraft(id!,sessionId.current!,snapshot.goal,snapshot.source?.key||'initial:'+snapshot.requestId,snapshot.autoSubmit,snapshot.direct?undefined:snapshot.requestId);
      }
      if(snapshot.source)localStorage.setItem('velella:source-launch',snapshot.action);
      await finish(id!);
    }catch(e){setError((createdId.current?'工作区已保留，重试会继续打开，不会重复创建。\n':'请求已冻结，重试会使用完全相同的内容。\n')+String(e))}finally{submitting.current=false;setBusy(false)}
  }
  async function refreshStatus(){
    const snapshot=frozen.current;if(!snapshot||submitting.current)return;
    submitting.current=true;setBusy(true);setError('');
    try{const r=await rpc({cmd:'workbench.creation.status',request_id:snapshot.requestId});if(!r.creation)throw new Error('未找到原请求创建记录；未启动或自动重试。');recordCreation(r.creation,r.retry_ready===true)}
    finally{submitting.current=false;setBusy(false)}
    if(creationRef.current&&prepared(creationRef.current))await continueRequest();
  }
  useEffect(()=>{
    if(!waiting&&!(setupStep?.state==='failed'&&!retryReady))return;
    let live=true;
    const timer=setTimeout(async()=>{
      const snapshot=frozen.current;if(!snapshot)return;
      try{const r=await rpc({cmd:'workbench.creation.status',request_id:snapshot.requestId});if(!live)return;if(!r.creation)throw new Error('原请求创建记录不可用；未启动或自动重试。');recordCreation(r.creation,r.retry_ready===true);if(prepared(r.creation))await continueRequest()}
      catch(e){if(live)setError(String(e))}
    },700);
    return()=>{live=false;clearTimeout(timer)};
  },[creation,waiting,retryReady,setupStep?.state]);
  async function retrySetup(){
    const snapshot=frozen.current,script=creationRef.current?.frozen.setup?.script;
    if(!snapshot||!retryConfirmed||!retryReady||setupStep?.state!=='failed'||script===undefined||submitting.current)return;
    submitting.current=true;setBusy(true);setError('');setRetryConfirmed(false);
    try{const r=await rpc({cmd:'workbench.creation.retry',request_id:snapshot.requestId,confirmed:true,reviewed_setup:script});if(!r.creation)throw new Error('未收到重试记录，请检查原请求状态。');recordCreation(r.creation,r.retry_ready===true)}catch(e){setError(String(e))}finally{submitting.current=false;setBusy(false)}
    if(creationRef.current&&prepared(creationRef.current))await continueRequest();
  }
  async function newRequest(){
    if(submitting.current||!frozen.current)return;submitting.current=true;setBusy(true);
    try{const r=await rpc({cmd:'workbench.creation.status',request_id:frozen.current.requestId});if(r.creation){setError('原请求已被接受，请使用原请求继续；不能更换请求标识。');return}resetRequest()}
    catch(e){setError('无法确认原请求尚未接受，未更换请求标识。'+String(e))}finally{submitting.current=false;setBusy(false)}
  }
  async function submit(e:React.FormEvent){
    e.preventDefault();if(submitting.current||sourceLoading||waiting)return;
    if(!frozen.current){if(selectedTerminalBlocked||setupBlocked||promptBlocked||needsReview&&!reviewed||supervised&&(!ack||!goal.trim()))return;try{frozen.current=freeze()}catch(error){setError(String(error));return}}
    await continueRequest();
  }
  async function check(){const generation=++cliGeneration.current;setHealth('正在检查 CLI…');try{const r=await rpc({cmd:'agent.check',agent});if(generation!==cliGeneration.current)return;const supported=launchCapabilities(r);setCapabilities(supported);if(supported?.model===false)setModel('');if(supported?.effort===false)setEffort('');setHealth(`${r.available?r.version:'未找到 CLI'} · ${r.authenticated?'登录检查通过':'请先完成登录'}${supported?' · 已依据目标 CLI 帮助检查参数支持；具体模型是否被接受尚未验证。':' · 未提供参数支持证据。'}`)}catch(e){if(generation===cliGeneration.current)setHealth(String(e))}}
  const blocked=!!frozen.current?(!!creation&&!prepared(creation)):supervised&&(!ack||!goal.trim())||needsReview&&!reviewed||!!selectedTerminalBlocked||!!setupBlocked||!!promptBlocked;
  const shownSetup=creation?.frozen.setup||frozen.current?.setup||{decision:setupDecision,script:project.setup||''};
  const setupPath=creation?.frozen.workspace?.path||(project.direct?project.path:project.path.replace(/[\\/]+$/,'')+'/.velella/worktrees/'+launchKey.current);
  return <NativeDialog ref={dialog} className="workspace-composer creation-dialog" aria-labelledby="workspace-composer-title" onClose={onClose} onCancel={e=>{if(busy||sourceLoading)e.preventDefault()}}><form onSubmit={submit} onKeyDown={e=>{if((e.ctrlKey||e.metaKey)&&e.key==='Enter'&&!e.nativeEvent.isComposing){e.preventDefault();e.currentTarget.requestSubmit()}}}>
    <div className="dialog-heading"><div className="creation-heading-copy"><h2 id="workspace-composer-title">{project.direct?'打开文件夹工作区':'新建工作树'}</h2><p>{project.direct?'在现有项目目录中继续工作。':'从现有分支创建一个独立工作区。'}</p></div><Button type="button" aria-label="关闭新建工作区" disabled={busy||sourceLoading} onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body">
      <fieldset className="creation-fields" disabled={locked}>
      <div className="creation-context"><div className="creation-field"><div className="creation-label-row"><label htmlFor="creation-project">项目</label>{onNewProject&&<Button type="button" title="创建项目文件夹" aria-label="创建项目文件夹" onClick={onNewProject}><FolderPlus size={14}/><span>新建项目</span></Button>}</div><div className="creation-control"><Folder size={15}/><select id="creation-project" aria-label="开发项目" value={projectId} onChange={e=>{setProjectId(e.target.value);setBase('');setReviewed(false);setAck(false)}}>{(projects||[project]).map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select><ChevronDown size={14}/></div></div>
      <div className="creation-field"><span className="creation-label">运行于</span><div className="creation-location">{remote?<Server size={15}/>:<Monitor size={15}/>}<strong>{hostLabel}</strong><span title={project.path}>{project.path}</span></div></div></div>
      {project.direct&&<p className="creation-hint">直接打开原文件夹。需要隔离的工作树时，请选择 Git 项目。</p>}
      <CreationSourcePicker key={project.id+':'+cycle} project={project} selected={sourceItem} name={name} base={base} disabled={locked} onSelect={selectSource} onName={setName} onBase={value=>{setBase(value);setAck(false)}} onLoading={setSourceLoading} onSources={onSources}/>
      <label className="creation-field"><span className="creation-label">智能体 <small>可选</small></span><span className="creation-control">{action==='terminal'?<Terminal size={15}/>:<Bot size={15}/>}<select aria-label="创建后" value={action} onChange={e=>{setAction(e.target.value);setAutoSubmit(false);setAgentMode('native');setSetup('inherit');setEffort('');setModel('');setCapabilities(undefined);if(e.target.value!=='codex')setTerminalExecution('native');setHealth('');setAck(false)}}><option value="none">不启动 · 仅创建工作区</option><option value="codex">Codex</option><option value="claude-code">Claude Code</option><option value="terminal">终端</option></select><ChevronDown size={14}/></span></label>
      {isAgent&&<label className="creation-field"><span className="creation-label">智能体运行方式</span><span className="creation-control"><select aria-label="智能体运行方式" value={agentMode} onChange={e=>{const mode=e.target.value as 'native'|'managed';setAgentMode(mode);setSetup(mode==='managed'?'run':'inherit');setReviewed(false);setAck(false)}}><option value="native">原生 CLI · 交互式</option><option value="managed" disabled={restricted}>受管任务 · 明确目标与验证</option></select><ChevronDown size={14}/></span></label>}
      <details className="creation-advanced"><summary>高级设置 <ChevronDown size={13}/>{supervised&&!ack&&<small>请确认执行权限</small>}{needsReview&&!reviewed&&<small>请审阅准备脚本</small>}</summary><div className="composer-advanced">
      <label>名称<Input aria-label="工作区名称" value={name} maxLength={50} placeholder="新工作区" onChange={e=>setName(e.target.value)}/></label>
      <label>父级工作区<select aria-label="父级工作区" disabled={!!project.direct} value={project.direct?'':parentId} onChange={e=>setParentId(e.target.value)}><option value="">顶层工作区</option>{!project.direct&&parents.map(w=><option key={w.id} value={w.id}>{projects?.find(p=>p.id===w.project_id)?.name||w.project_id} · {w.name}</option>)}</select><small>父级仅用于组织，不改变 Git 基准、目录或权限。{project.direct?'直接文件夹固定为顶层。':''}</small></label>
      {!project.direct&&<label>自定义基准引用<Input aria-label="自定义基准引用" value={base} onChange={e=>{setBase(e.target.value);setAck(false)}} placeholder="留空使用项目默认；分支、标签或提交 SHA"/></label>}
      {action==='codex'&&<fieldset className="sandbox-fields"><legend>Codex 终端工具执行环境</legend>
        <label>执行模式<select aria-label="执行模式" aria-describedby="terminal-execution-status terminal-execution-scope" value={terminalExecution} onChange={e=>setTerminalExecution(e.target.value as TerminalExecutionMode)}><option value="native">{remote?'目标主机执行':'本机执行'}</option><option value="restricted" disabled={!!restrictedTerminalBlocked}>受限工具（实验性/Linux）</option></select></label>
        <div className="health-check"><span id="terminal-execution-status" role="status">{restrictedTerminalBlocked||'内核支持文件与网络限制；Codex 终端依赖将在启动时检查。'}</span><Button type="button" disabled={terminalProbe.status==='loading'} onClick={()=>void checkTerminalExecution()}>重新检查</Button></div>
        <p id="terminal-execution-scope" className="muted">{restricted?restrictedTerminalDescription:(remote?'使用远端执行主机上的 CLI 配置和原生审批规则。受限工具模式只适用于 Git 工作树。':'使用本机 Codex 配置和原生审批规则。受限工具模式只适用于 Git 工作树。')}</p>
      </fieldset>}
      {isAgent&&<>
        {!supervised&&!restricted&&<AgentPermissionField agent={agent} preferences={preferences} value={frozen.current?.permission??permissionChoice} onChange={setPermissionChoice} disabled={locked}/>}
        <label>模型<Input aria-label="模型" disabled={restricted||capabilities?.model===false} value={model} placeholder="继承执行主机 CLI 设置" onChange={e=>{setModel(e.target.value);setAck(false)}}/></label>
        <label>推理强度<select aria-label="推理强度" disabled={restricted||capabilities?.effort===false} value={effort} onChange={e=>{setEffort(e.target.value);setAck(false)}}><option value="">继承执行主机 CLI 设置</option>{(agent==='codex'?['none','minimal','low','medium','high','xhigh']:['low','medium','high','xhigh','max']).map(value=><option key={value} value={value}>{value}</option>)}</select></label>
        <p>模型与推理强度留空时继承执行主机 CLI 设置。这里只请求启动参数，不保证具体模型已被接受。{restricted?'受限工具模式禁用这些启动参数。':''}</p>
        {!restricted&&<><Button type="button" onClick={check}>检查 CLI 与登录</Button>{health&&<p role="status">{health}</p>}</>}
        <label>{sourceItem?'任务指令':'工作目标'}<Textarea aria-label={sourceItem?'任务指令':'工作目标'} rows={4} placeholder={supervised?'填写非空的受管任务指令':'可选；填入原生 CLI 输入框作为草稿'} value={goal} onChange={e=>{setGoal(e.target.value);setAck(false)}}/><small>{supervised?'指令交由受管任务执行，保留原生 ask 权限与 review 交付。':'提示词不代表受管任务；默认仅填入输入框，不自动提交。'}</small></label>
        {!supervised&&<label className="ack"><input type="checkbox" checked={autoSubmit} disabled={restricted||action==='codex'} onChange={e=>setAutoSubmit(e.target.checked)}/>输入就绪后自动提交初始提示词；绝不处理权限确认对话框。</label>}
        {!supervised&&action==='codex'&&!restricted&&<p>Codex 尚无可核实的首次输入就绪信号；提示词会保留，请在原终端手动粘贴并提交。</p>}
        {promptBlocked&&<p role="alert">{promptBlocked}</p>}
      </>}
      <label>准备脚本策略<select aria-label="准备脚本策略" disabled={directOrdinary} value={setupChoice} onChange={e=>{setSetup(e.target.value as SetupChoice);setReviewed(false);setAck(false)}}><option value="inherit">inherit · 继承项目设置</option><option value="run">run · 执行</option><option value="skip">skip · 跳过</option></select></label>
      <label>项目准备脚本<Textarea aria-label="项目准备脚本" rows={2} readOnly value={shownSetup.script} placeholder="未配置"/></label>
      <p>实际决定：{shownSetup.decision==='run'?(shownSetup.script.trim()?'run · 执行准备脚本':'run · 无脚本，跳过执行'):'skip · 跳过准备脚本'}。执行主机：{hostLabel}；路径：{setupPath}。</p>
      {directOrdinary&&<p>直接文件夹不运行普通工作树准备脚本，固定 skip；受管任务仍支持原有准备流程。</p>}
      {setupBlocked&&<p role="alert">{setupBlocked}</p>}
      {needsReview&&<label className="ack"><input type="checkbox" checked={reviewed} onChange={e=>setReviewed(e.target.checked)}/>我已审阅本次准备脚本，并同意执行。</label>}
      {supervised&&<>
        <label>时间上限<Input value={budget} required onChange={e=>{setBudget(e.target.value);setAck(false)}}/></label>
        <label>验证命令<Textarea rows={2} placeholder="每行一条；留空时结果标为未验证" value={checks} onChange={e=>{setChecks(e.target.value);setAck(false)}}/></label>
        {agent==='codex'&&<label className="ack"><input type="checkbox" checked={autoRecover} onChange={e=>{setAutoRecover(e.target.checked);setAck(false)}}/>明确安全时自动恢复一次；权限或输入不确定时停止。</label>}
        {agent==='claude-code'&&<label>预授权 Shell 命令<Textarea rows={2} value={allowed} onChange={e=>{setAllowed(e.target.value);setAck(false)}}/></label>}
        <label className="ack"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)}/>我确认本次目标、原生权限、准备脚本和验证设置。</label>
      </>}
      </div></details>
      </fieldset>
      {creation&&<section aria-label="创建进度"><p>创建记录：{creation.request_id}</p>{creation.steps.map(step=><div key={step.kind}><p role="status">{stepNames[step.kind]||step.kind}：{stepStates[step.state]||'结果未知'}</p>{step.error&&<p className="creation-error">{step.error}</p>}{step.descriptor&&<p>命令日志：{step.descriptor.id}{step.descriptor.directory?` · ${step.descriptor.directory}`:''}</p>}</div>)}{creation.frozen.workspace&&<p>执行主机：{hostLabel}；保留路径：{creation.frozen.workspace.path}</p>}</section>}
      {creation&&['failed','uncertain'].includes(setupStep?.state||'')&&<>
        <p>准备脚本未成功；不会自动重试。请先检查保留工作区的命令日志。</p>
        <label>冻结的准备脚本<Textarea readOnly value={creation.frozen.setup?.script||''}/></label>
        {setupStep?.state==='failed'&&<><label className="ack"><input type="checkbox" checked={retryConfirmed} disabled={busy||!retryReady} onChange={e=>setRetryConfirmed(e.target.checked)}/>我已核对保留日志，并确认重试上述冻结脚本。</label>
        <Button type="button" disabled={busy||!retryReady||!retryConfirmed||creation.frozen.setup?.script===undefined} onClick={()=>void retrySetup()}>确认重试准备脚本</Button>{!retryReady&&<p>等待原脚本进程确认退出后才能重试。</p>}</>}
      </>}
      {createdId.current&&<Button type="button" disabled={busy} onClick={()=>{const id=createdId.current!;void onCreated(id,frozen.current?.projectId||project.id).then(onClose).catch(e=>setError(String(e)))}}>打开已保留工作区</Button>}
      {frozen.current&&!frozen.current.managed&&(error||creation&&!prepared(creation))&&<Button type="button" disabled={busy} onClick={()=>void refreshStatus().catch(e=>setError(String(e)))}>检查原请求状态</Button>}
      {frozen.current&&!createdId.current&&error&&<Button type="button" disabled={busy} onClick={()=>void newRequest()}>重新编辑并开始新请求</Button>}
      {sourceLoading&&<p className="creation-hint" role="status">正在读取任务详情…</p>}
      {notice&&<p className="creation-notice" role="status">{notice}</p>}
      {error&&<p role="alert" className="creation-error">{error}</p>}
    </div><div className="dialog-footer"><label className="creation-more"><input type="checkbox" role="switch" checked={keepCreating} disabled={locked||!!project.direct} onChange={e=>setKeepCreating(e.target.checked)}/><span>创建更多</span></label><Button className="primary" disabled={busy||sourceLoading||waiting||blocked}>{busy?'正在创建…':waiting?'等待创建步骤…':createdId.current?'继续打开':project.direct?'打开工作区':'创建并打开'}<kbd aria-hidden="true">Ctrl ↵</kbd></Button></div>
  </form></NativeDialog>;
}
