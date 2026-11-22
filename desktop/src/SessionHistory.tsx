import {useState} from 'react';
import {Button,Input,Modal} from './ui';
import {AgentProgramIcon} from './AgentLogo';
import TerminalView from './TerminalView';
import type {Session,Workspace} from './workbench-types';
import {terminalSessionIsLive,terminalSessionStatus,terminalSessionTitle} from './terminal-execution';
import {filterSessionHistory,isAgentSession,type SessionHistoryFilter} from './session-history';
import './session-history.css';

export default function SessionHistory({workspace,sessions,openCount,busy,error,initialSession,onClose,onResume}:{workspace:Workspace;sessions:Session[];openCount:number;busy:boolean;error:string;initialSession?:string;onClose:()=>void;onResume:(session:Session)=>void}){
  const [filter,setFilter]=useState<SessionHistoryFilter>(initialSession&&sessions.some(s=>s.id===initialSession&&!isAgentSession(s))?'shells':'agents');
  const [query,setQuery]=useState(''),[selected,setSelected]=useState(initialSession);
  const rows=filterSessionHistory(sessions,filter,query),session=sessions.find(s=>s.id===selected);
  const agentCount=sessions.filter(isAgentSession).length;
  return <Modal open onOpenChange={open=>{if(!open)onClose()}} title="会话记录" className="session-history" closeLabel="关闭会话列表">
    <div className="session-history-summary">当前打开 {openCount} 个终端 · {sessions.length} 条历史记录</div>
    <div className="session-history-filters" role="group" aria-label="会话类型">
      {([['agents',`Agent 会话（${agentCount}）`],['shells',`终端记录（${sessions.length-agentCount}）`],['all',`全部（${sessions.length}）`]] as const).map(([id,label])=><Button key={id} aria-pressed={filter===id} onClick={()=>{setFilter(id);setSelected(undefined)}}>{label}</Button>)}
      <Input aria-label="搜索会话记录" placeholder="搜索名称或会话 ID" value={query} onChange={event=>setQuery(event.target.value)}/>
    </div>
    {error&&<p className="session-history-error" role="alert">{error}</p>}
    <div className={'session-history-body'+(session?' has-selection':'')}>
      <div className="wb-session-list session-history-list" aria-label="历史会话列表">
        {rows.map(s=><Button className="history-session-row" key={s.id} data-session-id={s.id} aria-pressed={selected===s.id} onClick={()=>setSelected(s.id)}><AgentProgramIcon program={s.program} size={18}/><span><strong>{terminalSessionTitle(s)}</strong><small>{terminalSessionStatus(s)} · #{s.id.slice(-8)}</small></span></Button>)}
        {!rows.length&&<p>{query?'没有匹配的会话记录。':filter==='agents'?'没有历史 Agent 会话。':'没有历史终端记录。'}</p>}
      </div>
      {session&&<section className="session-history-preview" aria-label="会话历史预览">
        <header><div><strong>{terminalSessionTitle(session)}</strong><small>{terminalSessionStatus(session)} · 只读记录</small></div><Button disabled={busy||(!terminalSessionIsLive(session)&&(!session.recoverable||!!session.task_id))} onClick={()=>onResume(session)}>{terminalSessionIsLive(session)?'打开终端':isAgentSession(session)?'继续会话':'重新启动终端'}</Button></header>
        <TerminalView key={session.id} historyOnly workspace={{projectId:workspace.project_id,workspaceId:workspace.id,id:session.id,program:session.program,status:session.status}}/>
      </section>}
    </div>
  </Modal>;
}
