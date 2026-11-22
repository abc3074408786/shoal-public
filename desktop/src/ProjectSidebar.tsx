import {useId,useState} from 'react';
import {Bell,ChevronDown,ChevronRight,CircleX,FolderInput,FolderPlus,FolderTree,Home,MoreHorizontal,Plus,Search,Settings2,Shapes,Trash2,X} from 'lucide-react';
import * as ContextMenu from '@radix-ui/react-context-menu';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import type {Project,Task} from './api';
import type {Workspace} from './workbench-types';
import {Button,Input,Modal} from './ui';
import ActionMenu from './ActionMenu';
import {useConfirm} from './ConfirmAction';
import {projectIconNames,defaultWorkspaceView} from './project-library';
import type {ProjectLibraryController} from './useProjectLibrary';
import ProjectIcon,{projectIconLabels} from './ProjectIcon';
import SidebarWorkspaceRow,{type WorkspaceRowActions} from './SidebarWorkspaceRow';
import {workspaceTreeRows} from './workspace-context';
import WorkspaceOptions from './WorkspaceOptions';
import {compareWorkspaceView,sortViewProjects,viewWorkspaces,workspaceViewGroups} from './workspace-view';

type Props={projects:Project[];workspaces:Workspace[];tasks?:Task[];workspace?:Workspace;disabled:boolean;library:ProjectLibraryController;hostLabel:string;remote:boolean;workspaceActions:WorkspaceRowActions;onHosts?:()=>void;onProject:(project:Project)=>void;onWorkspace:(workspace:Workspace)=>void;onAdd:()=>void;onHome:()=>void;onSettings:()=>void;onContents:(element:HTMLDivElement|null)=>void;onNewWorkspace:(project:Project)=>void;onProjectSettings:(project:Project)=>void;onRemove:(project:Project)=>void};
type Editor={kind:'icon';project:Project}|{kind:'group';project?:Project;groupId?:string};

/** The header dropdown and right-click alias share exactly the same actions. */
function ProjectActions({context=false,project,disabled,library,onSettings,onIcon,onGroup,onRemove}:{context?:boolean;project:Project;disabled:boolean;library:ProjectLibraryController;onSettings:()=>void;onIcon:()=>void;onGroup:()=>void;onRemove:()=>void}){
  const Menu=context?ContextMenu:DropdownMenu,group=library.state.projects[project.id]?.group;
  return <>
    <Menu.Item className="ui-button" disabled={disabled} onSelect={onSettings}><Settings2 size={14}/>项目设置</Menu.Item>
    <Menu.Item className="ui-button" onSelect={onIcon}><Shapes size={14}/>更改项目图标</Menu.Item>
    <Menu.Item className="ui-button" onSelect={onGroup}><FolderTree size={14}/>从此项目新建分组</Menu.Item>
    {!!library.state.groups.length&&<Menu.Sub><Menu.SubTrigger className="ui-button"><FolderInput size={14}/>移至分组<ChevronRight className="project-menu-arrow" size={13}/></Menu.SubTrigger><Menu.Portal><Menu.SubContent className="wb-menu-panel project-action-menu" collisionPadding={8}>{library.state.groups.map(g=><Menu.Item className="ui-button" key={g.id} disabled={g.id===group} onSelect={()=>library.edit(project.id,{group:g.id})}>{g.name}</Menu.Item>)}</Menu.SubContent></Menu.Portal></Menu.Sub>}
    {group&&<Menu.Item className="ui-button" onSelect={()=>library.edit(project.id,{group:undefined})}><CircleX size={14}/>移出分组</Menu.Item>}
    <Menu.Separator className="wb-menu-divider"/>
    <Menu.Item className="ui-button project-remove-action" onSelect={onRemove}><Trash2 size={14}/>移除项目</Menu.Item>
  </>;
}

