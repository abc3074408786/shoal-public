import {useEffect,useState} from 'react';
import {rpc} from './api';
import {Button,Modal} from './ui';
import LoadingState from './LoadingState';
import type {Session} from './workbench-types';
import {agentCatalog} from './agent-preferences';

export default function NativeSessionRestore({workspaceId,session,onSelected}:{workspaceId:string;session:Session;onSelected?:()=>Promise<void>}){
  const provider=agentCatalog.find(agent=>agent.program===session.program?.split(/[\\/]/).at(-1)?.replace(/\.(exe|cmd|ps1)$/i,''))?.name||session.title;
  const [open,setOpen]=useState(true),[sessions,setSessions]=useState<{id:string;title:string;created_at:number}[]|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{let current=true;void rpc({cmd:'workbench.terminal.restore.list',workspace_id:workspaceId,session_id:session.id}).then(r=>{if(current)setSessions(r.sessions)}).catch(e=>{if(current)setError(String(e))});return()=>{current=false}},[workspaceId,session.id]);
  async function select(native:string){setBusy(true);setError('');try{await rpc({cmd:'workbench.terminal.restore.select',workspace_id:workspaceId,session_id:session.id,native_id:native,confirmed:true});await onSelected?.();setOpen(false)}catch(e){setError(String(e))}finally{setBusy(false)}}
  return <><Button onClick={()=>setOpen(true)}>选择原 {provider} 会话</Button><Modal open={open} onOpenChange={value=>{if(!busy)setOpen(value)}} title={`首次还原 ${provider} 会话`}><p>这个标签尚未绑定原会话。请明确选择需要继续的对话，应用不会猜测最近会话。</p>{error&&<p role="alert">{error}</p>}{!sessions&&!error&&<LoadingState compact label="正在读取原会话"/>}<div className="wb-session-list">{sessions?.map(item=><Button key={item.id} disabled={busy} onClick={()=>void select(item.id)}><span><strong>{item.title||'未命名会话'}</strong><small>{new Date(item.created_at).toLocaleString()} · {item.id}</small></span></Button>)}{sessions?.length===0&&<p>此目录没有找到可还原的原会话。</p>}</div></Modal></>;
}
