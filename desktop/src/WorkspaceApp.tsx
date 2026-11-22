import {useConfirm} from './ConfirmAction';
import {useEffect,useRef,useState,type ReactNode} from 'react';
import {Archive,ArrowLeft,ArrowRight,ArrowUpToLine,ChevronDown,ChevronRight,Folder,FolderPlus,GitBranch,MoreHorizontal,Pencil,Plus,Search,Star,X,PanelLeft,Sun,Moon,Terminal,Settings2,LayoutGrid,Inbox,Zap,Network,Blocks,Monitor} from 'lucide-react';
import {open,notifyDesktop,onAction,invoke} from './desktop-platform';
import {copyText} from './text-clipboard';
import {rpc,isTauri,executionHosts,Project,Task,phases} from './api';
import {taskDisplayState} from './task-navigation';
import {taskWorkspaceId,workspaceRows,workspaceTask,workspaceTreeRows,workspaceParentCandidates,workspaceIsRoot,workspaceTopPosition} from './workspace-context';
import Workbench from './Workbench';
import {TerminalSessionProvider} from './TerminalSessions';
import WorkspaceComposer from './WorkspaceComposer';
import ProjectCreator from './ProjectCreator';
import AddProjectDialog from './AddProjectDialog';
import CapabilitiesPanel from './CapabilitiesPanel';
import AutomationsPage from './AutomationsPage';
import OrchestrationPage from './OrchestrationPage';
import {ChromeTarget,WorkspaceToolsTarget,ProjectContentsTarget,WindowControls} from './WindowChrome';
import './workspace-tabstrip.css';
import ProjectHome from './ProjectHome';
import ProjectWorkspace from './ProjectWorkspace';
import ProjectSidebar from './ProjectSidebar';
import {useProjectLibrary} from './useProjectLibrary';
import {ownsProjectLibrary} from './project-library';
import {useShellHistory} from './useShellHistory';
import {useAppearance,usePreference} from './Appearance';
import ResizeHandle from './ResizeHandle';
import {clampPanelWidth} from './ui-preferences';
import {Button,Input,Textarea,NativeDialog,Tip} from './ui';
import ActionMenu from './ActionMenu';
import {Workspace,draftPrefix} from './workbench-types';
import './workspace.css';
import './workbench-a.css';
import './project-shell.css';
import TaskSourcesPage from './TaskSourcesPage';
import SourceTaskDraft from './SourceTaskDraft';
import {SourceItem,SourceLink} from './task-sources';
import ResourceStatus from './ResourceStatus';
import ExecutionHostsPanel,{useExecutionTarget} from './ExecutionHostsPanel';
import {currentTarget,namespacedKey,sameTarget,checkedTarget,selectTarget,type ExecutionTarget} from './execution-target';
import {useDesktopTerminalSwitch} from './useDesktopTerminalSwitch';
import './orca-light.css';
import './project-orca.css';
import AppSettings from './AppSettings';
import PluginsPage from './PluginsPage';
import LoadingState from './LoadingState';
import {useWorkbenchPreferences} from './useWorkbenchPreferences';
import {readWorkbenchPreferences,shortcutMatches} from './workbench-preferences';
import {TaskNotificationTracker} from './task-notifications';
import {HudDigestBuilder,pushHud} from './hud-digest';
import {accountLaunchFields,type AccountProfile,type AccountProvider} from './account-preferences';
import type {SettingsDestination} from './SettingsPages';

