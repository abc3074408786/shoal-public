import {useEffect,useId,useState,type ReactNode} from 'react';
import {Folder,ChevronDown,ChevronUp} from 'lucide-react';
import type {ProjectSurfaceContext} from '../src/WorkspaceApp';
import type {Project} from '../src/api';
import type {Workspace} from '../src/workbench-types';
import {workspaceIsRoot} from '../src/workspace-context';
import {usePreference} from '../src/Appearance';
import Workbench from '../src/Workbench';
import {ChromeTarget,WorkspaceToolsTarget,ProjectContentsTarget} from '../src/WindowChrome';
import ActionMenu from '../src/ActionMenu';
import {Button} from '../src/ui';
import './project-layout-options.css';

type LayoutMode='tabs'|'columns';

// Only loaded by the isolated browser fixture, never by the installed application.
export default function ProjectLayoutOptions(context:ProjectSurfaceContext){
  const [mode,setMode]=useState<LayoutMode>(()=>new URLSearchParams(location.search).get('layout')==='columns'?'columns':'tabs');
  const [focused,setFocused]=useState(context.workspace.project_id);
  const [projectBarOpen,setProjectBarOpen]=usePreference('velella:ui:project-bar-open',false);
  const projectBarId=useId();
  const {projects,workspaces,workspace,onWorkspace}=context;
  const available=projects.flatMap(project=>{
    const candidates=workspaces.filter(w=>w.project_id===project.id&&!w.archived);
    const selected=candidates.find(w=>w.id===workspace.id)||candidates.find(workspaceIsRoot)||candidates[0];
    return selected?[{project,workspace:selected}]:[];
  });
  useEffect(()=>setFocused(workspace.project_id),[workspace.project_id]);
  function chooseMode(next:LayoutMode){
    setMode(next);
    if(next==='tabs'){const current=available.find(({project})=>project.id===focused);if(current)onWorkspace(current.workspace)}
    const url=new URL(location.href);url.searchParams.set('layout',next);history.replaceState(null,'',url);
  }
  function select(project:Project,w:Workspace){setFocused(project.id);onWorkspace(w)}
  return <div className="project-layout-options" data-layout={mode}>
    {mode==='columns'&&<div className="layout-comparison-bar"><div role="group" aria-label="布局方案">
      <Button aria-pressed={false} onClick={()=>chooseMode('tabs')}>方案一 · 顶部切换</Button>
      <Button aria-pressed={mode==='columns'} onClick={()=>chooseMode('columns')}>方案二 · 项目分栏</Button>
    </div><small>界面预览</small></div>}
    {mode==='tabs'&&<nav id={projectBarId} hidden={!projectBarOpen} className="layout-project-switcher" aria-label="中间项目切换">{available.map(({project,workspace:w})=><Button key={project.id} aria-current={project.id===workspace.project_id?'page':undefined} title={project.path} onClick={()=>select(project,w)}><Folder size={14}/><span>{project.name}</span></Button>)}</nav>}
    <div className="layout-project-board">
      {available.filter(({project})=>mode==='columns'||project.id===workspace.project_id).map(({project,workspace:w})=><ProjectColumn key={project.id} context={context} project={project} workspace={w} columns={mode==='columns'} focused={focused===project.id} onFocus={()=>{setFocused(project.id)}} projectToggle={mode==='tabs'?<Button className="layout-project-bar-toggle" aria-label={projectBarOpen?'收起项目切换栏':'展开项目切换栏'} title={projectBarOpen?'收起项目切换栏':'展开项目切换栏'} aria-expanded={projectBarOpen} aria-controls={projectBarId} onClick={()=>setProjectBarOpen(!projectBarOpen)}>{projectBarOpen?<ChevronUp size={14}/>:<ChevronDown size={14}/>}</Button>:undefined}/>) }
    </div>
  </div>;
}

function ProjectColumn({context,project,workspace,columns,focused,onFocus,projectToggle}:{context:ProjectSurfaceContext;project:Project;workspace:Workspace;columns:boolean;focused:boolean;onFocus:()=>void;projectToggle?:ReactNode}){
  const [chrome,setChrome]=useState<HTMLDivElement|null>(null),[tools,setTools]=useState<HTMLDivElement|null>(null);
  const candidates=context.workspaces.filter(w=>w.project_id===project.id&&!w.archived);
  return <section className="layout-project-column" aria-label={project.name+' 项目工作区'} data-focused={focused} onFocusCapture={onFocus} onPointerDownCapture={onFocus}>
    {columns&&<header className="layout-column-heading"><Folder size={14}/><span title={project.path}>{project.name}</span><small>{workspaceIsRoot(workspace)?'':workspace.name}</small></header>}
    <ChromeTarget.Provider value={chrome}><WorkspaceToolsTarget.Provider value={tools}><ProjectContentsTarget.Provider value={null}>
      <div className="layout-terminal-bar"><div className="shell-page-chrome workbench-tabbar" ref={setChrome}/>{projectToggle}<div className="project-tools" ref={setTools}/></div>
      <Workbench key={workspace.id} workspace={workspace} tasks={context.tasks} onTasksChanged={context.onRefresh} keyboardActive={focused} compactChrome workspaceActions={<ActionMenu label={project.name+' 工作区'} icon={<ChevronDown size={14}/>}>
        {candidates.map(w=><Button key={w.id} aria-current={w.id===workspace.id?'page':undefined} onClick={()=>context.onWorkspace(w)}>{workspaceIsRoot(w)?'默认工作区':w.name}</Button>)}
      </ActionMenu>}/>
    </ProjectContentsTarget.Provider></WorkspaceToolsTarget.Provider></ChromeTarget.Provider>
  </section>;
}
