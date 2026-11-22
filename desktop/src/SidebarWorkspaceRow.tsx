import {useEffect,useState} from 'react';
import * as Menu from '@radix-ui/react-context-menu';
import {Bell,BellOff,Check,ChevronRight,CircleX,Copy,FolderInput,FolderOpen,FolderTree,Monitor,Moon,Pencil,Pin,PinOff,Trash2,Unlink,Archive} from 'lucide-react';
import type {Project} from './api';
import {rpc,isTauri} from './api';
import {currentTarget,sameTarget} from './execution-target';
import type {Workspace} from './workbench-types';
import {workspaceIsRoot} from './workspace-context';
import {workspaceStatuses,defaultWorkspaceView,type WorkspaceProperty} from './project-library';
import type {ProjectLibraryController} from './useProjectLibrary';
import {Button} from './ui';
import {copyText} from './text-clipboard';

export type WorkspaceRowActions={onOpen:(w:Workspace)=>void;onEdit:(w:Workspace,mode:'rename'|'parent',name?:string)=>void;onUpdate:(w:Workspace,fields:Record<string,unknown>)=>void;onSleep:(w:Workspace)=>void;onArchive:(w:Workspace)=>void;onFolder:(w:Workspace)=>void;onError:(message:string)=>void};
export default function SidebarWorkspaceRow({workspace:w,project,workspaces,active,disabled,depth=0,hostLabel,remote,library,onCreateGroup,onRemoveProject,actions}:{workspace:Workspace;project:Project;workspaces:Workspace[];active:boolean;disabled:boolean;depth?:number;hostLabel:string;remote:boolean;library:ProjectLibraryController;onCreateGroup:()=>void;onRemoveProject:()=>void;actions:WorkspaceRowActions}){
  const [branch,setBranch]=useState('');
  useEffect(()=>{if(w.archived||w.kind==='folder')return;let live=true;const target=currentTarget();void rpc({cmd:'workbench.git',workspace_id:w.id},target).then(r=>{if(live&&sameTarget(target,currentTarget()))setBranch(r.branch?.trim()||'')}).catch(()=>{});return()=>{live=false}},[w.id,w.path,w.kind,w.archived]);
  const root=workspaceIsRoot(w),meta=library.state.workspaces?.[w.id]||{},status=meta.status||'todo';
  const view=library.state.view||defaultWorkspaceView(),shows=(property:WorkspaceProperty)=>view.properties.includes(property);
  const generatedName=['默认工作区','主工作区'].includes(w.name)||(root&&w.name===project.name&&w.kind!=='folder');
  const name=generatedName?(root?'主工作区':w.name):w.name;
  const indent=4+Math.min(36,Math.max(0,depth)*12);
  const group=library.state.projects[project.id]?.group,parent=workspaces.find(p=>p.id===w.parent_workspace_id);
  function open(){actions.onOpen(w)}
  async function copy(text:string){try{await copyText(text)}catch(e){actions.onError(e instanceof Error?e.message:String(e))}}
  return <Menu.Root><Menu.Trigger asChild><Button className={'sidebar-workspace-card'+(active?' selected':'')+(meta.unread&&shows('unread')?' unread':'')} data-card-layout={view.layout} aria-current={active?'page':undefined} aria-label={'打开工作区 '+name} aria-description={'第 '+(depth+1)+' 层工作区'} title={[w.path,branch].filter(Boolean).join('\n')} style={{marginInlineStart:indent,inlineSize:`calc(100% - ${indent}px)`}} onClick={open}>
    {(shows('status')||shows('unread'))&&<span className="workspace-status-slot" data-unread={shows('unread')&&meta.unread||undefined}>{shows('status')&&<span className={'workspace-state-dot '+(meta.sleeping?'sleeping':status)} title={meta.sleeping?'已休眠':workspaceStatuses.find(s=>s.id===status)?.label}/>}</span>}
    <span className="sidebar-workspace-summary"><span className="sidebar-workspace-title"><strong>{name}</strong>{root&&shows('primary')&&<small className="workspace-primary-badge">主要</small>}{w.favorite&&shows('favorite')&&<Pin size={11} aria-label="已固定"/>}</span><span className="sidebar-workspace-detail">{shows('host')&&<span className="workspace-host-badge">{hostLabel}</span>}{shows('sleeping')&&meta.sleeping&&<span>已休眠</span>}{shows('branch')&&branch&&name!==branch&&<span>{branch}</span>}{shows('project')&&<span>{project.name}</span>}{shows('path')&&<span className="workspace-directory" title={w.path}>{w.path}</span>}</span></span>
  </Button></Menu.Trigger><Menu.Portal><Menu.Content className="wb-menu-panel project-action-menu workspace-action-menu" aria-label={name+' 工作区操作'} collisionPadding={8} onCloseAutoFocus={e=>{if(document.querySelector('dialog[open],[role=dialog]'))e.preventDefault()}}>
    <Menu.Label className="wb-menu-label">工作区</Menu.Label>
    <Menu.Item className="ui-button" disabled={disabled} onSelect={()=>actions.onEdit(w,'rename',name)}><Pencil size={14}/>更新</Menu.Item>
    <Menu.Sub><Menu.SubTrigger className="ui-button" disabled={disabled}><span className={'workspace-state-dot '+status}/>移至状态<ChevronRight className="project-menu-arrow" size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu" collisionPadding={8}><Menu.RadioGroup value={status} onValueChange={value=>library.editWorkspace(w.id,{status:value as typeof status})}>{workspaceStatuses.map(s=><Menu.RadioItem className="ui-button workspace-status-item" key={s.id} value={s.id}><Menu.ItemIndicator><Check size={13}/></Menu.ItemIndicator><span className={'workspace-state-dot '+s.id}/>{s.label}</Menu.RadioItem>)}</Menu.RadioGroup></Menu.SubContent></Menu.Portal></Menu.Sub>
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Sub><Menu.SubTrigger className="ui-button" disabled={disabled}><FolderOpen size={14}/>打开于<ChevronRight className="project-menu-arrow" size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu" collisionPadding={8}><Menu.Item className="ui-button" onSelect={open}><Monitor size={14}/>鱼群工作区</Menu.Item><Menu.Item className="ui-button" disabled={remote||!isTauri()} title={remote?'远端路径请在远端主机打开':undefined} onSelect={()=>actions.onFolder(w)}><FolderOpen size={14}/>文件资源管理器</Menu.Item></Menu.SubContent></Menu.Portal></Menu.Sub>
    <Menu.Item className="ui-button" onSelect={()=>void copy(w.path)}><Copy size={14}/>复制路径</Menu.Item>
    <Menu.Item className="ui-button" onSelect={()=>void copy(name)}><Copy size={14}/>复制工作树名称</Menu.Item>
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Item className="ui-button" disabled={disabled} onSelect={()=>actions.onUpdate(w,{favorite:!w.favorite})}>{w.favorite?<PinOff size={14}/>:<Pin size={14}/>} {w.favorite?'取消固定':'固定'}</Menu.Item>
    <Menu.Item className="ui-button" disabled={disabled} onSelect={()=>library.editWorkspace(w.id,{unread:!meta.unread})}>{meta.unread?<BellOff size={14}/>:<Bell size={14}/>} {meta.unread?'标记为已读':'标记为未读'}</Menu.Item>
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Item className="ui-button" disabled={disabled} onSelect={onCreateGroup}><FolderTree size={14}/>来自项目的新组</Menu.Item>
    {!!library.state.groups.length&&<Menu.Sub><Menu.SubTrigger className="ui-button" disabled={disabled}><FolderInput size={14}/>移至群组<ChevronRight className="project-menu-arrow" size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu" collisionPadding={8}>{library.state.groups.map(g=><Menu.Item key={g.id} className="ui-button" disabled={g.id===group} onSelect={()=>library.edit(project.id,{group:g.id})}>{g.name}</Menu.Item>)}</Menu.SubContent></Menu.Portal></Menu.Sub>}
    {group&&<Menu.Item className="ui-button" disabled={disabled} onSelect={()=>library.edit(project.id,{group:undefined})}><CircleX size={14}/>移出群组</Menu.Item>}
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Item className="ui-button" disabled={disabled||root} title={root?'主工作区固定为顶级':undefined} onSelect={()=>actions.onEdit(w,'parent')}><FolderTree size={14}/>{w.parent_workspace_id?'更改父工作树…':'设置父工作树…'}</Menu.Item>
    {parent&&<Menu.Item className="ui-button" disabled={disabled} onSelect={()=>actions.onOpen(parent)}><FolderInput size={14}/>打开父工作树</Menu.Item>}
    {w.parent_workspace_id&&<Menu.Item className="ui-button" disabled={disabled} onSelect={()=>actions.onUpdate(w,{parent_workspace_id:null})}><Unlink size={14}/>移出父工作树</Menu.Item>}
    <Menu.Item className="ui-button" disabled={disabled} onSelect={()=>meta.sleeping?open():actions.onSleep(w)}><Moon size={14}/>{meta.sleeping?'唤醒工作区':'Sleep'}</Menu.Item>
    <Menu.Item className="ui-button project-remove-action" disabled title={root?'主工作树不能删除；可从鱼群中移除项目':'暂不支持删除此工作树，可使用归档工作区'}><Trash2 size={14}/>删除工作树</Menu.Item>
    {!root&&<Menu.Item className="ui-button" disabled={disabled} onSelect={()=>actions.onArchive(w)}><Archive size={14}/>归档工作区</Menu.Item>}
    {root&&<Menu.Item className="ui-button project-remove-action" disabled={disabled} onSelect={onRemoveProject}><Trash2 size={14}/>从鱼群中移除项目</Menu.Item>}
  </Menu.Content></Menu.Portal></Menu.Root>;
}
