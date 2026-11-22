import {useEffect,useRef,useState} from 'react';
import {X} from 'lucide-react';
import {Button,NativeDialog} from './ui';
import {currentTarget,sameTarget,type ExecutionTarget} from './execution-target';
import {agentCatalog,agentPermission,type AgentId,type AgentPermissionMode} from './agent-preferences';
import {readAgentPreferences,useAgentPreferences} from './useAgentPreferences';
import AgentPermissionField from './AgentPermissionField';
export default function AgentTerminalLauncher({agent,workspaceName,onClose,onLaunch,returnFocusTo}:{agent:AgentId;workspaceName:string;onClose:()=>void;onLaunch:(agent:AgentId,mode:AgentPermissionMode,target:ExecutionTarget)=>Promise<void>;returnFocusTo?:HTMLElement|null}){
 const dialog=useRef<HTMLDialogElement>(null),submitting=useRef(false),frozen=useRef<AgentPermissionMode|null>(null);
 const [target]=useState(currentTarget),{preferences}=useAgentPreferences(target),[choice,setChoice]=useState<'default'|AgentPermissionMode>('default');
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[attempted,setAttempted]=useState(false);
 const name=agentCatalog.find(value=>value.id===agent)!.name;
 useEffect(()=>{dialog.current?.showModal();return()=>{if(returnFocusTo?.isConnected)returnFocusTo.focus()}},[]);
 function close(){if(!submitting.current)onClose()}
 async function submit(){
  if(submitting.current)return;
  submitting.current=true;setBusy(true);setError('');
  try{
   if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，请从原主机重新打开。');
   if(frozen.current===null)frozen.current=choice==='default'?agentPermission(readAgentPreferences(target),agent):choice;
   setAttempted(true);
   await onLaunch(agent,frozen.current,target);onClose();
  }catch(error){setError(String(error))}finally{submitting.current=false;setBusy(false)}
 }
 return <NativeDialog ref={dialog} className="workspace-composer agent-terminal-launcher" aria-label={`新建 ${name} 终端`} onCancel={event=>{event.preventDefault();close()}}>
  <form onSubmit={event=>{event.preventDefault();void submit()}} aria-busy={busy}>
   <div className="dialog-heading"><h2>新建 {name} 终端</h2><Button type="button" aria-label="关闭启动选项" disabled={busy} onClick={close}><X size={16}/></Button></div>
   <div className="dialog-body"><p className="composer-project">{workspaceName}</p><AgentPermissionField agent={agent} preferences={preferences} value={frozen.current??choice} onChange={setChoice} disabled={busy||attempted}/>
    {attempted&&<p className="muted">本次请求已固定。重试继续同一会话；关闭后仍可从已有会话查看。</p>}{error&&<p role="alert" className="ide-error">{error}</p>}
   </div><div className="dialog-footer"><Button type="button" disabled={busy} onClick={close}>取消</Button><Button type="submit" className="primary" disabled={busy}>{busy?'正在打开…':attempted?'继续打开':'创建并打开'}</Button></div>
  </form></NativeDialog>;
}
