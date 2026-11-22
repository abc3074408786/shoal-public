import type {ReactElement} from 'react';
import * as ContextMenu from '@radix-ui/react-context-menu';
import {Play,RefreshCw,Square,X} from 'lucide-react';
import type {Session} from './workbench-types';
import {terminalSessionTitle,terminalSessionStatus} from './terminal-execution';

/** Uses the same live session actions as the terminal; opening a menu is read-only. */
export default function SidebarSessionMenu({session,disabled,onOpen,onClose,onRecover,onStop,children}:{session:Session;disabled:boolean;onOpen:()=>void;onClose?:()=>void;onRecover:()=>void;onStop:()=>void;children:ReactElement}){
  const running=session.status?.exit===null&&session.status.state!=='host_lost';
  return <ContextMenu.Root><ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger><ContextMenu.Portal><ContextMenu.Content className="wb-menu-panel project-action-menu" aria-label={terminalSessionTitle(session)+' 会话操作'} collisionPadding={8} onCloseAutoFocus={e=>{if(document.querySelector('dialog[open],[role=dialog]'))e.preventDefault()}}>
    <ContextMenu.Label className="wb-menu-label">{terminalSessionTitle(session)} · {terminalSessionStatus(session)}</ContextMenu.Label>
    <ContextMenu.Item className="ui-button" disabled={disabled} onSelect={onOpen}><Play size={14}/>打开会话</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" disabled={disabled||!onClose} onSelect={onClose}><X size={14}/>关闭终端（停止进程）</ContextMenu.Item>
    {(session.recoverable||running)&&<ContextMenu.Separator className="wb-menu-divider"/>}
    {session.recoverable&&<ContextMenu.Item className="ui-button" disabled={disabled} onSelect={onRecover}><RefreshCw size={14}/>重新启动终端</ContextMenu.Item>}
    {running&&<ContextMenu.Item className="ui-button project-remove-action" disabled={disabled} onSelect={onStop}><Square size={14}/>停止进程</ContextMenu.Item>}
  </ContextMenu.Content></ContextMenu.Portal></ContextMenu.Root>;
}
