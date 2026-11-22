import {ArrowUpRight,ChevronRight,FolderOpen,GitBranch,Globe,History,Plus,Terminal} from 'lucide-react';
import type {ReactNode} from 'react';
import {Button} from './ui';
import type {Session} from './workbench-types';
import ActionMenu from './ActionMenu';
import {AgentProgramIcon} from './AgentLogo';
import {terminalSessionTitle,terminalSessionStatus} from './terminal-execution';
import {isAgentSession} from './session-history';
import logo from './assets/velella-logo-v2.png';
import './workspace-overview.css';

export default function WorkspaceOverview({sessions,currentSessions=[],ready,busy,onTerminal,onCodexTerminal,onClaudeTerminal,onBrowser,onGit,onRestore,canRestore,nativeBlockedReason,onBrowse,onSessions,onSession,agentItems}:{
  name:string;files:string[];sessions:Session[];currentSessions?:Session[];ready:boolean;busy:boolean;
  onTerminal:()=>void;onCodexTerminal?:(event:{currentTarget:HTMLElement})=>void;onBrowse:()=>void;onFile:(path:string)=>void;
  onSession:(session:Session)=>void;onSessions:()=>void;onCreate?:()=>void;
  onClaudeTerminal?:()=>void;onBrowser?:()=>void;onGit?:()=>void;nativeBlockedReason?:string;
  onRestore?:()=>void;canRestore?:boolean;
  agentItems?:ReactNode;
}){
  const disabled=!ready||busy;
  const recentAgents=sessions.filter(isAgentSession).slice(-4).reverse();
  return <div className="project-empty-workspace workspace-start" aria-label="空工作区" data-open-terminals={currentSessions.length} data-history-count={sessions.length}><div className="workspace-start-content">
    <header className="workspace-start-heading"><img src={logo} width="36" height="36" alt=""/><h1>开始工作</h1><p>启动一个终端，或从历史记录中继续。</p><p>当前打开 {currentSessions.length} 个终端 · {sessions.length} 条历史记录</p></header>
    <div className="workspace-start-actions">
    <ActionMenu label="新建终端" disabled={disabled} align="start" icon={<><Plus size={14}/><span>新建终端</span></>}>
      {agentItems||<>{onCodexTerminal&&<Button disabled={disabled} onClick={onCodexTerminal}><Terminal size={15}/>新建 Codex 终端</Button>}
      {onClaudeTerminal&&<Button disabled={disabled||!!nativeBlockedReason} title={nativeBlockedReason} onClick={onClaudeTerminal}><Terminal size={15}/>新建 Claude 终端</Button>}
      <Button disabled={disabled} onClick={onTerminal}><Terminal size={15}/>新建终端</Button></>}
      <Button disabled={disabled} onClick={onSessions}><History size={15}/>历史会话（{sessions.length}）</Button>
      {canRestore&&<Button disabled={disabled} onClick={onRestore}><History size={15}/>恢复关闭的标签</Button>}
      <div className="wb-menu-divider"/>
      <Button disabled={disabled} onClick={onBrowse}><FolderOpen size={15}/>打开文件</Button>
      {onBrowser&&<Button disabled={disabled} onClick={onBrowser}>新建浏览器</Button>}
      {onGit&&<Button disabled={disabled} onClick={onGit}>Git 审查</Button>}
    </ActionMenu>
    <Button disabled={disabled} onClick={onSessions}><History size={16} aria-hidden="true"/>历史会话{ready&&<span className="workspace-start-count">{sessions.length}</span>}</Button>
    </div>
    <nav className="workspace-start-tools" aria-label="项目快捷工具">
      <Button disabled={disabled} onClick={onBrowse}><FolderOpen size={15} aria-hidden="true"/>打开文件</Button>
      {onBrowser&&<Button disabled={disabled} onClick={onBrowser}><Globe size={15} aria-hidden="true"/>新建浏览器</Button>}
      {onGit&&<Button disabled={disabled} onClick={onGit}><GitBranch size={15} aria-hidden="true"/>Git 审查</Button>}
      {canRestore&&<Button disabled={disabled} onClick={onRestore}><History size={15} aria-hidden="true"/>恢复关闭标签</Button>}
    </nav>
    <section className="workspace-start-sessions" aria-label="最近 Agent 会话">
      <div className="workspace-start-section-title"><h2>最近 Agent 会话</h2>{!!sessions.length&&<Button disabled={disabled} onClick={onSessions}>查看历史记录<ArrowUpRight size={13} aria-hidden="true"/></Button>}</div>
      {!ready?<p role="status">正在读取会话…</p>:recentAgents.length?<ul>{recentAgents.map(session=><li key={session.id}><Button className="workspace-start-session" disabled={disabled} onClick={()=>onSession(session)}><AgentProgramIcon program={session.program} size={16}/><strong>{terminalSessionTitle(session)}</strong><small>{terminalSessionStatus(session)}</small><ChevronRight size={14} aria-hidden="true"/></Button></li>)}</ul>:<p>暂无历史 Agent 会话。普通终端的记录可在“历史会话”中查看。</p>}
    </section>
  </div></div>;
}
