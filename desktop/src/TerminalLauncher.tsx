import {useEffect,useId,useRef,useState,type FormEvent} from 'react';
import {X} from 'lucide-react';
import {Button,NativeDialog} from './ui';
import {rpc} from './api';
import type {Workspace} from './workbench-types';
import {readCapabilities,type SandboxProbe} from './sandbox-policy';
import {restrictedTerminalDescription,terminalExecutionBlockReason,terminalExecutionRequest,type TerminalExecutionMode} from './terminal-execution';
import {currentTarget,sameTarget,type ExecutionTarget} from './execution-target';
import {agentPermission,type AgentPermissionMode} from './agent-preferences';
import {readAgentPreferences,useAgentPreferences} from './useAgentPreferences';
import AgentPermissionField from './AgentPermissionField';
import {permissionLaunchFields} from './agent-permission-launch';
import {defaultAccountHome,accountLaunchFields} from './account-preferences';

interface LaunchAttempt {
  requestId:string;
  workspaceId:string;
  mode:TerminalExecutionMode;
  sandbox:{mode:'restricted'}|undefined;
  sessionId:string|null;
  launchError:string|null;
  target:ExecutionTarget;
  permission:AgentPermissionMode;
  accountHome?:string;
}

interface TerminalLauncherProps {
  workspace:Workspace;
  taskOwned:boolean;
  nativeBlockedReason?:string;
  returnFocusTo?:HTMLElement|null;
  onClose:()=>void;
  onOpened:(sessionId:string,mode:TerminalExecutionMode,program?:string,target?:ExecutionTarget)=>Promise<void>;
}