export type ProjectSurfaceContext={projects:Project[];workspaces:Workspace[];workspace:Workspace;tasks:Task[];onWorkspace:(workspace:Workspace)=>void;onRefresh:()=>Promise<void>};
export default function WorkspaceApp({projectSurface}:{projectSurface?:(context:ProjectSurfaceContext)=>ReactNode}={}){
  const ask=useConfirm(),{theme,setTheme}=useAppearance();
  const preferences=useWorkbenchPreferences(),notificationTracker=useRef(new TaskNotificationTracker());
  const hudDigest=useRef(new HudDigestBuilder()),electronHUD=!!window.velellaDesktop;
  const pendingNotificationTask=useRef<{target:ExecutionTarget;taskId:string}|null>(null),settingsDirty=useRef(false),notificationSequence=useRef(0);
  const [notificationError,setNotificationError]=useState('');
  const loginAttempt=useRef<{key:string;requestId:string}|null>(null);
  const [registeredProjects,setProjects]=useState<Project[]>([]),[workspaces,setWorkspaces]=useState<Workspace[]>([]),[tasks,setTasks]=useState<Task[]>([]);
  const [snapshotScope,setSnapshotScope]=useState(()=>namespacedKey(currentTarget(),'project-snapshot'));
  const [capabilities,setCapabilities]=useState(false);
  const [pluginPage,setPluginPage]=useState(false);
  const [appSettings,setAppSettingsState]=useState(false);
  function setAppSettings(value:boolean){if(value)setPluginPage(false);setAppSettingsState(value)}
  async function openPlugins(){if(settingsDirty.current&&!await ask('打开插件并放弃未保存的设置？'))return;setAppSettings(false);setPluginPage(true)}
  useEffect(()=>{
    const open=()=>setAppSettings(true);
    window.addEventListener('velella:open-settings',open);
    return()=>{window.removeEventListener('velella:open-settings',open)};
  },[]);
  const [orchestrationPage,setOrchestrationPage]=useState(false),[orchestrationSession,setOrchestrationSession]=useState<string|undefined>();
  const [automationPage,setAutomationPageState]=useState(false);
  function setAutomationPage(value:boolean){setPluginPage(false);if(value)setAppSettings(false);setOrchestrationPage(false);setAutomationPageState(value)}
  function setTaskPage(value:boolean){setPluginPage(false);if(value)setAppSettings(false);setOrchestrationPage(false);setTaskPageState(value)}
  const [taskPage,setTaskPageState]=useState(false),[sourceKey,setSourceKey]=useState<string|undefined>(),[sourceLinks,setSourceLinks]=useState<SourceLink[]>([]),[sourceItem,setSourceItem]=useState<SourceItem|undefined>();
  const [creatingProject,setCreatingProject]=useState(false);
  const [addingProject,setAddingProject]=useState(false);
  const [createThenWorkspace,setCreateThenWorkspace]=useState(false);
  const [workspaceId,setWorkspaceIdState]=useState(()=>localStorage.getItem(namespacedKey(currentTarget(),'velella:workspace'))||'');
  const [home,setHome]=useState(()=>!workspaceId);
  function setWorkspaceId(id:string){setPluginPage(false);setAppSettings(false);setWorkspaceIdState(id);setHome(false)}
  const target=useExecutionTarget(),remote=target.kind==='ssh';
  const snapshotCurrent=snapshotScope===namespacedKey(target,'project-snapshot');
  const projectLibrary=useProjectLibrary(namespacedKey(target,'velella:project-library:v1'),message=>setError(message),()=>ownsProjectLibrary(snapshotScope,namespacedKey(target,'project-snapshot'),namespacedKey(currentTarget(),'project-snapshot')));
  const projects=snapshotCurrent?registeredProjects.filter(p=>!projectLibrary.state.projects[p.id]?.removed):[];
  const [navigationScope,setNavigationScope]=useState(()=>namespacedKey(currentTarget(),'shell'));
  const [hostManagement,setHostManagement]=useState(false),[hostName,setHostName]=useState('远端执行主机');
  useEffect(()=>{if(target.kind!=='ssh'){setHostName('远端执行主机');return}let live=true;void executionHosts<{hosts:{id:string,name:string}[];connections:{host_id:string;state:string}[]}>({action:'list'}).then(r=>{if(!live)return;const joined=r.connections.find(c=>c.state==='connected');setHostName(r.hosts.find(h=>h.id===joined?.host_id)?.name||'远端执行主机')}).catch(()=>{if(live)setHostName('远端执行主机（已断开）')});return()=>{live=false}},[target]);
  const [search,setSearch]=useState(''),[error,setError]=useState(''),[connectionError,setConnectionError]=useState(''),[connectionReady,setConnected]=useState(false),[archived,setArchived]=useState(false);
  const connected=connectionReady&&snapshotCurrent;
  const [connectionFailures,setConnectionFailures]=useState(0);
  const [hasSnapshot,setHasSnapshot]=useState(false);
  const [mode,setMode]=useState('project'),[text,setText]=useState(''),[busy,setBusy]=useState(false),[composer,setComposer]=useState<Project|null>(null);
  const [composerParent,setComposerParent]=useState<string|null>(null),[parentId,setParentId]=useState('');
  const [defaultBase,setDefaultBase]=useState(''),[setupDefault,setSetupDefault]=useState<'run'|'skip'>('skip');
  const [dialogTarget,setDialogTarget]=useState<{workspaceId?:string;projectId?:string}>({});
  const [leftOpen,setLeftOpen]=usePreference('velella:ui:left-open',true),[leftSize,setLeftSize]=usePreference('velella:ui:project-sidebar-width',180);
  const [projectLeftSize,setProjectLeftSize]=usePreference('velella:ui:workspace-sidebar-width',280);
  const [projectTools,setProjectTools]=useState<HTMLDivElement|null>(null);
  const [projectContents,setProjectContents]=useState<HTMLDivElement|null>(null);
  const projectPicker=useRef<HTMLDialogElement>(null),statusDialog=useRef<HTMLDialogElement>(null),helpDialog=useRef<HTMLDialogElement>(null);
  const [chrome,setChrome]=useState<HTMLDivElement|null>(null),[collapsed,setCollapsed]=useState<Set<string>>(new Set());
  const dialog=useRef<HTMLDialogElement>(null),leftWidth=clampPanelWidth(Number(leftSize),180,320);
  const rows=workspaceRows(workspaces,tasks).filter(w=>projects.some(p=>p.id===w.project_id)),workspace=rows.find(w=>w.id===workspaceId),project=projects.find(p=>p.id===workspace?.project_id);
  const persisted=workspaces.some(w=>w.id===workspaceId),task=workspace?workspaceTask(workspace,tasks):undefined;
  const editingWorkspace=rows.find(w=>w.id===dialogTarget.workspaceId);
  const parentCandidates=mode==='parent'&&editingWorkspace?workspaceParentCandidates(editingWorkspace,rows):[];
  const unavailableParent=!!parentId&&!parentCandidates.some(w=>w.id===parentId);
  function accept(r:any,issued:ExecutionTarget=currentTarget()){
    if(!sameTarget(issued,currentTarget()))return;
    const notificationPreferences=readWorkbenchPreferences().notifications;
    const notices=notificationTracker.current.update(namespacedKey(issued,'notifications'),r.tasks||[],notificationPreferences,!document.hasFocus()||document.hidden);
    for(const notice of notices)void notifyDesktop({title:notice.title,body:notice.body,silent:!notificationPreferences.sound,extra:{taskId:notice.taskId,target:issued}}).catch(()=>{});
    setSourceLinks(r.source_links||[]);setProjects(r.projects);setWorkspaces(r.workspaces);setTasks(r.tasks||[]);setSnapshotScope(namespacedKey(issued,'project-snapshot'));setConnected(true);setHasSnapshot(true);setConnectionFailures(0);setConnectionError('');
    pushHud(hudDigest.current.build(r.tasks||[],r.projects||[],true));
    setWorkspaceIdState(id=>workspaceRows(r.workspaces,r.tasks||[]).some(w=>w.id===id)?id:r.workspaces.find((w:Workspace)=>!w.archived)?.id||'');
    const pending=pendingNotificationTask.current;
    if(pending&&sameTarget(pending.target,issued)){
      pendingNotificationTask.current=null;const notifiedTask=(r.tasks||[]).find((task:Task)=>task.id===pending.taskId);
      if(notifiedTask){setWorkspaceId(taskWorkspaceId(notifiedTask));setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false);setNotificationError('')}
      else setNotificationError('通知对应的任务已不可用，请查看该主机的任务列表。');
    }
  }
  async function refresh(issued:ExecutionTarget=currentTarget()){const snapshot=await rpc({cmd:'workbench.snapshot'},issued);accept(snapshot,issued)}
  useEffect(()=>{let live=true,timer:ReturnType<typeof setTimeout>;async function poll(){const issued=currentTarget();try{const r=await rpc({cmd:'workbench.snapshot'},issued);if(live)accept(r,issued)}catch(e){if(live&&sameTarget(issued,currentTarget())){notificationTracker.current.reset();setConnected(false);setConnectionFailures(value=>value+1);setConnectionError(String(e));pushHud(hudDigest.current.disconnected())}}if(live)timer=setTimeout(poll,1500)}void poll();return()=>{live=false;clearTimeout(timer)}},[]);
  useEffect(()=>{const saved=localStorage.getItem(namespacedKey(target,'velella:workspace'))||'';setWorkspaceIdState(saved);setHome(!saved);setNavigationScope(namespacedKey(target,'shell'));setConnected(false);setHasSnapshot(false);setConnectionError('');setError('');setSearch('');setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false);setComposer(null);setAddingProject(false);setCreatingProject(false);dialog.current?.close();setDialogTarget({})},[target]);
  useEffect(()=>{if(workspaceId)localStorage.setItem(namespacedKey(target,'velella:workspace'),workspaceId)},[workspaceId,target]);
  useEffect(()=>{const openTask=(event:Event)=>{const id=(event as CustomEvent<{taskId?:string}>).detail?.taskId;const task=id?tasks.find(t=>t.id===id):undefined;if(task){setWorkspaceId(taskWorkspaceId(task));setTaskPage(false);setAutomationPage(false)}};window.addEventListener('velella:open-task',openTask);return()=>window.removeEventListener('velella:open-task',openTask)},[tasks]);
  useEffect(()=>{
    if(!window.velellaDesktop)return;
    const off=window.velellaDesktop.listen('velella:hud-open-task',(event:{payload:{taskId?:string}})=>{
      const id=event.payload?.taskId,found=id?tasks.find(t=>t.id===id):undefined;
      if(found){setWorkspaceId(taskWorkspaceId(found));setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false)}
    });
    return()=>off();
  },[tasks]);
  function show(next:string,value='',context?:{workspace?:Workspace;project?:Project}){
    const targetWorkspace=context?context.workspace:workspace,targetProject=context?context.project:project;
    setMode(next);setText(next==='settings'?targetProject?.setup||'':value);setError('');
    setDialogTarget({workspaceId:targetWorkspace?.id,projectId:targetProject?.id});
    if(next==='settings'){setDefaultBase(targetProject?.default_base||'');setSetupDefault(targetProject?.setup_default||'skip')}
    if(next==='parent')setParentId(targetWorkspace?.parent_workspace_id||'');
    dialog.current?.showModal();
  }
  function revealProject(id:string){if(projectLibrary.state.projects[id]?.removed&&!projectLibrary.restore(id))throw Error('无法恢复项目列表条目，请检查本机存储后重试。')}
  async function addProject(path:string){setBusy(true);try{const r=await rpc({cmd:'project.add',path});revealProject(r.project.id);await refresh();setWorkspaceId('main:'+r.project.id);dialog.current?.close();setError('')}catch(e){setError(String(e))}finally{setBusy(false)}}
  async function chooseProject(){if(isTauri()&&!remote){try{const path=await open({directory:true,multiple:false,title:'打开项目文件夹'});if(typeof path==='string')await addProject(path)}catch(e){setError(String(e))}}else show('project')}
  function openComposer(target:Project,parent:string|null=null,item?:SourceItem){setSourceItem(item);setComposerParent(parent);setComposer(target)}
  function createWorkspace(){setSourceItem(undefined);setComposerParent(null);const target=project||projects[0];if(target)openComposer(target);else{setCreateThenWorkspace(true);setCreatingProject(true)}}
  function openWorkspace(chosen:Workspace){if(!snapshotCurrent||!sameTarget(target,currentTarget()))return;const meta=projectLibrary.state.workspaces?.[chosen.id];if((meta?.unread||meta?.sleeping)&&!projectLibrary.editWorkspace(chosen.id,{unread:false,sleeping:false}))return;setWorkspaceId(chosen.id);setTaskPage(false);setAutomationPage(false);setSearch('');setCollapsed(current=>{const next=new Set(current);next.delete(chosen.project_id);return next})}
  async function submit(){
    if(busy)return;
    if(mode==='project')return addProject(text);
    if(mode==='settings'?!projects.some(p=>p.id===dialogTarget.projectId):!workspaces.some(w=>w.id===dialogTarget.workspaceId)){setError('项目或工作区已不可用，请关闭后重试。');return}
    if(mode==='parent'&&(!editingWorkspace||workspaceIsRoot(editingWorkspace)||unavailableParent)){setError('请选择有效的父工作区；主工作区固定为顶级。');return}
    setBusy(true);
    try{
      await rpc(mode==='settings'
        ?{cmd:'project.update',project_id:dialogTarget.projectId,default_base:defaultBase.trim()||null,setup_default:setupDefault,setup:text}
        :{cmd:'workbench.update',workspace_id:dialogTarget.workspaceId,...(mode==='parent'?{parent_workspace_id:parentId||null}:{name:text})});
      await refresh();dialog.current?.close();setError('');
    }catch(e){setError(String(e))}finally{setBusy(false)}
  }
  async function update(fields:Record<string,unknown>){if(!workspace)return;try{await rpc({cmd:'workbench.update',workspace_id:workspace.id,...fields});await refresh();setError('')}catch(e){setError(String(e))}}
  async function archive(){if(!workspace)return;if(!workspace.archived){const state=await rpc({cmd:'workbench.git',workspace_id:workspace.id}).catch(()=>null);if(!(await ask('归档此工作区？文件和草稿保留，活跃会话会阻止归档。\n'+(state?.status||'无法读取 Git 状态')+'\n未保存草稿：'+Object.keys(localStorage).filter(k=>k.startsWith(draftPrefix(workspace.id))).length)))return}await update({archived:!workspace.archived,confirmed:true})}
  const page=appSettings?'settings':pluginPage?'plugins':orchestrationPage?'orchestration':automationPage?'automation':taskPage?'sources':home||!workspace?'home':'project';
  const sidebarWidth=page==='project'?clampPanelWidth(Number(projectLeftSize),220,500):leftWidth;
  const desktopSwitch=useDesktopTerminalSwitch((snapshot,request)=>{
    accept(snapshot);setWorkspaceId(request.workspace_id);setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false);
  },request=>page==='project'&&workspaceId===request.workspace_id);
  const navigation=useShellHistory({page},navigationScope,location=>{
    setAppSettings(location.page==='settings');
    setPluginPage(location.page==='plugins');
    if(location.page==='settings')return;
    setOrchestrationPage(location.page==='orchestration');setAutomationPageState(location.page==='automation');setTaskPageState(location.page==='sources');setHome(location.page==='home');
  });
  function openProject(p:Project){const candidate=rows.find(w=>w.project_id===p.id&&!w.archived&&workspaceIsRoot(w))||rows.find(w=>w.project_id===p.id&&!w.archived);if(candidate)openWorkspace(candidate);else openComposer(p)}
  function showHome(){setAppSettings(false);setTaskPage(false);setAutomationPage(false);setHome(true)}
  async function toggleHud(){try{await invoke('hud',{action:'toggle'});setError(previous=>previous.startsWith('任务监视窗打开失败：')?'':previous)}catch(error){setError(`任务监视窗打开失败：${String(error)}`)}}
  useEffect(()=>{const handler=(event:KeyboardEvent)=>{
    if(event.defaultPrevented||event.isComposing||event.repeat||document.querySelector('dialog[open], [role="dialog"][data-state="open"]')||(event.target instanceof Element&&event.target.closest('.settings-keybinding[aria-pressed=true]')))return;
    const actions={settings:()=>setAppSettings(true),sidebar:()=>setLeftOpen(!leftOpen),workspace:createWorkspace,projects:()=>projectPicker.current?.showModal(),resources:()=>statusDialog.current?.showModal()};
    for(const id of Object.keys(actions) as (keyof typeof actions)[])if(shortcutMatches(id,event,preferences)){if(appSettings&&id!=='settings')return;event.preventDefault();event.stopImmediatePropagation();actions[id]();return}
  };window.addEventListener('keydown',handler,true);return()=>window.removeEventListener('keydown',handler,true)},[preferences,appSettings,leftOpen,project,projects,connected]);
  useEffect(()=>{if(!isTauri())return;let live=true,unregister:(()=>Promise<void>)|undefined;void onAction(action=>{void (async()=>{
    const extra=action.notification?.extra as {taskId?:string;target?:ExecutionTarget}|undefined;if(!extra?.taskId||!extra.target)return;
    const sequence=++notificationSequence.current,previous=currentTarget();
    try{
      const destination=checkedTarget(extra.target);
      if(destination.kind==='ssh'){
        const managed=await executionHosts<{connections:{state:string;target:ExecutionTarget|null}[]}>({action:'list'});
        if(!managed.connections.some(connection=>connection.state==='connected'&&connection.target&&sameTarget(connection.target,destination)))throw Error('通知对应的执行主机未连接。请先连接原主机，再打开通知。');
      }
      if(settingsDirty.current&&!await ask('打开通知对应的任务并放弃未保存的设置？'))return;
      if(!live||sequence!==notificationSequence.current)return;
      if(!sameTarget(previous,currentTarget()))throw Error('执行主机已切换，请再次点击通知。');
      pendingNotificationTask.current={target:destination,taskId:extra.taskId};setNotificationError('');setAppSettings(false);
      const switching=!sameTarget(destination,previous);selectTarget(destination);
      // After a host switch, wait for its next bound snapshot. The host-reset
      // effect must finish before selecting that host's task workspace.
      if(!switching)await refresh();
    }catch(error){if(live)setNotificationError(String(error))}
  })()}).then(handle=>{if(live)unregister=()=>handle.unregister();else void handle.unregister()}).catch(()=>{});return()=>{live=false;void unregister?.()}},[]);
  useEffect(()=>{const pending=pendingNotificationTask.current;if(pending&&!sameTarget(pending.target,target))pendingNotificationTask.current=null},[target]);
  function openSettingsDestination(destination:SettingsDestination){
    if((destination==='project'||destination==='tools')&&(!project||!workspace||!persisted||workspace.archived)){setError('请先打开一个可用项目工作区。');return}
    setAppSettings(false);
    if(destination==='hosts')setHostManagement(true);
    if(destination==='sources'){setTaskPage(true);setAutomationPage(false);setSourceKey(undefined)}
    if(destination==='automation'){setAutomationPage(true);setTaskPage(false)}
    if(destination==='tools')setCapabilities(true);
    if(destination==='project')show('settings');
  }
  async function openAccountLogin(profile:AccountProfile|{provider:AccountProvider}){
    if(!workspace||!persisted||workspace.archived)throw Error('请先打开一个可用工作区。');
    const issued=currentTarget(),home='home' in profile?profile.home:undefined;
    const protocol=await rpc({cmd:'protocol'},issued);if(!protocol.capabilities?.includes('terminal-accounts-v1'))throw Error('当前后台尚不支持账号登录终端，请升级后台。');
    const account=await accountLaunchFields(home,issued),key=JSON.stringify([issued,workspace.id,profile.provider,home]);
    if(!loginAttempt.current||loginAttempt.current.key!==key)loginAttempt.current={key,requestId:crypto.randomUUID()};
    if(!sameTarget(issued,currentTarget()))throw Error('执行主机已切换，未打开登录终端。');
    const result=await rpc({cmd:'workbench.terminal.open',workspace_id:workspace.id,launch:profile.provider,account_action:'login',request_id:loginAttempt.current.requestId,...account},issued);
    if(!sameTarget(issued,currentTarget()))throw Error('登录终端保留在原执行主机，请切回后查看已有会话。');
    setOrchestrationSession(result.session_id);setAppSettings(false);setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false);setHome(false);loginAttempt.current=null;
  }
  async function copyPath(){try{await copyText(workspace?.path||project?.path||'')}catch(e){setError(e instanceof Error?e.message:String(e))}}
  async function removeProjectFromList(project:Project){
    const issued=currentTarget();
    if(!await ask('从鱼群项目列表移除“'+project.name+'”？\n'+(remote?'远端主机上的':'磁盘上的')+'文件、工作区和会话记录都会保留。重新打开同一目录即可恢复。'))return;
    if(!sameTarget(issued,currentTarget()))return;
    if(projectLibrary.edit(project.id,{removed:true})&&workspace?.project_id===project.id)showHome();
  }
  async function updateWorkspaceEntry(w:Workspace,fields:Record<string,unknown>){const issued=target;if(!snapshotCurrent||!sameTarget(issued,currentTarget()))return;try{setBusy(true);await rpc({cmd:'workbench.update',workspace_id:w.id,...fields},issued);await refresh(issued)}catch(e){if(sameTarget(issued,currentTarget()))setError(String(e))}finally{setBusy(false)}}
  function editWorkspaceEntry(w:Workspace,mode:'rename'|'parent',name?:string){if(!snapshotCurrent||!sameTarget(target,currentTarget()))return;show(mode,name??w.name,{workspace:w,project:projects.find(p=>p.id===w.project_id)})}
  async function archiveWorkspaceEntry(w:Workspace){const issued=target;if(!snapshotCurrent||!sameTarget(issued,currentTarget()))return;if(!await ask('归档工作区“'+w.name+'”？文件和会话记录保留，仍有终端运行时不会归档。')||!sameTarget(issued,currentTarget()))return;await updateWorkspaceEntry(w,{archived:true,confirmed:true})}
  async function sleepWorkspace(w:Workspace){
    const issued=target;if(!snapshotCurrent||!sameTarget(issued,currentTarget()))return;try{
      setBusy(true);const listed=await rpc({cmd:'workbench.sessions',workspace_id:w.id},issued);
      const running=(listed.sessions as import('./workbench-types').Session[]).filter(s=>s.status?.exit===null&&s.status.state!=='host_lost');
      if(!sameTarget(issued,currentTarget())||!await ask('休眠工作区“'+w.name+'”？\n将停止此工作区的 '+running.length+' 个运行中终端；相关任务执行也会停止。文件和会话历史保留。')||!sameTarget(issued,currentTarget()))return;
      const stoppedTasks=new Set<string>();for(const session of running){if(session.task_id&&stoppedTasks.has(session.task_id))continue;if(!sameTarget(issued,currentTarget()))throw Error('执行主机已切换，已停止继续休眠操作。');await rpc({cmd:'workbench.terminal.stop',workspace_id:w.id,session_id:session.id,confirmed:true},issued);if(session.task_id)stoppedTasks.add(session.task_id)}
      if(!sameTarget(issued,currentTarget()))return;
      let stopped=false;for(let attempt=0;attempt<8;attempt++){if(!sameTarget(issued,currentTarget()))return;const after=await rpc({cmd:'workbench.sessions',workspace_id:w.id},issued);if(!(after.sessions as import('./workbench-types').Session[]).some(s=>s.status?.exit===null&&s.status.state!=='host_lost')){stopped=true;break}if(attempt<7)await new Promise(resolve=>setTimeout(resolve,250))}
      if(!stopped)throw Error('仍有终端运行，工作区尚未休眠。');
      if(sameTarget(issued,currentTarget()))projectLibrary.editWorkspace(w.id,{sleeping:true});
    }catch(e){if(sameTarget(issued,currentTarget()))setError(String(e))}finally{setBusy(false)}
  }
  async function openWorkspaceFolder(w:Workspace){const issued=target;if(!snapshotCurrent||!sameTarget(issued,currentTarget()))return;try{await invoke('workspace_open_folder',{workspace_id:w.id,target:issued})}catch(e){if(sameTarget(issued,currentTarget()))setError(String(e))}}
  const hasWorkspaceStrip=!!(page==='project'&&workspace&&persisted&&!workspace.archived&&!appSettings&&!pluginPage);
  return <TerminalSessionProvider><ChromeTarget.Provider value={chrome}><WorkspaceToolsTarget.Provider value={projectTools}><ProjectContentsTarget.Provider value={projectContents}><div className={'ide-shell workbench-a-shell project-shell '+(leftOpen?'':'left-hidden')} data-shell-page={page} style={{gridTemplateColumns:appSettings?'minmax(0,1fr)':(leftOpen?sidebarWidth:0)+'px minmax(0,1fr)'}}>
    <header className="app-titlebar">
      {!appSettings&&<Tip label={leftOpen?'收起项目栏':'展开项目栏'}><Button aria-label={leftOpen?'收起项目栏':'展开项目栏'} onClick={()=>setLeftOpen(!leftOpen)}><PanelLeft size={16}/></Button></Tip>}
      <div className="shell-history"><Button aria-label="返回" disabled={!navigation.canBack} onClick={()=>navigation.go(-1)}><ArrowLeft size={16}/></Button><Button aria-label="前进" disabled={!navigation.canForward} onClick={()=>navigation.go(1)}><ArrowRight size={16}/></Button></div>
      <nav className="shell-menus" aria-label="应用菜单">
        <ActionMenu label="文件" align="start"><Button disabled={!connected||busy} onClick={()=>setCreatingProject(true)}>创建项目</Button><Button disabled={!connected||busy} onClick={()=>setAddingProject(true)}>打开项目</Button><Button onClick={()=>projectPicker.current?.showModal()}>项目与工作区</Button><div className="wb-menu-divider"/><Button disabled={!connected||busy} onClick={createWorkspace}>新建工作区</Button><Button disabled={!project} onClick={()=>show('settings')}>项目设置</Button></ActionMenu>
        <ActionMenu label="编辑" align="start"><Button disabled={!workspace} onClick={()=>void copyPath()}>复制工作区路径</Button><Button disabled={!workspace} onClick={()=>show('rename',workspace?.name)}>重命名工作区</Button></ActionMenu>
        <ActionMenu label="视图" align="start"><Button onClick={()=>setAppSettings(true)}>设置…</Button><Button onClick={()=>void openPlugins()}>插件</Button><div className="wb-menu-divider"/><Button onClick={()=>setLeftOpen(!leftOpen)}>{leftOpen?'收起侧栏':'展开侧栏'}</Button><Button onClick={()=>setTheme(theme==='dark'?'light':'dark')}>{theme==='dark'?'切换浅色主题':'切换深色主题'}</Button><Button onClick={()=>setHostManagement(true)}>执行主机</Button><Button disabled={!workspace||!project||!persisted||workspace.archived} onClick={()=>setCapabilities(true)}>工作区工具</Button><Button onClick={()=>statusDialog.current?.showModal()}>状态与资源</Button></ActionMenu>
        <ActionMenu label="帮助" align="start"><Button onClick={()=>helpDialog.current?.showModal()}>关于鱼群</Button></ActionMenu>
      </nav>
      <div className="workbench-window-drag" data-tauri-drag-region/>
      {electronHUD&&<Tip label="任务监视窗"><Button aria-label="任务监视窗" onClick={()=>void toggleHud()}><Monitor size={16}/></Button></Tip>}
      <WindowControls/><div className="chrome-drag-strip" data-tauri-drag-region/>
    </header>
    {hasWorkspaceStrip&&<div className="workspace-terminal-strip" role="region" aria-label={workspace!.name+' 工作区标签栏'}>
      <div className="shell-page-chrome workbench-tabbar" ref={setChrome}/>
      <div className="project-tools" ref={setProjectTools}/>
    </div>}
    {leftOpen&&!appSettings&&<aside className="ide-sidebar" aria-label={page==='project'?'项目导航':'Velella 导航'}>{page==='project'?<ProjectSidebar key={namespacedKey(target,'project-sidebar')} projects={projects} workspaces={rows} tasks={tasks} workspace={workspace} disabled={!connected||busy} library={projectLibrary} onHosts={()=>setHostManagement(true)} hostLabel={remote?hostName:'Local Windows'} remote={remote} workspaceActions={{onOpen:openWorkspace,onEdit:editWorkspaceEntry,onUpdate:(w,fields)=>void updateWorkspaceEntry(w,fields),onSleep:w=>void sleepWorkspace(w),onArchive:w=>void archiveWorkspaceEntry(w),onFolder:w=>void openWorkspaceFolder(w),onError:setError}} onProject={openProject} onWorkspace={openWorkspace} onAdd={()=>setAddingProject(true)} onHome={showHome} onSettings={()=>setAppSettings(true)} onContents={setProjectContents} onNewWorkspace={p=>openComposer(p)} onProjectSettings={p=>show('settings','',{project:p})} onRemove={p=>void removeProjectFromList(p)}/>:<><nav className="workbench-navigation" aria-label="主导航">
      <Button aria-label="工作台" aria-current={page==='home'?'page':undefined} className={'ide-nav '+(page==='home'?'selected':'')} onClick={showHome}><LayoutGrid size={18}/>工作台</Button>
      {!remote&&<><Button aria-label="Agent 编排" aria-current={orchestrationPage?'page':undefined} className={'ide-nav '+(orchestrationPage?'selected':'')} onClick={()=>{setTaskPage(false);setAutomationPage(false);setOrchestrationPage(true)}}><Network size={18}/>Agent 编排</Button>
      <Button aria-label="自动化" aria-current={automationPage?'page':undefined} className={'ide-nav '+(automationPage?'selected':'')} onClick={()=>{setAutomationPage(true);setTaskPage(false)}}><Zap size={18}/>自动化</Button>
      <Button aria-label="任务来源" aria-current={taskPage?'page':undefined} className={'ide-nav '+(taskPage?'selected':'')} onClick={()=>{setTaskPage(true);setAutomationPage(false);setSourceKey(undefined)}}><Inbox size={18}/>任务来源</Button></>}
      <Button aria-label="插件" title="插件" aria-current={pluginPage?'page':undefined} className={'ide-nav '+(pluginPage?'selected':'')} onClick={()=>void openPlugins()}><Blocks size={18}/>插件</Button>
      </nav>
      <div className="app-settings-entry"><Button className="ide-nav" aria-label="设置" title="设置 · Ctrl+," onClick={()=>setAppSettings(true)}><Settings2 size={17}/>设置</Button></div></>}
      <ResizeHandle side="left" width={sidebarWidth} min={page==='project'?220:180} max={page==='project'?500:320} onChange={page==='project'?setProjectLeftSize:setLeftSize}/>
    </aside>}
    <NativeDialog ref={projectPicker} className="shell-project-picker" aria-label="项目与工作区"><div className="dialog-heading"><h2>项目与工作区</h2><Button aria-label="关闭项目列表" onClick={()=>projectPicker.current?.close()}><X size={16}/></Button></div><div className="dialog-body" onClick={e=>{if((e.target as Element).closest('.ide-workspace,.workspace-parent-link'))projectPicker.current?.close()}}><label className="ide-search"><Search size={14}/><Input aria-label="搜索工作区" value={search} onChange={e=>setSearch(e.target.value)} placeholder="搜索工作区"/></label>
      <div className="ide-project-heading"><span>项目</span><div className="ide-project-actions"><Tip label="添加项目"><Button type="button" aria-label="添加项目" disabled={!connected||busy} onClick={()=>setAddingProject(true)}><FolderPlus size={16}/></Button></Tip><Tip label="新建工作区"><Button type="button" aria-label="新建工作区" disabled={!connected||busy} onClick={createWorkspace}><Plus size={16}/></Button></Tip></div></div>
      <nav className="ide-projects" aria-label="项目工作区">{projects.map(p=>{
        const visible=workspaceTreeRows(rows,{projectId:p.id,projectName:p.name,search,includeArchived:archived});
        if(!visible.length&&search)return null;
        const expanded=!!search||!collapsed.has(p.id);
        return <div className="ide-project" key={p.id}><div className="ide-project-row"><Button className="ide-project-name" title={p.path} aria-expanded={expanded} onClick={()=>setCollapsed(current=>{const next=new Set(current);next.has(p.id)?next.delete(p.id):next.add(p.id);return next})}>{expanded?<ChevronDown size={13}/>:<ChevronRight size={13}/>}<Folder size={15}/><span>{p.name}</span><small>{visible.length}</small></Button><Tip label={'在 '+p.name+' 中新建工作区'}><Button className="project-create-task" aria-label={'在 '+p.name+' 中新建工作区'} disabled={!connected||busy} onClick={()=>openComposer(p)}><Plus size={14}/></Button></Tip></div>
          {expanded&&<div className="ide-workspaces workspace-hierarchy" role="list" aria-label={p.name+' 的工作区'}>{visible.map(({workspace:w,depth,parent,contextOnly,archivedAncestor})=>{
            const t=workspaceTask(w,tasks),state=t?taskDisplayState(t):'';
            return <div key={w.id} role="listitem" className={'workspace-tree-row '+(contextOnly?'workspace-tree-context':'')} data-depth={depth} style={{paddingInlineStart:depth*12,minInlineSize:140+depth*12}}>
              <Button className={'ide-workspace '+(w.id===workspaceId&&!automationPage&&!taskPage&&!orchestrationPage?'selected':'')} aria-current={w.id===workspaceId&&!automationPage&&!taskPage&&!orchestrationPage?'page':undefined} aria-description={'第 '+(depth+1)+' 层'+(archivedAncestor?'，已归档父级，仅显示层级上下文':'')} onClick={()=>{setWorkspaceId(w.id);setTaskPage(false);setAutomationPage(false)}} title={w.path}><GitBranch size={14}/><span>{w.kind==='primary'||w.kind==='folder'?'默认工作区':w.name}</span>{depth>0&&<small className="workspace-tree-depth" aria-hidden="true">{depth+1} 层</small>}{t&&<small className={'project-task-status '+state}>{state==='waiting_permission'?'等待授权':phases[state]}</small>}{w.favorite&&<Star size={12}/>} {w.archived&&<><Archive size={12}/><small className="workspace-tree-archive">{archivedAncestor?'已归档父级':'已归档'}</small></>}</Button>
              {parent&&parent.project_id!==p.id&&<Button className="workspace-parent-link" title={parent.path} aria-label={'打开父工作区：'+parent.name+(parent.archived?'（已归档）':'')} onClick={()=>openWorkspace(parent)}>父级：{projects.find(project=>project.id===parent.project_id)?.name||parent.project_id} / {parent.name}{parent.archived?'（已归档）':''}</Button>}
            </div>;
          })}</div>}
        </div>})}</nav>
      <div className="workbench-sidebar-footer"><div className="ide-sidebar-bottom"><Button className="ide-nav" disabled={!workspace||!project||!persisted||workspace.archived} onClick={()=>setCapabilities(true)}><Settings2 size={16}/>工作区工具</Button><ActionMenu label="项目列表选项" icon={<MoreHorizontal size={16}/>}><Button aria-pressed={archived} onClick={()=>setArchived(!archived)}><Archive size={14}/>{archived?'隐藏已归档':'显示已归档'}</Button></ActionMenu><ActionMenu label="工作区管理"><Button disabled={!workspace} onClick={()=>show('rename',workspace?.name)}>重命名</Button><Button disabled={!workspace} onClick={()=>void update({favorite:!workspace?.favorite})}>切换收藏</Button><Button disabled={!workspace} onClick={()=>void archive()}>{workspace?.archived?'恢复工作区':'归档工作区'}</Button></ActionMenu></div></div></div></NativeDialog>
    <main className="ide-main" hidden={appSettings||pluginPage} inert={appSettings||pluginPage}>{(error||notificationError||(hasSnapshot&&connectionError))&&<div className="ide-error" role="alert">{error||notificationError||connectionError}<Button aria-label="关闭错误提示" onClick={()=>{setError('');setNotificationError('');setConnectionError('')}}><X size={14}/></Button></div>}

      {page==='project'&&workspace&&sourceLinks.some(link=>link.workspace_id===workspace.id)&&<div className="source-workspace-links"><span>任务</span>{sourceLinks.filter(link=>link.workspace_id===workspace.id).map(link=><Button key={link.key} title={link.title} onClick={()=>{setSourceKey(link.key);setTaskPage(true);setAutomationPage(false)}}>{link.identifier} · {link.title}</Button>)}</div>}
      {page==='project'&&workspace&&<SourceTaskDraft key={'source-draft:'+workspace.id} workspace={workspace.id}/>}
      {!hasSnapshot?<div className="shell-loading">{connectionError?<div className="shell-loading-error" role="alert"><h2>暂时无法连接工作台</h2><p>请确认执行主机可用后重试。</p><Button onClick={()=>void refresh().catch(error=>setConnectionError(String(error)))}>重新连接</Button></div>:<LoadingState label="正在加载工作台"/>}</div>:orchestrationPage?<OrchestrationPage workspaces={workspaces} onOpen={(wid,sid)=>{setWorkspaceId(wid);setOrchestrationSession(sid);setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false)}}/>:automationPage?<AutomationsPage projects={projects} onOpenTask={async id=>{const snapshot=await rpc({cmd:'workbench.snapshot'});const found=snapshot.tasks?.find((t:Task)=>t.id===id);if(!found)throw Error('任务暂不可用，请刷新运行记录后重试。');accept(snapshot);setWorkspaceId(taskWorkspaceId(found));setAutomationPage(false)}}/>:taskPage?<TaskSourcesPage projects={projects} links={sourceLinks} initialKey={sourceKey} onOpen={id=>{setWorkspaceId(id);setTaskPage(false);setAutomationPage(false)}} onStart={item=>{const p=projects.find(p=>p.id===item.project_id)||project||projects[0];if(p)openComposer(p,null,item)}}/>:home||!workspace?<ProjectHome projects={projects} disabled={!connected||busy} onOpen={openProject} onCreate={()=>{setCreateThenWorkspace(false);setCreatingProject(true)}} onAdd={()=>setAddingProject(true)}/>:workspace?projectSurface?projectSurface({projects,workspaces,workspace,tasks,onWorkspace:openWorkspace,onRefresh:refresh}):workspace.archived?<div className="ide-empty"><Archive size={28}/><h1>工作区已归档</h1><p>文件和会话记录已保留。</p><Button onClick={archive}>恢复工作区</Button></div>:!persisted?<div className="ide-empty"><Terminal size={24}/><h2>{workspace.name}</h2><p>{task?.reason||'正在准备工作区…'}</p></div>:<ProjectWorkspace projects={projects} workspaces={workspaces} workspace={workspace} onWorkspace={openWorkspace}><Workbench key={workspace.id} workspace={workspace} compactChrome desktopSwitch={desktopSwitch} initialSession={orchestrationSession} tasks={tasks} onTasksChanged={refresh} workspaceActions={<ActionMenu label="工作区选项" icon={<MoreHorizontal size={16}/>}>
        <Button disabled={!project} onClick={()=>{if(project)openComposer(project,workspace.id)}}><Plus size={14}/>新建子工作区</Button>
        <Button onClick={()=>show('rename',workspace.name)}><Pencil size={14}/>重命名</Button>
        <Button disabled={workspaceIsRoot(workspace)} title={workspaceIsRoot(workspace)?'主工作区固定为顶级':'父级仅用于组织，不继承 Git 基准、权限或文件'} onClick={()=>show('parent')}><Network size={14}/>{workspaceIsRoot(workspace)?'主工作区固定顶级':'更改父工作区'}</Button>
        <Button onClick={()=>update({favorite:!workspace.favorite})}><Star size={14}/>{workspace.favorite?'取消收藏':'收藏工作区'}</Button>
        <Button onClick={()=>update({position:workspaceTopPosition(workspace,rows)})}><ArrowUpToLine size={14}/>移到顶部</Button>
        <Button disabled={!project} onClick={()=>show('settings')}><Settings2 size={14}/>项目设置</Button>
        <Button onClick={archive}><Archive size={14}/>归档工作区</Button>
      </ActionMenu>}/></ProjectWorkspace>:<div className="ide-empty"><FolderPlus size={30}/><h1>打开项目，开始工作</h1><Button onClick={chooseProject}>打开项目文件夹</Button></div>}
    </main>
    {pluginPage&&!appSettings&&<PluginsPage key={namespacedKey(target,'plugins')} onBack={()=>setPluginPage(false)} onSettings={()=>setAppSettings(true)}/>}
    <NativeDialog ref={statusDialog} aria-label="状态与资源"><div className="dialog-heading"><h2>状态与资源</h2><Button aria-label="关闭状态与资源" onClick={()=>statusDialog.current?.close()}><X size={16}/></Button></div><div className="dialog-body"><p>{workspace?.path||'尚未打开项目'}</p>{remote?<p>执行主机：{hostName}。资源监控、垃圾清理、浏览器和编排/自动化仅在本机可用。</p>:<ResourceStatus workspaceId={persisted&&!workspace?.archived?workspace?.id:undefined} workspaceName={workspace?.name} connected={connected} failures={connectionFailures} connectionError={connectionError} onOpenSession={(wid,id)=>{statusDialog.current?.close();setWorkspaceId(wid);setOrchestrationSession(id);setTaskPage(false);setAutomationPage(false);setOrchestrationPage(false);window.dispatchEvent(new CustomEvent('velella:open-session',{detail:{workspaceId:wid,sessionId:id}}))}}/>}</div></NativeDialog>
    <NativeDialog ref={helpDialog} aria-label="关于鱼群"><div className="dialog-heading"><h2>鱼群 Velella</h2><Button aria-label="关闭帮助" onClick={()=>helpDialog.current?.close()}><X size={16}/></Button></div><div className="dialog-body"><p>从工作台打开项目，在项目内使用终端、会话和文件。</p><p>文件菜单可管理项目与工作区，视图菜单可切换主题、执行主机和查看资源状态。</p></div></NativeDialog>
    {addingProject&&<AddProjectDialog remote={remote} hostName={hostName} onClose={()=>setAddingProject(false)} onCreate={()=>{setAddingProject(false);setComposerParent(null);setCreateThenWorkspace(false);setCreatingProject(true)}} onAdded={async id=>{revealProject(id);await refresh();setWorkspaceId('main:'+id);setSearch('');setTaskPage(false);setAutomationPage(false);setCollapsed(current=>{const next=new Set(current);next.delete(id);return next})}}/>}
    {creatingProject&&<ProjectCreator remote={remote} hostName={hostName} onClose={()=>{setCreatingProject(false);setCreateThenWorkspace(false)}} onCreated={async id=>{revealProject(id);const snapshot=await rpc({cmd:'workbench.snapshot'},currentTarget());accept(snapshot);setWorkspaceId('main:'+id);setSearch('');setTaskPage(false);setAutomationPage(false);setCollapsed(current=>{const next=new Set(current);next.delete(id);return next});if(createThenWorkspace){const created=snapshot.projects.find((p:Project)=>p.id===id);setComposerParent(null);setSourceItem(undefined);if(created)openComposer(created)}}}/>}
    {hostManagement&&<ExecutionHostsPanel onClose={()=>setHostManagement(false)}/>}
    {appSettings&&<AppSettings key={namespacedKey(target,'app-settings')} onNavigate={openSettingsDestination} onLogin={openAccountLogin} canLogin={!!workspace&&persisted&&!workspace.archived} onDirtyChange={dirty=>{settingsDirty.current=dirty}} notice={notificationError} onClose={()=>{if(navigation.canBack)navigation.go(-1);else setAppSettings(false)}}/>}
    {composer&&<WorkspaceComposer key={composer.id+':'+(sourceItem?.key||'')+':'+(composerParent||'')} project={composer} projects={projects} workspaces={rows} parentWorkspaceId={composerParent} sourceItem={sourceItem} remote={remote} hostName={hostName} onClose={()=>{setComposer(null);setComposerParent(null)}} onNewProject={()=>{setComposer(null);setComposerParent(null);setCreateThenWorkspace(true);setCreatingProject(true)}} onSources={()=>{setComposer(null);setComposerParent(null);setTaskPage(true);setAutomationPage(false);setSourceKey(undefined)}} onCreated={async (id,projectId)=>{await refresh();setWorkspaceId(id);setTaskPage(false);setAutomationPage(false);setCollapsed(current=>{const next=new Set(current);next.delete(projectId||composer.id);return next})}}/>}
    {capabilities&&workspace&&project&&<CapabilitiesPanel key={workspace.id} workspace={workspace} project={project} onClose={()=>setCapabilities(false)} onSelect={setWorkspaceId} onAutomation={()=>{setCapabilities(false);setTaskPage(false);setAutomationPage(true)}} onRefresh={refresh}/>}
    <NativeDialog ref={dialog} className="workspace-settings-dialog" aria-labelledby="workspace-edit-title"><form onSubmit={e=>{e.preventDefault();void submit()}}>
      <div className="dialog-heading"><h2 id="workspace-edit-title">{mode==='project'?'打开项目文件夹':mode==='settings'?'项目设置 · '+(projects.find(p=>p.id===dialogTarget.projectId)?.name||'项目不可用'):mode==='parent'?'更改父工作区':'重命名工作区'}</h2><Button type="button" aria-label="关闭" onClick={()=>dialog.current?.close()}><X size={16}/></Button></div>
      <div className="dialog-body">
        {mode==='settings'?<>
          <label>默认 Git 基准<Input autoFocus disabled={busy} value={defaultBase} onChange={e=>setDefaultBase(e.target.value)} placeholder="留空使用项目配置或只读推断" aria-describedby="workspace-base-help"/></label>
          <p id="workspace-base-help" className="workspace-settings-note">留空会清除显式默认值。创建时先读取项目配置，否则只读推断 origin/HEAD 或唯一的 main/master；没有有效且唯一的候选时将拒绝创建，不修改 Git 状态。</p>
          <fieldset className="workspace-setup-choice" disabled={busy} aria-describedby="workspace-setup-help"><legend>默认准备脚本执行方式</legend>
            <label><Input type="radio" name="workspace-setup-default" value="skip" checked={setupDefault==='skip'} onChange={()=>setSetupDefault('skip')}/>跳过准备脚本</label>
            <label><Input type="radio" name="workspace-setup-default" value="run" checked={setupDefault==='run'} onChange={()=>setSetupDefault('run')}/>运行准备脚本</label>
          </fieldset>
          <p id="workspace-setup-help" className="workspace-settings-note">仅保存默认选择，不立即执行。创建时仍可明确选择运行或跳过；旧项目未设置时默认跳过。</p>
          <label>准备脚本<Textarea rows={5} disabled={busy} value={text} spellCheck={false} onChange={e=>setText(e.target.value)}/></label>
        </>:mode==='parent'?<>
          <label>父工作区<select autoFocus disabled={busy||!editingWorkspace||workspaceIsRoot(editingWorkspace)} value={parentId} onChange={e=>setParentId(e.target.value)} aria-describedby="workspace-parent-help">
            <option value="">无父级（移除父级，作为顶级工作区）</option>
            {unavailableParent&&<option value={parentId} disabled>当前父级不可用，请重新选择</option>}
            {parentCandidates.map(parent=><option key={parent.id} value={parent.id}>{projects.find(p=>p.id===parent.project_id)?.name||parent.project_id} / {parent.name}{parent.archived?'（已归档）':''} · {parent.id}</option>)}
          </select></label>
          <p id="workspace-parent-help" className="workspace-settings-note">仅可选择同一主机的工作区，不能选择自身或后代。父级只用于组织，不继承 Git 基准、准备脚本、权限或文件，也不改变项目归属或任务依赖。跨项目父级将显示为链接；主工作区固定为顶级。</p>
        </>:<label>{mode==='project'?'文件夹路径':'工作区名称'}<Input autoFocus required disabled={busy} value={text} onChange={e=>setText(e.target.value)}/></label>}
        {error&&<p role="alert">{error}</p>}
      </div>
      <div className="dialog-footer"><Button type="button" onClick={()=>dialog.current?.close()}>取消</Button><Button type="submit" disabled={busy||mode==='parent'&&(!editingWorkspace||workspaceIsRoot(editingWorkspace)||unavailableParent)}>确定</Button></div>
    </form></NativeDialog>
  </div></ProjectContentsTarget.Provider></WorkspaceToolsTarget.Provider></ChromeTarget.Provider></TerminalSessionProvider>;
}