export default function ProjectSidebar({projects,workspaces,tasks=[],workspace,disabled,library,hostLabel,remote,workspaceActions,onHosts,onProject,onAdd,onHome,onSettings,onNewWorkspace,onProjectSettings,onRemove}:Props){
  const ask=useConfirm();
  const categoryId=useId(),projectHeadingId=categoryId+'-projects-heading',projectPanelId=categoryId+'-projects-panel';
  const [search,setSearch]=useState(''),[collapsed,setCollapsed]=useState<Set<string>>(new Set()),[collapsedGroups,setCollapsedGroups]=useState<Set<string>>(new Set());
  const [editor,setEditor]=useState<Editor|null>(null),[groupName,setGroupName]=useState('');
  const view=library.state.view||defaultWorkspaceView(),filtered=viewWorkspaces(workspaces,projects,library.state,view),compare=compareWorkspaceView(view.workspaceSort);
  const query=search.trim().toLocaleLowerCase(),matches=(value:string)=>value.toLocaleLowerCase().includes(query);
  const projectMatches=(p:Project)=>!query||matches(p.name)||matches(p.path)||matches(library.state.groups.find(g=>g.id===library.state.projects[p.id]?.group)?.name||'');
  const visible=sortViewProjects(projects.filter(p=>(!view.project||p.id===view.project)&&(projectMatches(p)||filtered.some(w=>w.project_id===p.id&&(matches(w.name)||matches(w.path))))),view.projectSort);
  const flat=filtered.filter(w=>{const p=projects.find(p=>p.id===w.project_id);return p&&(projectMatches(p)||matches(w.name)||matches(w.path))}).sort(compare);
  const displayGroups=view.groupBy==='project'?[]:workspaceViewGroups(flat,view,library.state,tasks);
  const activeProject=projects.find(p=>p.id===workspace?.project_id);
  const unreadWorkspaces=workspaces.filter(w=>!w.archived&&library.state.workspaces?.[w.id]?.unread&&projects.some(p=>p.id===w.project_id));
  function toggle(id:string){setCollapsed(current=>{const next=new Set(current);next.has(id)?next.delete(id):next.add(id);return next})}
  function open(p:Project){setCollapsed(current=>{const next=new Set(current);next.delete(p.id);return next});onProject(p)}
  function groupEditor(project?:Project,groupId?:string){setGroupName(groupId?library.state.groups.find(g=>g.id===groupId)?.name||'':project?.name||'');setEditor({kind:'group',project,groupId})}
  function closeMenuFocus(e:Event){if(document.querySelector('[role=dialog],dialog[open]'))e.preventDefault()}
  function renderWorkspace(w:Workspace,depth=0){const p=projects.find(p=>p.id===w.project_id);return p?<div role="listitem" key={w.id}><SidebarWorkspaceRow workspace={w} project={p} workspaces={workspaces} active={w.id===workspace?.id} disabled={disabled} depth={depth} hostLabel={hostLabel} remote={remote} library={library} actions={workspaceActions} onCreateGroup={()=>groupEditor(p)} onRemoveProject={()=>onRemove(p)}/></div>:null}
  function renderProject(p:Project){
    const active=p.id===workspace?.project_id,expanded=!!query||!collapsed.has(p.id),choices=workspaceTreeRows(filtered,{projectId:p.id,projectName:p.name,search:projectMatches(p)?'':search,includeArchived:false,compare});
    const actions={project:p,disabled,library,onSettings:()=>onProjectSettings(p),onIcon:()=>setEditor({kind:'icon',project:p}),onGroup:()=>groupEditor(p),onRemove:()=>onRemove(p)};
    return <section className={'sidebar-project'+(active?' current-project':'')} key={p.id} aria-label={p.name}>
      <ContextMenu.Root><ContextMenu.Trigger asChild><div className="project-sidebar-row">
        <Button className="project-sidebar-disclosure" aria-label={(expanded?'收起':'展开')+p.name} aria-expanded={expanded} onClick={()=>toggle(p.id)}><span className="project-disclosure-arrow">{expanded?<ChevronDown size={12}/>:<ChevronRight size={12}/>}</span></Button>
        <Button className={'sidebar-project-name'+(active?' selected':'')} aria-current={active?'page':undefined} title={p.path} onClick={()=>open(p)}><ProjectIcon name={library.state.projects[p.id]?.icon} size={15}/><span>{p.name}</span></Button>
        <Button className="project-sidebar-action" aria-label={'在 '+p.name+' 中新建工作区'} title="新建工作区" disabled={disabled} onClick={()=>onNewWorkspace(p)}><Plus size={14}/></Button>
        <DropdownMenu.Root modal={false}><DropdownMenu.Trigger asChild><Button className="project-sidebar-action" aria-label={p.name+' 项目操作'} title="项目操作"><MoreHorizontal size={15}/></Button></DropdownMenu.Trigger><DropdownMenu.Portal><DropdownMenu.Content className="wb-menu-panel project-action-menu" aria-label={p.name+' 项目菜单'} align="end" sideOffset={6} collisionPadding={8} onCloseAutoFocus={closeMenuFocus}><ProjectActions {...actions}/></DropdownMenu.Content></DropdownMenu.Portal></DropdownMenu.Root>
      </div></ContextMenu.Trigger><ContextMenu.Portal><ContextMenu.Content className="wb-menu-panel project-action-menu" aria-label={p.name+' 项目菜单'} collisionPadding={8} onCloseAutoFocus={closeMenuFocus}><ProjectActions context {...actions}/></ContextMenu.Content></ContextMenu.Portal></ContextMenu.Root>
      {expanded&&<div className="sidebar-project-workspaces" role="list" aria-label={p.name+' 的工作区'}>{choices.map(({workspace:w,depth})=>renderWorkspace(w,depth))}{!choices.length&&<div role="listitem" className="project-workspace-empty">{query||view.hideSleeping?'没有符合条件的工作区':'还没有工作区'}</div>}</div>}
    </section>;
  }
  const projectTools=<div className="project-sidebar-tools" role="group" aria-label="项目工具">
        <DropdownMenu.Root modal={false}><DropdownMenu.Trigger asChild>
          <Button className="project-sidebar-unread" type="button" aria-label={'未读工作区，'+unreadWorkspaces.length+' 个'} title="未读工作区"><Bell size={14}/><span className="project-sidebar-unread-count" aria-hidden="true">{unreadWorkspaces.length>99?'99+':unreadWorkspaces.length}</span></Button>
        </DropdownMenu.Trigger><DropdownMenu.Portal><DropdownMenu.Content className="wb-menu-panel project-action-menu project-toolbar-menu" aria-label="未读工作区" align="end" sideOffset={8} collisionPadding={8}>
          <DropdownMenu.Label className="wb-menu-label">未读工作区</DropdownMenu.Label>
          {unreadWorkspaces.length?unreadWorkspaces.map(w=><DropdownMenu.Item key={w.id} className="ui-button project-unread-item" disabled={disabled} onSelect={()=>workspaceActions.onOpen(w)}><span>{w.name}</span><small>{projects.find(p=>p.id===w.project_id)?.name}</small></DropdownMenu.Item>):<div className="project-toolbar-empty" role="status">暂无未读工作区</div>}
        </DropdownMenu.Content></DropdownMenu.Portal></DropdownMenu.Root>
        <WorkspaceOptions view={view} projects={projects} hostLabel={hostLabel} disabled={disabled} onChange={library.editView} onHosts={onHosts} onExpand={()=>{setCollapsed(new Set());setCollapsedGroups(new Set())}} onCollapse={()=>{setCollapsed(new Set(projects.map(p=>p.id)));setCollapsedGroups(new Set([...library.state.groups.map(g=>g.id),...displayGroups.map(g=>'view:'+g.id)]))}} canCollapse={!query&&view.groupBy!=='none'}/>
        <Button type="button" aria-label="添加项目" title="添加项目" disabled={disabled} onClick={onAdd}><FolderPlus size={16}/></Button>
        <Button type="button" aria-label="新建工作区" title={activeProject?'在 '+activeProject.name+' 中新建工作区':'请先选择项目'} disabled={disabled||!activeProject} onClick={()=>{if(activeProject)onNewWorkspace(activeProject)}}><Plus size={16}/></Button>
      </div>;
  return <>
    <div className="project-sidebar-heading">
      <strong id={projectHeadingId}>项目</strong>
    </div>
    <section className="project-sidebar-utilities" aria-label="项目搜索与工具">
      <label className="project-sidebar-search"><Search size={14} aria-hidden="true"/><Input aria-label="搜索项目" placeholder="搜索项目" value={search} onChange={e=>setSearch(e.target.value)}/>{search&&<Button aria-label="清除项目搜索" onClick={()=>setSearch('')}><X size={13}/></Button>}</label>
      {projectTools}
    </section>
    <div className="project-sidebar-panel" role="region" id={projectPanelId} aria-labelledby={projectHeadingId} tabIndex={0}>
    {(view.project||view.hideSleeping)&&<div className="workspace-filter-summary"><span>{[view.project?(projects.find(p=>p.id===view.project)?.name||'项目已不可用'):'',view.hideSleeping?'隐藏休眠':''].filter(Boolean).join(' · ')}</span><Button aria-label="清除工作区筛选" onClick={()=>library.editView({project:'',hideSleeping:false})}><X size={12}/></Button></div>}
    <nav className="sidebar-projects project-sidebar-list workspace-navigation" aria-label="项目列表">
      {view.groupBy==='project'?<>{visible.filter(p=>!library.state.projects[p.id]?.group).map(renderProject)}
      {library.state.groups.map(g=>{
        const members=visible.filter(p=>library.state.projects[p.id]?.group===g.id),expanded=!!query||!collapsedGroups.has(g.id);if(!members.length)return null;
        return <section className="project-sidebar-group" key={g.id} aria-label={'项目分组 '+g.name}><div className="project-group-heading"><Button aria-label={(expanded?'收起':'展开')+'分组 '+g.name} aria-expanded={expanded} onClick={()=>setCollapsedGroups(current=>{const next=new Set(current);next.has(g.id)?next.delete(g.id):next.add(g.id);return next})}>{expanded?<ChevronDown size={12}/>:<ChevronRight size={12}/>}<span>{g.name}</span><small>{members.length}</small></Button><ActionMenu label={g.name+' 分组操作'} icon={<MoreHorizontal size={14}/>}><Button onClick={()=>groupEditor(undefined,g.id)}>重命名分组</Button><Button onClick={()=>void (async()=>{if(await ask('删除分组“'+g.name+'”？其中的项目将移出分组，项目和会话记录保留。'))library.deleteGroup(g.id)})()}>删除分组</Button></ActionMenu></div>{expanded&&members.map(renderProject)}</section>;
      })}</>:displayGroups.map(g=>{const id='view:'+g.id,expanded=!!query||!collapsedGroups.has(id);return <section key={id} className="workspace-view-group" aria-label={g.label}>{view.groupBy!=='none'&&<div className="project-group-heading"><Button aria-expanded={expanded} onClick={()=>setCollapsedGroups(current=>{const next=new Set(current);next.has(id)?next.delete(id):next.add(id);return next})}>{expanded?<ChevronDown size={12}/>:<ChevronRight size={12}/>}<span>{g.label}</span><small>{g.rows.length}</small></Button></div>}{(expanded||view.groupBy==='none')&&<div role="list" aria-label={g.label}>{g.rows.map(w=>renderWorkspace(w))}</div>}</section>})}
      {(!visible.length||(view.groupBy!=='project'&&!flat.length))&&<p className="sidebar-content-empty" role="status">{query||view.project||view.hideSleeping?'没有符合条件的工作区':projects.length?'还没有工作区':'尚未添加项目'}</p>}
    </nav>
    </div>
    <div className="project-sidebar-footer"><Button aria-label="返回项目首页" title="返回项目首页" onClick={onHome}><Home size={16}/><span>项目首页</span></Button><Button aria-label="设置" title="设置 · Ctrl+," onClick={onSettings}><Settings2 size={16}/></Button></div>
    <Modal open={editor!==null} onOpenChange={open=>{if(!open)setEditor(null)}} title={editor?.kind==='icon'?'更改项目图标 · '+editor.project.name:editor?.kind==='group'&&editor.groupId?'重命名分组':'新建项目分组'}>
      {editor?.kind==='icon'?<div className="project-icon-options" aria-label="项目图标">{projectIconNames.map(name=><Button key={name} aria-label={projectIconLabels[name]} aria-pressed={(library.state.projects[editor.project.id]?.icon||'folder')===name} onClick={()=>{if(library.edit(editor.project.id,{icon:name}))setEditor(null)}}><ProjectIcon name={name} size={22}/><span>{projectIconLabels[name]}</span></Button>)}</div>:editor?.kind==='group'?<form onSubmit={e=>{e.preventDefault();const name=groupName.trim();if(!name)return;const saved=editor.groupId?library.renameGroup(editor.groupId,name):editor.project?library.createGroup(name,editor.project.id):false;if(saved)setEditor(null)}}><div className="dialog-body"><label>分组名称<Input autoFocus required maxLength={64} value={groupName} onChange={e=>setGroupName(e.target.value)}/></label></div><div className="dialog-footer"><Button type="button" onClick={()=>setEditor(null)}>取消</Button><Button type="submit" className="primary" disabled={!groupName.trim()}>{editor.groupId?'保存':'创建分组'}</Button></div></form>:null}
    </Modal>
  </>;
}