export default function TerminalLauncher({workspace,taskOwned,nativeBlockedReason,returnFocusTo,onClose,onOpened}:TerminalLauncherProps){
  const id=useId(),dialog=useRef<HTMLDialogElement>(null),modeSelect=useRef<HTMLSelectElement>(null);
  const [target]=useState(currentTarget);
  const {preferences}=useAgentPreferences(target);
  const [permissionChoice,setPermissionChoice]=useState<'default'|AgentPermissionMode>('default');
  const [mode,setMode]=useState<TerminalExecutionMode>('restricted');
  const [probe,setProbe]=useState<SandboxProbe>({status:'loading'});
  const [busy,setBusy]=useState(false),[error,setError]=useState('');
  const [attempted,setAttempted]=useState(false),[sessionId,setSessionId]=useState<string|null>(null);
  const attempt=useRef<LaunchAttempt|null>(null),submitting=useRef(false),mounted=useRef(false),closing=useRef(false);
  const probeGeneration=useRef(0),returnFocus=useRef<HTMLElement|null>(null),currentWorkspace=useRef(workspace.id);
  currentWorkspace.current=workspace.id;
  const worktree=workspace.kind==='worktree';
  const restrictedBlocked=taskOwned
    ?'此工作树属于受管任务，不能新建受限 Codex 终端。请使用独立的 Git 工作树。'
    :terminalExecutionBlockReason('restricted',probe,worktree,false);
  const selectedBlocked=mode==='restricted'?restrictedBlocked:(nativeBlockedReason||'');
  const titleId=`${id}-title`,contextId=`${id}-workspace`,modeId=`${id}-mode`;
  const statusId=`${id}-status`,scopeId=`${id}-scope`,lockedId=`${id}-locked`,errorId=`${id}-error`;

  async function checkCapabilities(){
    const generation=++probeGeneration.current;
    setProbe({status:'loading'});
    try{
      const result=await rpc({cmd:'sandbox.capabilities'},target);
      if(mounted.current&&generation===probeGeneration.current)setProbe({status:'ready',capabilities:readCapabilities(result.sandbox)});
    }catch(error){
      if(mounted.current&&generation===probeGeneration.current)setProbe({status:'error',error:String(error)});
    }
  }

  useEffect(()=>{
    mounted.current=true;
    if(!returnFocus.current){
      if(returnFocusTo)returnFocus.current=returnFocusTo;
      else if(document.activeElement instanceof HTMLElement)returnFocus.current=document.activeElement;
    }
    if(!dialog.current?.open)dialog.current?.showModal();
    modeSelect.current?.focus();
    return()=>{
      mounted.current=false;
      probeGeneration.current++;
      if(returnFocus.current?.isConnected)returnFocus.current.focus();
    };
  },[]);
  useEffect(()=>{
    void checkCapabilities();
    return()=>{probeGeneration.current++};
  },[workspace.id]);

  function close(){
    if(submitting.current||closing.current)return;
    closing.current=true;
    onClose();
  }

  async function submit(event:FormEvent){
    event.preventDefault();
    if(submitting.current||closing.current)return;
    submitting.current=true;
    setBusy(true);
    setError('');
    try{
      if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，请关闭此窗口后重新打开。');
      let launch=attempt.current;
      if(!launch){
        if(selectedBlocked)throw new Error(selectedBlocked);
        const sandbox=terminalExecutionRequest(mode,probe,worktree,taskOwned);
        launch={requestId:crypto.randomUUID(),workspaceId:workspace.id,mode,sandbox,sessionId:null,launchError:null,target,permission:mode==='native'?(permissionChoice==='default'?agentPermission(readAgentPreferences(target),'codex'):permissionChoice):'inherit',accountHome:mode==='native'?defaultAccountHome('codex',target):undefined};
        attempt.current=launch;
        setAttempted(true);
      }
      if(launch.workspaceId!==currentWorkspace.current)throw new Error('工作区已切换，请关闭此窗口后重新打开。原创建请求仍保留在原工作区。');
      if(!launch.sessionId){
        if(selectedBlocked)throw new Error(selectedBlocked);
        try{
          const fields=await permissionLaunchFields(launch.permission,launch.target);
          const account=await accountLaunchFields(launch.accountHome,launch.target);
          if(!sameTarget(launch.target,currentTarget()))throw new Error('执行主机已切换，未启动终端。');
          const result=await rpc({cmd:'workbench.terminal.open',workspace_id:launch.workspaceId,launch:'codex',request_id:launch.requestId,...fields,...account,...(launch.sandbox?{sandbox:launch.sandbox}:{})},launch.target);
          if(result.session_id!==launch.requestId)throw new Error('后台未返回本次请求的会话，请重试确认创建结果。');
          launch.sessionId=result.session_id;
          launch.launchError=null;
          if(mounted.current)setSessionId(launch.sessionId);
        }catch(openError){
          launch.launchError=String(openError);
          try{
            const result=await rpc({cmd:'workbench.sessions',workspace_id:launch.workspaceId},launch.target);
            if(Array.isArray(result.sessions)&&result.sessions.some((session:{id?:unknown})=>session?.id===launch.requestId)){
              launch.sessionId=launch.requestId;
              if(mounted.current)setSessionId(launch.sessionId);
            }
          }catch{
            // The original request stays fixed while its outcome is unknown.
          }
          throw openError;
        }
      }
      if(!mounted.current)return;
      if(launch.workspaceId!==currentWorkspace.current)throw new Error('工作区已切换。会话已保留，请从原工作区的会话列表继续。');
      if(!sameTarget(launch.target,currentTarget()))throw new Error('执行主机已切换，会话保留在原主机。');
      await onOpened(launch.sessionId!,launch.mode,'codex',launch.target);
      if(mounted.current){closing.current=true;onClose()}
    }catch(error){
      if(mounted.current){
        const launch=attempt.current;
        const recovery=launch?.sessionId
          ?launch.launchError
            ?'已找到本次请求的会话记录，但尚未确认启动成功。选择“查看已有会话”检查状态。'
            :'会话已保留。选择“继续打开”重试打开此会话。'
          :launch
            ?'创建结果尚未确认。重试会沿用本次请求和执行模式。'
            :'';
        setError([recovery,String(error)].filter(Boolean).join('\n'));
      }
    }finally{
      submitting.current=false;
      if(mounted.current)setBusy(false);
    }
  }

  const status=mode==='restricted'
    ?restrictedBlocked||'内核支持文件与网络限制；Codex 终端依赖将在启动时检查。'
    :nativeBlockedReason||'使用本机 Codex 配置和原生审批规则。';
  const submitLabel=busy
    ?sessionId?'正在打开…':'正在创建…'
    :sessionId
      ?attempt.current?.launchError?'查看已有会话':'继续打开'
      :attempted?'重试创建':'创建并打开';
  return <NativeDialog ref={dialog} className="workspace-composer" aria-labelledby={titleId} aria-describedby={contextId} onClose={close} onCancel={event=>{event.preventDefault();close()}}>
    <form onSubmit={submit} aria-busy={busy}>
      <div className="dialog-heading"><h2 id={titleId}>新建 Codex 终端</h2><Button type="button" aria-label="关闭新建 Codex 终端" disabled={busy} onClick={close}><X size={16} aria-hidden="true"/></Button></div>
      <div className="dialog-body">
        <p id={contextId} className="composer-project">{workspace.name}<small>{workspace.path}</small></p>
        <fieldset className="sandbox-fields"><legend>Codex 终端工具执行环境</legend>
          <label htmlFor={modeId}>执行模式<select ref={modeSelect} id={modeId} value={mode} disabled={busy||attempted} aria-describedby={[statusId,scopeId,attempted?lockedId:'',error?errorId:''].filter(Boolean).join(' ')} onChange={event=>{setMode(event.target.value as TerminalExecutionMode);setError('')}}>
            <option value="restricted">受限工具（实验性/Linux）</option>
            <option value="native">本机执行</option>
          </select></label>
          <div className="health-check"><span id={statusId} role="status">{status}</span><Button type="button" disabled={busy||probe.status==='loading'} onClick={()=>void checkCapabilities()}>重新检查</Button></div>
          <p id={scopeId} className="muted">{mode==='restricted'?restrictedTerminalDescription+' 此模式使用自身限制，不应用默认权限。':'在当前工作区启动本机 Codex，应用设置中的 Codex 默认权限。'}</p>
          {attempted&&<p id={lockedId} className="muted">本次请求的执行模式已固定。{sessionId?'关闭此窗口会保留会话，可从会话列表继续。':'关闭此窗口后，可从会话列表检查创建结果。'}</p>}
        </fieldset>
        {mode==='native'&&<AgentPermissionField agent="codex" preferences={preferences} value={attempt.current?.permission??permissionChoice} onChange={setPermissionChoice} disabled={busy||attempted}/>}
        {error&&<p id={errorId} role="alert" className="ide-error">{error}</p>}
      </div>
      <div className="dialog-footer"><Button type="button" disabled={busy} onClick={close}>{sessionId?'关闭':'取消'}</Button><Button type="submit" className="primary" disabled={busy||(!sessionId&&!!selectedBlocked)} aria-describedby={error?errorId:statusId}>{submitLabel}</Button></div>
    </form>
  </NativeDialog>;
}
