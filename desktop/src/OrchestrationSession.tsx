import {useEffect,useState} from 'react';
import {rpc} from './api';
import {Button,Modal} from './ui';
import type {Session,Workspace} from './workbench-types';
import PersistentTerminalView from './TerminalSessions';
import {HybridApprovals} from './HybridCodexLauncher';

export default function OrchestrationSession({workspace,session,onClose,onExpand}:{workspace:Workspace;session:string;onClose:()=>void;onExpand:()=>void}){
  const [current,setCurrent]=useState<Session|null>(null),[error,setError]=useState(''),[loaded,setLoaded]=useState(false);
  useEffect(()=>{let live=true,timer:ReturnType<typeof setTimeout>;async function poll(){try{const result=await rpc<{sessions:Session[]}>({cmd:'workbench.sessions',workspace_id:workspace.id});if(live){setCurrent(result.sessions.find(s=>s.id===session)||null);setLoaded(true);setError('');}}catch(e){if(live)setError(String(e));}if(live)timer=setTimeout(poll,1500);}void poll();return()=>{live=false;clearTimeout(timer);};},[workspace.id,session]);
  return <Modal open onOpenChange={open=>{if(!open)onClose();}} title={current?.title||'协作会话'}><div className="orch-session"><div className="orch-session-toolbar"><span>{workspace.name} · {workspace.path}</span><Button onClick={onExpand}>在工作台展开</Button></div>{error?<p role="alert">{error}</p>:!current?<p role="status">{loaded?'该会话暂不可用，请在工作台核对。':'正在连接会话…'}</p>:current.native_chat?<p>独立 Codex 聊天已停用，历史数据已保留。请使用 Codex 终端。</p>:<><PersistentTerminalView workspace={{projectId:workspace.project_id,workspaceId:workspace.id,id:current.id,program:current.program,agent:true,status:current.status}}/>{current.hybrid&&<HybridApprovals workspaceId={workspace.id} sessionId={current.id}/>}</>}</div></Modal>;
}
