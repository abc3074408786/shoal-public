import type {MouseEvent} from 'react';
import {ChevronRight,Settings2,Terminal} from 'lucide-react';
import * as Menu from '@radix-ui/react-dropdown-menu';
import {Button} from './ui';
import {defaultAgentLaunch,enabledAgents,type AgentId} from './agent-preferences';
import {useAgentPreferences} from './useAgentPreferences';
import AgentLogo from './AgentLogo';
export function openAppSettings(){window.dispatchEvent(new Event('velella:open-settings'))}
export default function AgentLaunchItems({onLaunch,onConfigure,blockedReason}:{onLaunch:(id:AgentId|'terminal',event:MouseEvent<HTMLButtonElement>)=>void;onConfigure?:(id:AgentId,event:MouseEvent<HTMLButtonElement>)=>void;blockedReason?:string}){
  const {preferences}=useAgentPreferences(),preferred=defaultAgentLaunch(preferences);
  const shell=<Menu.Item asChild key="terminal"><Button onClick={event=>onLaunch('terminal',event)}><Terminal size={15}/>新建终端{preferred==='terminal'&&<small aria-hidden="true">默认</small>}</Button></Menu.Item>;
  return <>{preferred==='terminal'&&shell}{enabledAgents(preferences).map(agent=><Menu.Item asChild key={agent.id} disabled={!!blockedReason}><Button disabled={!!blockedReason} title={blockedReason||undefined} aria-label={`新建 ${agent.name==='Claude Code'?'Claude':agent.name} 终端`} onClick={event=>onLaunch(agent.id,event)}><AgentLogo agent={agent.id} size={16}/>新建 {agent.name==='Claude Code'?'Claude':agent.name} 终端{preferred===agent.id&&<small aria-hidden="true">默认</small>}</Button></Menu.Item>)}{preferred!=='terminal'&&shell}
    {onConfigure&&<Menu.Sub><Menu.SubTrigger asChild><Button><Settings2 size={15}/>启动选项…<ChevronRight size={14}/></Button></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel" sideOffset={4} collisionPadding={8}>{enabledAgents(preferences).map(agent=><Menu.Item asChild key={agent.id} disabled={agent.id!=='codex'&&!!blockedReason}><Button disabled={agent.id!=='codex'&&!!blockedReason} onClick={event=>onConfigure(agent.id,event)}><AgentLogo agent={agent.id} size={16}/>{agent.name==='Claude Code'?'Claude':agent.name} 启动选项…</Button></Menu.Item>)}</Menu.SubContent></Menu.Portal></Menu.Sub>}
    <Menu.Separator className="wb-menu-divider"/><Menu.Item asChild><Button onClick={openAppSettings}><Settings2 size={15}/>Agent 设置…</Button></Menu.Item></>;
}
