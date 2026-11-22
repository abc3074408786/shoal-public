import {useState} from 'react';
import {ExternalLink} from 'lucide-react';
import {phases,type Task} from './api';
import {Button} from './ui';
import {AgentProgramIcon} from './AgentLogo';
import {terminalSessionTitle,terminalSessionStatus} from './terminal-execution';
import type {Session} from './workbench-types';
import TerminalActivity from './TerminalActivity';
import TaskProgress from './TaskProgress';
import SwarmProgress from './SwarmProgress';
import './cli-task-monitor.css';

function sessionState(session:Session){
  if(session.status?.exit!=null||!session.status)return terminalSessionStatus(session);
  if(session.status.human_control)return '人工接管中';
  if(session.status.state==='waiting_permission')return '等待授权';
  if(session.status.state==='waiting_input')return '等待输入';
  return terminalSessionStatus(session);
}

export default function CliTaskMonitor({workspaceId,sessions,tasks,activeSessionId,onOpen,disabled=false}:{workspaceId:string;sessions:Session[];tasks:Task[];activeSessionId?:string;onOpen:(session:Session)=>void;disabled?:boolean}){
  const [chosen,setChosen]=useState<string>();
  const [showActivity,setShowActivity]=useState(false);
  const available=sessions.filter(session=>!session.native_chat);
  const selected=available.find(session=>session.id===(chosen||activeSessionId))||available.find(session=>session.id===activeSessionId)||available[0];
  const task=selected?tasks.find(task=>task.id===selected.task_id||task.agent?.id===selected.id):undefined;
  return <section className="cli-task-monitor" aria-label="CLI 任务监控">
    <header className="cli-monitor-heading"><strong>CLI 任务</strong><span>{available.length} 个会话</span></header>
    {!available.length&&<p className="cli-monitor-note">当前工作区还没有 CLI 会话。从“新建标签”启动后，可在这里查看状态和操作记录。</p>}
    <div className="cli-monitor-sessions" aria-label="监控会话">
      {available.map(session=><Button key={session.id} className="cli-monitor-session" aria-pressed={selected?.id===session.id} onClick={()=>setChosen(session.id)}>
        <AgentProgramIcon program={session.program} size={16}/><span className="cli-monitor-session-name">{terminalSessionTitle(session)}</span><small data-waiting={['waiting_permission','waiting_input'].includes(session.status?.state||'')}>{sessionState(session)}</small>
      </Button>)}
    </div>
    {selected&&<>
      <div className="cli-monitor-current">
        <div><strong>{task?.spec.name||terminalSessionTitle(selected)}</strong><Button disabled={disabled} onClick={()=>onOpen(selected)}><ExternalLink size={13}/>打开会话</Button></div>
        {task&&<p><span>{phases[task.phase]||task.phase}</span>{task.reason&&<> · {task.reason}</>}</p>}
      </div>
      <TaskProgress key={selected.id} workspaceId={workspaceId} sessionId={selected.id} nativeId={selected.native_id} taskName={task?.spec.name}/>
      {['codex','claude'].includes(selected.program||'')&&<details className="cli-monitor-activity" onToggle={event=>setShowActivity(event.currentTarget.open)}><summary>工具操作记录</summary>{showActivity&&<TerminalActivity key={selected.id} workspaceId={workspaceId} sessionId={selected.id} visible exited={selected.status?.exit!=null} variant="sidebar"/>}</details>}
    </>}
    <SwarmProgress workspaceId={workspaceId}/>
  </section>;
}
