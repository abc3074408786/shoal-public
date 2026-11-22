import {useConfirm} from './ConfirmAction';
import {ReactNode,useCallback,useEffect,useRef,useState} from 'react';
import {ArrowLeftRight,ChevronDown,Columns2,FilePlus2,FolderOpen,GitBranch,Globe,History,PanelRight,Plus,MoreHorizontal,RefreshCw,Search,Terminal,X} from 'lucide-react';
import {invoke} from './desktop-platform';
import {listen} from './desktop-platform';
import {confirm} from './desktop-platform';
import {rpc,isTauri,Task,phases} from './api';
import TaskTools from './TaskTools';
import {taskDisplayState} from './task-navigation';
import {workspaceTask,initialAgentSession} from './workspace-context';
import {useRetainedTerminals} from './TerminalSessions';
import TerminalSplitView from './TerminalSplitView';
import SplitSurface from './SplitSurface';
import {useNativeSplitDrop,useNativeSplitFocus} from './useNativeSplitDrop';
import * as ContextMenu from '@radix-ui/react-context-menu';
import WorkbenchFile from './WorkbenchFile';
import WorkbenchGit from './WorkbenchGit';
import PullRequestChecks from './PullRequestChecks';
import AgentChat from './AgentChat';
import {TopbarPortal,WorkspaceToolsPortal,ProjectContentsPortal} from './WindowChrome';
import {usePreference} from './Appearance';
import {clampPanelWidth} from './ui-preferences';
import ResizeHandle from './ResizeHandle';
import {Button,Input,Tip} from './ui';
import ActionMenu from './ActionMenu';
import WorkbenchToolbar from './WorkbenchToolbar';
import FileTree from './FileTree';
import LoadingState from './LoadingState';
import WorkspaceOverview from './WorkspaceOverview';
import TerminalLauncher from './TerminalLauncher';
import AgentTerminalLauncher from './AgentTerminalLauncher';
import HybridCodexLauncher,{HybridApprovals} from './HybridCodexLauncher';
import {terminalSessionTitle,terminalSessionStatus,terminalSessionIsLive,type TerminalExecutionMode} from './terminal-execution';
import BrowserPane from './BrowserPane';
import {installBrowserSurfaces} from './browser-surfaces';
import {useExecutionTarget} from './ExecutionHostsPanel';
import {currentTarget,sameTarget,type ExecutionTarget} from './execution-target';
import AgentLaunchItems from './AgentLaunchItems';
import {agentCatalog,agentPermission,type AgentId,type AgentPermissionMode} from './agent-preferences';
import {readAgentPreferences} from './useAgentPreferences';
import {permissionLaunchFields} from './agent-permission-launch';
import {AgentProgramIcon} from './AgentLogo';
import {Workspace,Session,Tab,Layout,emptyLayout,type SplitAxis,type SplitNode} from './workbench-types';
import {dropEdge,leaf,leafIds,MAX_SPLIT_LEAVES,moveTab,normalizeLayout,pruneEmptyPanes,removeLeaf,resizeSplit,splitLeaf,tabSessions,terminalTree,workspaceTree,type SplitEdge} from './split-layout';
import {annotationChatLayout} from './annotation-layout';
import {readAnnotations} from './annotation-drafts';
import type {DesktopTerminalSwitch} from './useDesktopTerminalSwitch';
import {sessionSurfaceTab} from './session-surface-tab';
import {defaultAccountHome,accountLaunchFields} from './account-preferences';
import {useWorkbenchPreferences} from './useWorkbenchPreferences';
import {shortcutMatches} from './workbench-preferences';
import SidebarSessionMenu from './SidebarSessionMenu';
import CliTaskMonitor from './CliTaskMonitor';
import SessionHistory from './SessionHistory';
import {partitionSessionHistory} from './session-history';

function TerminalActionsSlot({paneId,onTarget}:{paneId:string;onTarget:(id:string,target:HTMLDivElement|null)=>void}){
  const bind=useCallback((target:HTMLDivElement|null)=>onTarget(paneId,target),[paneId,onTarget]);
  return <div className="terminal-tab-actions" ref={bind}/>;
}

export default function Workbench({workspace:w,workspaceActions,initialSession,desktopSwitch,tasks=[],onTasksChanged=async()=>{},keyboardActive=true,compactChrome=false}:{workspace:Workspace;workspaceActions:ReactNode;initialSession?:string;desktopSwitch?:DesktopTerminalSwitch;tasks?:Task[];onTasksChanged?:()=>Promise<void>;keyboardActive?:boolean;compactChrome?:boolean}){
  const ask=useConfirm();
  const [hybridLauncher,setHybridLauncher]=useState(false);
  const preferences=useWorkbenchPreferences();
  // The embedded browser is a machine-local surface, so a remote workbench offers no browser tabs.
  const remoteHost=useExecutionTarget().kind==='ssh';
  const [terminalTargets,setTerminalTargets]=useState<Record<string,HTMLDivElement|null>>({});
  const bindTerminalTarget=useCallback((id:string,target:HTMLDivElement|null)=>setTerminalTargets(current=>current[id]===target?current:{...current,[id]:target}),[]);
  const [sessions,setSessions]=useState<Session[]>([]),[files,setFiles]=useState<string[]>([]),[filter,setFilter]=useState(''),[newPath,setNewPath]=useState(''),[matches,setMatches]=useState<{path:string;line:number;excerpt:string}[]>([]),[searched,setSearched]=useState(false),[layout,setLayout]=useState<Layout>(emptyLayout),[ready,setReady]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[paneId,setPaneId]=useState('main');
  const [branch,setBranch]=useState('');
  useEffect(()=>{let live=true;void rpc({cmd:'workbench.git',workspace_id:w.id}).then(r=>{if(live)setBranch(r.branch?.trim()||'')}).catch(()=>{});return()=>{live=false}},[w.id]);
  useRetainedTerminals(w.id,ready?layout.panes.flatMap(p=>p.tabs.flatMap(tabSessions)):null);
  const [splitSupported,setSplitSupported]=useState(false),[dropTarget,setDropTarget]=useState<{pane:string;edge?:SplitEdge}>();
  const [terminalFocus,setTerminalFocus]=useState<{tab:string;session:string;request:number}>();
  const [historyInitialSession,setHistoryInitialSession]=useState<string>();
  const closingTerminals=useRef(false);
  const splitLaunch=useRef<{tab:string;source:string;axis:SplitAxis;request:string;session?:string}|undefined>(undefined);
  const splitOpening=useRef(false);
  const splitContent=useRef<HTMLDivElement>(null);
  useNativeSplitFocus(splitContent,event=>{const pane=(event.target as Element)?.closest<HTMLElement>('[data-pane-id]');if(pane)setPaneId(pane.dataset.paneId!);});
  function workspaceDrop(event:DragEvent){const target=(event.target as Element)?.closest<HTMLElement>('[data-pane-id]');if(!target||!splitContent.current?.contains(target))return;const box=target.getBoundingClientRect();return {pane:target.dataset.paneId!,edge:splitSupported?dropEdge(event.clientX-box.left,event.clientY-box.top,box.width,box.height):undefined};}
  useNativeSplitDrop(splitContent,'text/velella-tab',{over:event=>{if(!busy)setDropTarget(workspaceDrop(event))},drop:event=>{const target=workspaceDrop(event);if(!busy&&target)move(event.dataTransfer!.getData('text/velella-tab'),target.pane,target.edge)},leave:()=>setDropTarget(undefined)});
  const activePane=layout.panes.find(p=>p.id===paneId)||layout.panes[0];
  const activeSession=sessions.find(s=>s.id===activePane.tabs.find(t=>t.id===activePane.active)?.target);
  const task=workspaceTask(w,tasks,activeSession);
  const [fileAction,setFileAction]=useState<'create'|'search'|null>(null),[sessionPicker,setSessionPicker]=useState(false);
  const [terminalLauncher,setTerminalLauncher]=useState(false);
  const [agentLauncher,setAgentLauncher]=useState<AgentId|null>(null);
  const launcherFocus=useRef<HTMLElement|null>(null);
  const workspaceTasks=tasks.filter(t=>t.worktree===w.path);
  const nativeBlockedReason=workspaceTasks.some(t=>!['queued','cancelled','delivered'].includes(t.phase))?'此工作区正在由任务使用。请先在任务中结束执行，再新建 Codex 终端。':'';
  const searchInput=useRef<HTMLInputElement>(null),createInput=useRef<HTMLInputElement>(null);
  const searchVersion=useRef(0);
  const openedSession=useRef<string|undefined>(undefined);
  const switchedRequest=useRef<string|undefined>(undefined);
  useEffect(()=>{
    if(!desktopSwitch||desktopSwitch.workspace_id!==w.id||!ready||busy||switchedRequest.current===desktopSwitch.request_id||Date.now()>=desktopSwitch.expires_at_ms||remoteHost)return;
    const session=sessions.find(s=>s.id===desktopSwitch.session_id);
    if(session){switchedRequest.current=desktopSwitch.request_id;openSession(session,true)}
  },[desktopSwitch,ready,busy,sessions,w.id,remoteHost]);
  const [requestedSession,setRequestedSession]=useState<string|undefined>();
  const [annotationSession,setAnnotationSession]=useState<string|undefined>();
  useEffect(()=>{const handler=(event:Event)=>{const d=(event as CustomEvent).detail;if(d?.workspaceId===w.id)setAnnotationSession(d.sessionId)};window.addEventListener('velella:annotation-chat',handler);return()=>window.removeEventListener('velella:annotation-chat',handler)},[w.id]);
  useEffect(()=>{if(!annotationSession||!ready||busy)return;const s=sessions.find(s=>s.id===annotationSession);if(!s)return;setAnnotationSession(undefined);const next=annotationChatLayout(layout,activePane.id,s);void save(next.layout).then(ok=>{if(ok)setPaneId(next.paneId)})},[annotationSession,ready,busy,sessions]);
  const [requestedBrowser,setRequestedBrowser]=useState<{id?:string;url?:string}|undefined>();
  useEffect(()=>{
    installBrowserSurfaces();
    const handler=(event:Event)=>{const d=(event as CustomEvent).detail;if(d?.workspaceId===w.id)setRequestedBrowser(d.page||{})};
    window.addEventListener('velella:open-browser',handler);
    const off=isTauri()?listen<any>('velella:browser-open',event=>{if(event.payload.workspace_id===w.id){const next=event.payload.layout as Layout|undefined;if(next){setLayout(current=>next.revision>=current.revision?next:current);setPaneId(next.panes.find(p=>p.tabs.some(t=>t.target===event.payload.id))?.id||'main')}else setRequestedBrowser(event.payload)}}):null;
    return()=>{window.removeEventListener('velella:open-browser',handler);void off?.then(fn=>fn())};
  },[w.id]);
  useEffect(()=>{if(!requestedBrowser||!ready||busy)return;const page=requestedBrowser;setRequestedBrowser(undefined);if(remoteHost)return;void openTab({id:crypto.randomUUID(),kind:'browser',target:page.id||`browser-draft-${crypto.randomUUID()}`,title:page.url?new URL(page.url).host:'浏览器'})},[requestedBrowser,ready,busy,remoteHost]);
  useEffect(()=>{const handler=(event:Event)=>{const detail=(event as CustomEvent<{workspaceId:string;sessionId:string}>).detail;if(detail?.workspaceId===w.id)setRequestedSession(detail.sessionId)};window.addEventListener('velella:open-session',handler);return()=>window.removeEventListener('velella:open-session',handler)},[w.id]);
  useEffect(()=>{if(!requestedSession||!ready||busy)return;const s=sessions.find(s=>s.id===requestedSession);if(s){setRequestedSession(undefined);openSession(s)}},[requestedSession,ready,busy,sessions]);
  useEffect(()=>{if(!initialSession||!ready||busy||openedSession.current===initialSession)return;const s=sessions.find(s=>s.id===initialSession);if(s){openedSession.current=initialSession;openSession(s)}},[initialSession,ready,busy,sessions]);
  const [filesVisible,setFilesVisible]=usePreference('velella:ui:right-open',false),[rightSize,setRightSize]=usePreference('velella:ui:right-width',300);
  const [rightTabValue,setRightTab]=usePreference<'files'|'git'|'checks'|'task'>('velella:ui:right-tab','files'),[viewport,setViewport]=useState(window.innerWidth);
  const rightTab=rightTabValue==='checks'?'checks':rightTabValue==='task'?'task':rightTabValue==='git'?'git':'files';
  useEffect(()=>{const resize=()=>setViewport(window.innerWidth);window.addEventListener('resize',resize);return()=>window.removeEventListener('resize',resize)},[]);
  const rightMax=Math.max(220,Math.min(480,viewport-680)),rightWidth=clampPanelWidth(Number(rightSize),220,rightMax);
  useEffect(()=>{if(!ready||busy)return;const session=initialAgentSession(layout,sessions,task);if(session)void openSession(session)},[ready,busy,task?.agent?.id,sessions.length]);
  function openGit(){openTab({id:crypto.randomUUID(),kind:'diff',target:'git',title:'Git 审查'})}
  function chatTerminal(){const existing=sessions.find(s=>s.program==='powershell'&&s.status?.exit===null);if(existing)openSession(existing);else void terminal()}

  useEffect(()=>{if(fileAction==='search')searchInput.current?.focus();if(fileAction==='create')createInput.current?.focus()},[fileAction,filesVisible]);
  function viewHistory(session?:Session){setHistoryInitialSession(session?.id);setSessionPicker(true)}
  function openFile(path:string){openTab({id:crypto.randomUUID(),kind:'files',target:path,title:path.replaceAll('\\','/').split('/').at(-1)!})}
  function newBrowser(){void openTab({id:crypto.randomUUID(),kind:'browser',target:`browser-draft-${crypto.randomUUID()}`,title:'浏览器'})}
  function openCodexLauncher(event:{currentTarget:HTMLElement}){const menu=event.currentTarget.closest('[role="menu"]');launcherFocus.current=menu?document.getElementById(menu.getAttribute('aria-labelledby')||''):event.currentTarget;setTerminalLauncher(true)}
  async function browserCreated(tabId:string,page:{id:string;url:string}){const next=structuredClone(layout),tab=next.panes.flatMap(p=>p.tabs).find(t=>t.id===tabId);if(!tab)return false;tab.target=page.id;tab.title=new URL(page.url).host;return save(next)}
  function openSession(s:Session,forceTerminal=false){let chat=s.kind==='agent';if(!forceTerminal)try{const drafts=readAnnotations(w.id);chat=chat||!!drafts.chats[s.id]||drafts.items.some(i=>i.sessionId===s.id)}catch{/* Storage errors are shown by the annotation panel. */}const tab=sessionSurfaceTab(layout,s,terminalSessionTitle(s),chat,forceTerminal);setTerminalFocus({tab:tab.id,session:s.id,request:Date.now()});void openTab(tab);setSessionPicker(false)}
  async function refreshSessions(target:ExecutionTarget=currentTarget()){const r=await rpc({cmd:'workbench.sessions',workspace_id:w.id},target);if(sameTarget(target,currentTarget()))setSessions(r.sessions)}
  async function refreshFiles(){try{const r=await rpc(task?{cmd:'task.files',task_id:task.id}:{cmd:'workbench.files',workspace_id:w.id});setFiles(r.files)}catch(e){setError(String(e))}}
  async function search(){if(busy)return;const version=++searchVersion.current;if(!filter.trim()){setMatches([]);setSearched(false);return}try{setBusy(true);const r=await rpc({cmd:'workbench.search',workspace_id:w.id,query:filter});if(version!==searchVersion.current)return;setMatches(r.matches);setSearched(true);setError(r.truncated?'搜索结果已按文件读取预算或 200 条上限截断。':'')}catch(e){if(version===searchVersion.current)setError(String(e))}finally{setBusy(false)}}
  async function createFile(){const path=newPath.trim();if(!path)return;try{setBusy(true);await rpc({cmd:'workbench.file.create',workspace_id:w.id,path});await refreshFiles();setNewPath('');setError('');await openTab({id:crypto.randomUUID(),kind:'files',target:path,title:path.split('/').at(-1)!});setFileAction(null)}catch(e){setError(String(e))}finally{setBusy(false)}}
  useEffect(()=>{let live=true,timer:ReturnType<typeof setTimeout>;async function load(){try{const [r,f]=await Promise.all([rpc({cmd:'workbench.layout',workspace_id:w.id}),rpc(task?{cmd:'task.files',task_id:task.id}:{cmd:'workbench.files',workspace_id:w.id})]);if(live){setLayout(current=>r.layout.revision>=current.revision?r.layout:current);setSplitSupported(r.split_layout_version===1);setFiles(f.files);setReady(true)}}catch(e){if(live)setError(String(e))}}async function poll(){try{const r=await rpc({cmd:'workbench.sessions',workspace_id:w.id});if(live)setSessions(r.sessions)}catch(e){if(live)setError(String(e))}if(live)timer=setTimeout(poll,2000)}void load();void poll();return()=>{live=false;clearTimeout(timer)}},[w.id]);
  useEffect(()=>{const changed=(event:Event)=>{const detail=(event as CustomEvent<{kind:string;path:string;newPath?:string}>).detail;if(!detail)return;const next=structuredClone(layout);for(const pane of next.panes){if(detail.kind==='rename')for(const tab of pane.tabs)if(tab.kind==='files'&&tab.target===detail.path){tab.target=detail.newPath!;tab.title=detail.newPath!.split('/').at(-1)!}if(detail.kind==='delete'){pane.tabs=pane.tabs.filter(tab=>!(tab.kind==='files'&&tab.target===detail.path));if(pane.active&&!pane.tabs.some(tab=>tab.id===pane.active))pane.active=pane.tabs[0]?.id||null}}void save(next);void refreshFiles()};window.addEventListener('velella:file-changed',changed);return()=>window.removeEventListener('velella:file-changed',changed)},[layout]);
  async function save(next:Layout):Promise<boolean>{if(busy||!ready)return false;setBusy(true);try{const r=await rpc({cmd:'workbench.layout.save',workspace_id:w.id,layout:splitSupported?normalizeLayout(next):next});setLayout(r.layout);setError('');return true}catch(e){setError(String(e));try{const r=await rpc({cmd:'workbench.layout',workspace_id:w.id});setLayout(r.layout)}catch{/* Keep current layout visible while offline. */}return false}finally{setBusy(false)}}
  function openTab(tab:Tab){const existing=layout.panes.find(p=>p.tabs.some(t=>t.kind===tab.kind&&t.target===tab.target));const next=structuredClone(layout),pane=next.panes.find(p=>p.id===(existing?.id||activePane.id))!;const old=pane.tabs.find(t=>t.kind===tab.kind&&t.target===tab.target);if(!old)pane.tabs.push(tab);pane.active=old?.id||tab.id;setPaneId(pane.id);return save(next)}
  const agentLaunches=useRef(new Map<string,{requestId:string;sessionId?:string;accountHome?:string}>()),agentOpening=useRef(false);
  async function launchAgent(id:AgentId){const target=currentTarget();try{await agentTerminal(id,agentPermission(readAgentPreferences(target),id),target)}catch(error){setError(String(error))}}
  const agentLaunchItems=<AgentLaunchItems blockedReason={nativeBlockedReason} onLaunch={id=>{if(id==='terminal')void terminal();else void launchAgent(id)}} onConfigure={(id,event)=>{if(id==='codex')openCodexLauncher(event);else{const menu=event.currentTarget.closest('[role="menu"]');launcherFocus.current=menu?document.getElementById(menu.getAttribute('aria-labelledby')||''):event.currentTarget;setAgentLauncher(id)}}}/>;
  async function agentTerminal(id:AgentId,mode:AgentPermissionMode,target:ExecutionTarget){
    if(busy||agentOpening.current||nativeBlockedReason)throw new Error(nativeBlockedReason||'工作区正在处理其他操作，请稍后继续。');
    agentOpening.current=true;setBusy(true);
    const agent=agentCatalog.find(agent=>agent.id===id)!;
    const key=JSON.stringify([target,w.id,id,mode]);
    try{
      if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，请从原主机继续。');
      if(!agentLaunches.current.has(key))agentLaunches.current.set(key,{requestId:crypto.randomUUID(),accountHome:defaultAccountHome(id,target)});
      const launch=agentLaunches.current.get(key)!;
      if(!launch.sessionId){const fields=await permissionLaunchFields(mode,target),account=await accountLaunchFields(launch.accountHome,target);if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，未创建终端。');const r=await rpc({cmd:'workbench.terminal.open',workspace_id:w.id,launch:id,request_id:launch.requestId,...fields,...account},target);launch.sessionId=r.session_id;}
      if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，会话保留在原主机。');
      await codexOpened(launch.sessionId!,'native',agent.program,target);agentLaunches.current.delete(key);
    }catch(e){throw new Error(`${agent.name} 终端打开未完成：${String(e)}。继续会沿用同一请求，也可从“已有会话”查看。`)}
    finally{agentOpening.current=false;setBusy(false)}
  }
  async function codexOpened(sessionId:string,mode:TerminalExecutionMode,program='codex',target:ExecutionTarget=currentTarget()){
    setBusy(true);
    try{
      // A failed save may already have persisted. Merge with the latest revision on every retry.
      const [current,listed]=await Promise.all([rpc({cmd:'workbench.layout',workspace_id:w.id},target),rpc({cmd:'workbench.sessions',workspace_id:w.id},target)]);
      const session=(listed.sessions as Session[]).find(s=>s.id===sessionId);
      if(!session)throw new Error('暂时找不到已创建的终端。请继续打开，或在已有会话中查看。');
      if(session.kind!=='terminal'||session.native_chat||session.program!==program||(session.execution_mode==='restricted')!==(mode==='restricted'))throw new Error('后台会话与所选执行模式不一致。请关闭窗口，在已有会话中查看记录。');
      const next=structuredClone(current.layout) as Layout;const kind='terminal';
      const pane=next.panes.find(p=>p.tabs.some(t=>t.kind===kind&&t.target===sessionId))||next.panes.find(p=>p.id===activePane.id)||next.panes[0];
      if(!pane)throw new Error('工作区布局尚未就绪，请继续打开。');
      let tab=pane.tabs.find(t=>t.kind===kind&&t.target===sessionId);
      if(!tab){tab={id:crypto.randomUUID(),kind,target:sessionId,title:terminalSessionTitle(session)};pane.tabs.push(tab)}
      pane.active=tab.id;
      if(!sameTarget(target,currentTarget()))throw new Error('执行主机已切换，请从原主机的已有会话中继续打开。');
      const saved=await rpc({cmd:'workbench.layout.save',workspace_id:w.id,layout:next},target);
      if(!sameTarget(target,currentTarget()))return;
      setSessions(listed.sessions);setLayout(saved.layout);setPaneId(pane.id);
      setError(!session.status?'会话已保留，终端尚未就绪。可在“已有会话”中查看状态。':session.status.exit!=null?(session.recoverable?'终端已结束，可通过终端菜单重新启动并恢复会话。':'终端已结束，文件和日志已保留。'):'');
    }finally{setBusy(false)}
  }
  async function terminal(){
    if(busy)return;
    const target=currentTarget();setBusy(true);
    try{
      let r:any;
      if(task){
        await rpc({cmd:'terminal.shell',task_id:task.id},target);
        if(!sameTarget(target,currentTarget()))return;
        const snapshot=await rpc({cmd:'snapshot'},target);
        if(!sameTarget(target,currentTarget()))return;
        const current=snapshot.tasks.find((t:Task)=>t.id===task.id);r={session_id:current?.shell?.id};
        if(!r.session_id)throw new Error('Shell 尚未就绪');
        if(layout.panes.some(p=>p.tabs.some(t=>t.target===r.session_id))){
          const existing=sessions.find(s=>s.id===r.session_id);
          if(existing){
            const next=structuredClone(layout),pane=next.panes.find(p=>p.tabs.some(t=>t.target===r.session_id))!;
            pane.active=pane.tabs.find(t=>t.target===r.session_id)!.id;
            const saved=await rpc({cmd:'workbench.layout.save',workspace_id:w.id,layout:next},target);
            if(!sameTarget(target,currentTarget()))return;
            setPaneId(pane.id);setLayout(saved.layout);
          }
          return;
        }
      }else r=await rpc({cmd:'workbench.terminal.open',workspace_id:w.id},target);
      if(!sameTarget(target,currentTarget()))return;
      const listed=await rpc({cmd:'workbench.sessions',workspace_id:w.id},target);
      if(!sameTarget(target,currentTarget()))return;
      const session=(listed.sessions as Session[]).find(session=>session.id===r.session_id);
      const next=structuredClone(layout),pane=next.panes.find(p=>p.id===activePane.id)!;
      pane.tabs.push({id:crypto.randomUUID(),kind:'terminal',target:r.session_id,title:session?terminalSessionTitle(session):'Shell'});pane.active=pane.tabs.at(-1)!.id;
      const saved=await rpc({cmd:'workbench.layout.save',workspace_id:w.id,layout:next},target);
      if(!sameTarget(target,currentTarget()))return;
      setSessions(listed.sessions);setLayout(saved.layout);setError('');
    }catch(e){if(sameTarget(target,currentTarget()))setError(String(e))}finally{setBusy(false)}
  }
  async function retireTerminals(ids:string[]){
    if(busy||!ready||closingTerminals.current)return false;
    const target=currentTarget(),records=[...new Set(ids)].map(id=>sessions.find(s=>s.id===id));
    if(records.some(s=>!s)){setError('终端状态尚未读取完整，请稍后再关闭。');return false}
    const pending=records.filter((s):s is Session=>!!s&&s.kind==='terminal'&&s.status?.exit==null);
    if(!pending.length)return true;
    closingTerminals.current=true;
    try{
      const message=pending.some(s=>s.task_id)?'此终端属于受管任务。关闭会停止该任务的全部执行进程，历史记录保留。继续关闭？':`关闭将停止 ${pending.length} 个终端中的程序，历史记录保留。继续关闭？`;
      if(!(await ask(message))||!sameTarget(target,currentTarget()))return false;
      setBusy(true);const stopped=new Set<string>();
      for(const session of pending){
        if(!sameTarget(target,currentTarget()))return false;
        const owner=session.task_id||session.id;if(stopped.has(owner))continue;
        await rpc({cmd:'workbench.terminal.stop',workspace_id:w.id,session_id:session.id,confirmed:true},target);stopped.add(owner);
      }
      await refreshSessions(target);return sameTarget(target,currentTarget());
    }catch(e){if(sameTarget(target,currentTarget()))setError(String(e));return false}
    finally{closingTerminals.current=false;setBusy(false)}
  }
  async function close(pid:string,id:string,stopTerminals=true){const next=structuredClone(layout),p=next.panes.find(p=>p.id===pid);const tab=p?.tabs.find(t=>t.id===id);if(!p||!tab)return;if(stopTerminals&&tab.kind==='terminal'&&!(await retireTerminals(tabSessions(tab))))return;p.tabs=p.tabs.filter(t=>t.id!==id);if(p.active===id)p.active=p.tabs[0]?.id||null;next.closed=[tab,...next.closed].slice(0,30);if(await save(pruneEmptyPanes(next))){if(tab.kind==='browser'&&!tab.target.startsWith('browser-draft-')&&isTauri())try{await invoke('browser_command',{request:{op:'close',workspace_id:w.id,id:tab.target},target:currentTarget()})}catch(e){setError(String(e))}}}
  function restore(){
    const next=structuredClone(layout),tab=next.closed.shift();if(!tab)return;
    const opened=next.panes.flatMap(p=>p.tabs.flatMap(tabSessions));
    if(tab.kind==='terminal'){
      let tree:SplitNode|undefined=terminalTree(tab);for(const id of opened)if(tree)tree=removeLeaf(tree,id);
      if(tree){tab.terminalTree=tree;tab.target=leafIds(tree)[0];}else{const existing=next.panes.find(p=>p.tabs.some(t=>tabSessions(t).includes(tab.target)));if(existing){existing.active=existing.tabs.find(t=>tabSessions(t).includes(tab.target))!.id;setPaneId(existing.id);void save(next);return;}}
    }
    const existing=next.panes.find(p=>p.tabs.some(t=>t.kind===tab.kind&&t.target===tab.target));const p=existing||next.panes.find(p=>p.id===activePane.id)!;if(existing)p.active=p.tabs.find(t=>t.kind===tab.kind&&t.target===tab.target)!.id;else{tab.id=crypto.randomUUID();p.tabs.push(tab);p.active=tab.id}setPaneId(p.id);void save(next);
  }
  function move(tabId:string,to:string,edge?:SplitEdge){if(edge&&!splitSupported)return;const moved=moveTab(layout,tabId,to,edge);if(!moved)return;setPaneId(moved.paneId);void save(moved.layout)}
  function splitGroup(pid:string,axis:SplitAxis){
    if(!splitSupported||busy||layout.panes.length>=MAX_SPLIT_LEAVES)return;
    const next=normalizeLayout(structuredClone(layout)),id=crypto.randomUUID();
    next.panes.push({id,active:null,tabs:[]});next.tree=splitLeaf(next.tree!,pid,leaf(id),axis==='horizontal'?'right':'bottom');setPaneId(id);void save(next);
  }
  function mergeGroup(pid:string){const next=structuredClone(layout),from=next.panes.find(p=>p.id===pid),to=next.panes.find(p=>p.id!==pid);if(!from||!to)return;to.tabs.push(...from.tabs);to.active=from.active||to.active;next.panes=next.panes.filter(p=>p!==from);setPaneId(to.id);void save(normalizeLayout(next));}
  function split(){const next=structuredClone(layout);if(next.panes.length===1){if(splitSupported){splitGroup(activePane.id,'horizontal');return;}next.panes.push({id:crypto.randomUUID(),tabs:[],active:null});}else{next.panes[0].tabs.push(...next.panes.slice(1).flatMap(p=>p.tabs));next.panes.splice(1);next.tree=leaf(next.panes[0].id);setPaneId(next.panes[0].id)}void save(next)}
  function resizeWorkspace(id:string,ratio:number,axis?:SplitAxis){const tree=resizeSplit(workspaceTree(layout),id,ratio,axis);void save({...layout,tree,axis:tree.kind==='split'?tree.axis:layout.axis,ratio:tree.kind==='split'?tree.ratio:layout.ratio});}
  function updateTerminalTree(tabId:string,tree:SplitNode){const next=structuredClone(layout),tab=next.panes.flatMap(p=>p.tabs).find(t=>t.id===tabId);if(!tab)return;tab.terminalTree=tree;if(!leafIds(tree).includes(tab.target)){tab.target=leafIds(tree)[0];const s=sessions.find(s=>s.id===tab.target);tab.title=s?terminalSessionTitle(s):'终端';}void save(next);}
  async function closeTerminalPane(pid:string,tabId:string,sid:string){
    const next=structuredClone(layout),tab=next.panes.flatMap(p=>p.tabs).find(t=>t.id===tabId);if(!tab)return;
    const tree=removeLeaf(terminalTree(tab),sid);if(!tree){await close(pid,tabId);return;}if(!(await retireTerminals([sid])))return;
    const s=sessions.find(s=>s.id===sid);next.closed=[{id:crypto.randomUUID(),kind:'terminal' as const,target:sid,title:s?terminalSessionTitle(s):'终端'},...next.closed].slice(0,30);
    tab.terminalTree=tree;if(tab.target===sid){tab.target=leafIds(tree)[0];const remaining=sessions.find(s=>s.id===tab.target);tab.title=remaining?terminalSessionTitle(remaining):'终端';}void save(next);
  }
  async function splitTerminal(tabId:string,source:string,axis:SplitAxis){
    if(!ready||busy||splitOpening.current||!splitSupported)return;
    const original=layout.panes.flatMap(p=>p.tabs).find(t=>t.id===tabId);if(!original||leafIds(terminalTree(original)).length>=MAX_SPLIT_LEAVES)return;
    splitOpening.current=true;setBusy(true);
    try{
      if(!splitLaunch.current||splitLaunch.current.tab!==tabId||splitLaunch.current.source!==source||splitLaunch.current.axis!==axis)splitLaunch.current={tab:tabId,source,axis,request:crypto.randomUUID()};
      const launch=splitLaunch.current!;
      if(!launch.session){const r=await rpc({cmd:'workbench.terminal.open',workspace_id:w.id,request_id:launch.request});launch.session=r.session_id;}
      const [current,listed]=await Promise.all([rpc({cmd:'workbench.layout',workspace_id:w.id}),rpc({cmd:'workbench.sessions',workspace_id:w.id})]);
      const next=normalizeLayout(current.layout),pane=next.panes.find(p=>p.tabs.some(t=>t.id===tabId)),tab=pane?.tabs.find(t=>t.id===tabId);
      if(!tab||!pane)throw new Error('原标签已关闭，新终端可从已有会话中打开');
      const tree=terminalTree(tab),sid=launch.session!;
      if(!leafIds(tree).includes(sid)){
        if(!leafIds(tree).includes(source))throw new Error('原分屏已关闭，新终端可从已有会话中打开');
        tab.terminalTree=splitLeaf(tree,source,leaf(sid),axis==='horizontal'?'right':'bottom');
      }
      pane.active=tabId;const saved=await rpc({cmd:'workbench.layout.save',workspace_id:w.id,layout:next});
      setSessions(listed.sessions);setLayout(saved.layout);setPaneId(pane.id);setTerminalFocus({tab:tabId,session:sid,request:Date.now()});setError('');splitLaunch.current=undefined;
    }catch(e){setError(`拆分未完成：${String(e)}。再次点击相同拆分操作会继续打开同一终端。`);try{const [current,listed]=await Promise.all([rpc({cmd:'workbench.layout',workspace_id:w.id}),rpc({cmd:'workbench.sessions',workspace_id:w.id})]);setLayout(current.layout);setSessions(listed.sessions)}catch{/* Keep the current view while disconnected. */}}
    finally{splitOpening.current=false;setBusy(false)}
  }
  async function stop(s:Session){const target=currentTarget(),msg=s.task_id?'停止此任务的全部执行进程？文件和日志会保留。':'停止此终端中的程序？文件和日志会保留。';if(!(await ask(msg))||!sameTarget(target,currentTarget()))return;try{await rpc({cmd:'workbench.terminal.stop',workspace_id:w.id,session_id:s.id,confirmed:true},target);await refreshSessions(target)}catch(e){if(sameTarget(target,currentTarget()))setError(String(e))}}
  async function recover(s:Session){const target=currentTarget();try{setBusy(true);await rpc({cmd:'workbench.terminal.recover',workspace_id:w.id,session_id:s.id},target);const listed=await rpc({cmd:'workbench.sessions',workspace_id:w.id},target);if(!sameTarget(target,currentTarget()))return;setSessions(listed.sessions);setError('');return (listed.sessions as Session[]).find(session=>session.id===s.id)}catch(e){if(sameTarget(target,currentTarget()))setError(String(e))}finally{setBusy(false)}}
  async function resumeHistory(s:Session){if(terminalSessionIsLive(s)){openSession(s);return}const restored=await recover(s);if(restored)openSession(restored)}
  function sidebarSessionLocation(s:Session){for(const pane of layout.panes){const tab=pane.tabs.find(t=>tabSessions(t).includes(s.id));if(tab)return {pane,tab}}}
  function closeSidebarSession(s:Session){const location=sidebarSessionLocation(s);if(!location)return;if(location.tab.kind==='terminal')closeTerminalPane(location.pane.id,location.tab.id,s.id);else void close(location.pane.id,location.tab.id)}
  useEffect(()=>{const handler=(e:KeyboardEvent)=>{if(!keyboardActive||e.defaultPrevented||e.isComposing||document.querySelector('dialog[open],.app-settings-page'))return;if(shortcutMatches('restoreTab',e,preferences)){e.preventDefault();restore();return}const nextTab=shortcutMatches('nextTab',e,preferences),previousTab=shortcutMatches('previousTab',e,preferences);if(nextTab||previousTab){e.preventDefault();const p=activePane;if(!p.tabs.length)return;const i=p.tabs.findIndex(t=>t.id===p.active),next=structuredClone(layout);next.panes.find(x=>x.id===p.id)!.active=p.tabs[(i+(previousTab?-1:1)+p.tabs.length)%p.tabs.length].id;void save(next)}};window.addEventListener('keydown',handler);return()=>window.removeEventListener('keydown',handler)},[layout,paneId,busy,ready,keyboardActive,preferences]);
  const activeFile=activePane.tabs.find(t=>t.id===activePane.active&&t.kind==='files')?.target;
  const compactDocument=layout.panes.length===1&&!!activeFile&&/\.(md|markdown|mdown)$/i.test(activeFile)&&!task&&!branch;
  const visibleFiles=files.filter(f=>f.toLowerCase().includes(filter.toLowerCase()));
  const activeTab=activePane.tabs.find(t=>t.id===activePane.active);
  const selectedSession=terminalFocus&&terminalFocus.tab===activeTab?.id?terminalFocus.session:activeTab?.target;
  const otherTabs=layout.panes.flatMap(p=>p.tabs).filter(t=>!['terminal','agent'].includes(t.kind));
  const {current:currentSessions,history:historySessions}=partitionSessionHistory(sessions,layout.panes.flatMap(p=>p.tabs.flatMap(tabSessions)));
  const newTabMenu=<ActionMenu label="新建标签" disabled={!ready||busy} icon={<Plus size={16}/>}>
    <div className="wb-menu-label">终端与会话</div>
    {agentLaunchItems}
    {!remoteHost&&<Button disabled={!!nativeBlockedReason} onClick={()=>setHybridLauncher(true)}><Terminal size={16}/>远程 Codex · 本地项目</Button>}
    <Button onClick={()=>viewHistory()}><History size={16}/>历史会话（{historySessions.length}）</Button>
    <div className="wb-menu-divider"/><div className="wb-menu-label">{remoteHost?'文件':'文件与浏览器'}</div>
    <Button disabled={!!task} onClick={()=>{setFilesVisible(true);setRightTab('files');setFileAction('create')}}><FilePlus2 size={16}/>新建文件</Button>
    {!remoteHost&&<Button onClick={newBrowser}><Globe size={16}/>新建浏览器</Button>}
    <div className="wb-menu-divider"/><div className="wb-menu-label">审查</div>
    <Button onClick={openGit}><GitBranch size={16}/>Git 审查</Button>
  </ActionMenu>;
  function tabs(p:typeof activePane){return <div className="ide-tabs" data-tauri-drag-region role="tablist" aria-label="工作区标签">{p.tabs.map(t=><ContextMenu.Root key={t.id}><ContextMenu.Trigger asChild><div className={`ide-tab ${p.active===t.id?'selected':''}`} draggable={!busy} onDragStart={e=>{e.dataTransfer.setData('text/velella-tab',t.id);e.dataTransfer.effectAllowed='move'}} onDragEnd={()=>setDropTarget(undefined)}>
    <Button role="tab" title={t.title} aria-selected={p.active===t.id} disabled={busy} onClick={()=>{setPaneId(p.id);void save({...layout,panes:layout.panes.map(x=>x.id===p.id?{...x,active:t.id}:x)})}}>{t.kind==='terminal'?<AgentProgramIcon program={sessions.find(s=>s.id===t.target)?.program} size={14}/>:t.kind==='browser'?<Globe size={13}/>:t.kind==='diff'?<GitBranch size={13}/>:<FolderOpen size={13}/>}<span>{t.title}</span></Button>
    {layout.panes.length===2&&<Button disabled={busy} aria-label={`移动 ${t.title} 到另一分屏`} title="移动到另一分栏" onClick={()=>move(t.id,layout.panes.find(x=>x.id!==p.id)!.id)}><ArrowLeftRight size={12}/></Button>}
    <Button disabled={busy} aria-label={`关闭标签 ${t.title}`} title={t.kind==='terminal'?'关闭终端并停止进程，历史记录保留':'关闭标签'} onClick={()=>void close(p.id,t.id)}><X size={12}/></Button>
  </div></ContextMenu.Trigger><ContextMenu.Portal><ContextMenu.Content className="wb-menu-panel">
    <ContextMenu.Item className="ui-button" disabled={!splitSupported||busy||layout.panes.length>=MAX_SPLIT_LEAVES||p.tabs.length===1} onSelect={()=>move(t.id,p.id,'right')}>移到右侧新分栏</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" disabled={!splitSupported||busy||layout.panes.length>=MAX_SPLIT_LEAVES||p.tabs.length===1} onSelect={()=>move(t.id,p.id,'bottom')}>移到下方新分栏</ContextMenu.Item>
    {layout.panes.filter(x=>x.id!==p.id).map(dest=><ContextMenu.Item key={dest.id} className="ui-button" disabled={busy} onSelect={()=>move(t.id,dest.id)}>移到分栏 {layout.panes.indexOf(dest)+1}</ContextMenu.Item>)}
    <ContextMenu.Item className="ui-button" disabled={busy} onSelect={()=>void close(p.id,t.id)}>{t.kind==='terminal'?'关闭终端（停止进程）':'关闭标签'}</ContextMenu.Item>
    {t.kind==='terminal'&&<ContextMenu.Item className="ui-button" disabled={busy} onSelect={()=>void close(p.id,t.id,false)}>隐藏标签（保持运行）</ContextMenu.Item>}
  </ContextMenu.Content></ContextMenu.Portal></ContextMenu.Root>)}</div>}
  function renderPane(id:string){const p=layout.panes.find(p=>p.id===id);if(!p)return null;return <section className={`wb-pane ${activePane.id===p.id?'focused':''}`} data-pane-id={p.id} aria-label={layout.panes.length>1?`分屏 ${layout.panes.indexOf(p)+1}`:'工作区内容'} onFocusCapture={()=>setPaneId(p.id)} onPointerDownCapture={()=>setPaneId(p.id)}>
    {p.tabs.filter(t=>t.id===p.active).map(t=>sessions.find(s=>s.id===t.target&&s.hybrid)?<HybridApprovals key={t.target} workspaceId={w.id} sessionId={t.target}/>:null)}
    {layout.panes.length>1&&<div className="wb-tabbar">{tabs(p)}<TerminalActionsSlot paneId={p.id} onTarget={bindTerminalTarget}/><div className="wb-pane-tools"><ActionMenu label={`分栏 ${layout.panes.indexOf(p)+1} 操作`} disabled={busy} icon={<Columns2 size={14}/>}><Button disabled={!splitSupported||layout.panes.length>=MAX_SPLIT_LEAVES} onClick={()=>splitGroup(p.id,'horizontal')}>向右新建分栏</Button><Button disabled={!splitSupported||layout.panes.length>=MAX_SPLIT_LEAVES} onClick={()=>splitGroup(p.id,'vertical')}>向下新建分栏</Button><Button onClick={()=>mergeGroup(p.id)}>合并当前分栏</Button></ActionMenu></div></div>}
    {p.tabs.filter(t=>t.id===p.active).map(t=>t.kind==='browser'?<BrowserPane key={t.id} workspaceId={w.id} pageId={t.target} onCreated={page=>browserCreated(t.id,page)}/>:t.kind==='files'?<WorkbenchFile key={t.id} workspaceId={w.id} path={t.target} task={task}/>:t.kind==='diff'?(task?<TaskTools key={t.id} task={task} view="diff" onChanged={onTasksChanged}/>:<WorkbenchGit key={t.id} workspaceId={w.id} variant="diff"/>):(t.kind==='agent'||sessions.find(s=>s.id===t.target)?.native_chat)?<AgentChat actionsTarget={terminalTargets[p.id]} key={t.id} workspaceId={w.id} session={sessions.find(s=>s.id===t.target)} onTerminal={chatTerminal}/>:<TerminalSplitView key={t.id} workspace={w} tab={t} sessions={sessions} actionsTarget={terminalTargets[p.id]} focusSession={terminalFocus?.tab===t.id?terminalFocus:undefined} disabled={busy||!splitSupported} onSplit={(sid,axis)=>void splitTerminal(t.id,sid,axis)} onTree={tree=>updateTerminalTree(t.id,tree)} onClose={sid=>closeTerminalPane(p.id,t.id,sid)} onStop={s=>void stop(s)} onRecover={s=>void recover(s)} onReconnected={refreshSessions}/>)}
    {!p.tabs.length&&(layout.panes.length>1?<div className="wb-empty-split"><p>拖动标签到这里，或打开一个会话</p><Button disabled={!ready||busy} onClick={terminal}><Terminal size={15}/>新建终端</Button><Button disabled={!ready||busy} onClick={()=>viewHistory()}>历史会话</Button></div>:<WorkspaceOverview name={w.name} files={files} sessions={historySessions} currentSessions={currentSessions} ready={ready} busy={busy} onTerminal={terminal} agentItems={agentLaunchItems} onBrowser={remoteHost?undefined:newBrowser} onGit={openGit} onRestore={restore} canRestore={!!layout.closed.length} nativeBlockedReason={nativeBlockedReason} onCodexTerminal={()=>void launchAgent('codex')} onFile={openFile} onSession={viewHistory} onSessions={()=>viewHistory()} onBrowse={()=>{setFilesVisible(true);setRightTab('files');setFileAction('search');requestAnimationFrame(()=>searchInput.current?.focus())}} onCreate={task?undefined:()=>{setFilesVisible(true);setRightTab('files');setFileAction('create')}}/>)}
    {dropTarget?.pane===p.id&&<div className="split-drop-hint" data-edge={dropTarget.edge}>{dropTarget.edge?'松开创建分栏':'松开移入此分栏'}</div>}
  </section>}
  return <div className={'wb-body'+(compactDocument?' document-focused':'')} style={{gridTemplateColumns:filesVisible?`minmax(0,1fr) ${rightWidth}px`:'minmax(0,1fr)'}}>
    <ProjectContentsPortal>
      <div className="sidebar-content-heading"><span>当前终端</span><ActionMenu label="在项目中新建终端" disabled={!ready||busy} icon={<Plus size={14}/>}>
        {agentLaunchItems}
        <div className="wb-menu-divider"/>
        <Button onClick={()=>viewHistory()}>历史会话</Button>
        <Button disabled={!layout.closed.length} onClick={restore}>恢复关闭的标签</Button>
        {!remoteHost&&<Button onClick={newBrowser}>新建浏览器</Button>}
        <Button onClick={openGit}>Git 审查</Button>
      </ActionMenu></div>
      <ul className="sidebar-content-list" aria-label="项目终端">{currentSessions.map(s=><li key={s.id}><SidebarSessionMenu session={s} disabled={!ready||busy} onOpen={()=>openSession(s)} onClose={sidebarSessionLocation(s)?()=>closeSidebarSession(s):undefined} onRecover={()=>void recover(s)} onStop={()=>void stop(s)}><Button className="sidebar-content-item" disabled={!ready||busy} aria-current={selectedSession===s.id?'true':undefined} title={terminalSessionTitle(s)+' · '+terminalSessionStatus(s)} onClick={()=>openSession(s)}><AgentProgramIcon program={s.program} size={14}/><span>{terminalSessionTitle(s)}</span></Button></SidebarSessionMenu></li>)}</ul>
      {!currentSessions.length&&(ready?<p className="sidebar-content-empty">当前未打开终端</p>:<LoadingState compact label="正在读取终端" className="sidebar-content-empty"/>)}
      {!!otherTabs.length&&<><div className="sidebar-content-heading"><span>其他</span></div><ul className="sidebar-content-list" aria-label="项目其他内容">{otherTabs.map(t=><li key={t.id}><Button className="sidebar-content-item" disabled={!ready||busy} aria-current={activeTab?.id===t.id?'true':undefined} title={t.title} onClick={()=>void openTab(t)}>{t.kind==='browser'?<Globe size={13}/>:t.kind==='diff'?<GitBranch size={13}/>:<FolderOpen size={13}/>}<span>{t.title}</span></Button></li>)}</ul></>}
    </ProjectContentsPortal>
    <TopbarPortal>
      <div className="workspace-tab-navigation" role="group" aria-label="标签与新建">
        {layout.panes.length===1&&activePane.tabs.length?tabs(activePane):<div className="workspace-empty-tabs"><Terminal size={14}/><span>{w.name}</span><small>{layout.panes.length>1?'分屏工作区':'暂无打开标签'}</small></div>}
        {newTabMenu}
      </div>
    </TopbarPortal>
    <WorkspaceToolsPortal>
      <WorkbenchToolbar compact={compactChrome} hasTabs={layout.panes.some(p=>p.tabs.length)}>
        <Tip label="会话记录"><Button className="workspace-session-history" aria-label="会话记录" title="查看当前工作区的历史记录" disabled={!ready||busy} onClick={()=>viewHistory()}><History size={16}/></Button></Tip>
        {layout.panes.length===1&&<TerminalActionsSlot paneId={activePane.id} onTarget={bindTerminalTarget}/>}
        {task&&<Tip label={task.reason||'查看任务详情'}><Button className="workspace-task-indicator" data-state={taskDisplayState(task)} onClick={()=>{setFilesVisible(true);setRightTab('task')}}>{taskDisplayState(task)==='waiting_permission'?'等待授权':phases[task.phase]}</Button></Tip>}
        {(task?.branch||branch)&&<ActionMenu label="分支与更改" icon={<GitBranch size={16}/>}><div className="wb-menu-label wb-menu-branch">{task?.branch||branch}</div><Button onClick={()=>{setFilesVisible(true);setRightTab('git')}}>查看更改</Button><Button onClick={openGit}>Git 审查</Button></ActionMenu>}
        <ActionMenu label="布局与标签" disabled={!ready||busy} icon={<Columns2 size={16}/>}><Button onClick={split}>{layout.panes.length===1?'分屏':'合并分屏'}</Button><Button disabled={!splitSupported||layout.panes.length>=MAX_SPLIT_LEAVES} onClick={()=>splitGroup(activePane.id,'horizontal')}>向右新建分栏</Button><Button disabled={!splitSupported||layout.panes.length>=MAX_SPLIT_LEAVES} onClick={()=>splitGroup(activePane.id,'vertical')}>向下新建分栏</Button>{layout.panes.length>1&&<><Button onClick={()=>resizeWorkspace(workspaceTree(layout).id,layout.ratio,layout.axis==='horizontal'?'vertical':'horizontal')}>切换分屏方向</Button><Button onClick={()=>resizeWorkspace(workspaceTree(layout).id,.5)}>均分空间</Button><Button onClick={()=>resizeWorkspace(workspaceTree(layout).id,.65)}>扩大第一个分屏</Button><Button onClick={()=>resizeWorkspace(workspaceTree(layout).id,.35)}>扩大第二个分屏</Button></>}<div className="wb-menu-divider"/><Button disabled={!layout.closed.length} onClick={restore}>恢复关闭的标签</Button></ActionMenu>

        <Tip label={filesVisible?'收起工具栏':'展开工具栏'}><Button className="workspace-tools-toggle" aria-label={filesVisible?'收起工具栏':'展开工具栏'} aria-expanded={filesVisible} onClick={()=>setFilesVisible(!filesVisible)}><PanelRight size={16}/></Button></Tip>
        {workspaceActions}
      </WorkbenchToolbar>
    </WorkspaceToolsPortal>
    <div className="wb-content" ref={splitContent}>
      {error&&<div role="alert" className="ide-error"><span>{error}</span><Button aria-label="关闭工作台错误" onClick={()=>setError('')}><X size={14}/></Button></div>}
      <div className={`wb-panes nested-panes ${layout.axis} ${layout.panes.length>1?'is-split':''}`}>
        <SplitSurface tree={workspaceTree(layout)} renderLeaf={renderPane} disabled={busy||!splitSupported} onResize={resizeWorkspace} label="工作台"/>
      </div>
    </div>
    {filesVisible&&<aside className="wb-files" aria-label="工作区工具">
      <ResizeHandle side="right" width={rightWidth} min={220} max={rightMax} onChange={setRightSize}/>
      <div className="right-panel-tabs cli-monitor-tabs" role="tablist" aria-label="工作区工具视图"><Button role="tab" aria-selected={rightTab==='files'} onClick={()=>setRightTab('files')}>文件</Button><Button role="tab" aria-selected={rightTab==='git'} onClick={()=>setRightTab('git')}>更改</Button><Button role="tab" aria-selected={rightTab==='checks'} onClick={()=>setRightTab('checks')}>检查</Button><Button role="tab" aria-selected={rightTab==='task'} onClick={()=>setRightTab('task')}>任务</Button><Tip label="收起工具栏"><Button aria-label="关闭右侧工具栏" onClick={()=>setFilesVisible(false)}><PanelRight size={14}/></Button></Tip></div>
      {rightTab==='checks'?<PullRequestChecks key={w.id} workspaceId={w.id}/>:rightTab==='task'?<div className="cli-task-panel"><CliTaskMonitor workspaceId={w.id} sessions={currentSessions} tasks={tasks} activeSessionId={activeSession?.id} onOpen={openSession} disabled={!ready||busy}/>{task&&<details className="cli-task-details"><summary>受管任务详情</summary><TaskTools task={task} view="info" onChanged={onTasksChanged}/></details>}</div>:rightTab==='git'?(task?<TaskTools task={task} view="changes" onChanged={onTasksChanged} onOpenDiff={openGit}/>:<WorkbenchGit workspaceId={w.id} variant="sidebar" onOpenDiff={openGit}/>):<>
        <div className="wb-files-heading"><strong title={w.name}>{w.name}</strong><div>
          <Tip label={task?'任务中的文件创建由 Agent 处理':'新建文件'}><Button disabled={!!task} aria-label="新建文件" aria-expanded={fileAction==='create'} onClick={()=>setFileAction(fileAction==='create'?null:'create')}><FilePlus2 size={14}/></Button></Tip>
          <Tip label="搜索文件"><Button aria-label="搜索文件" aria-expanded={fileAction==='search'} onClick={()=>setFileAction(fileAction==='search'?null:'search')}><Search size={14}/></Button></Tip>
          <Tip label="刷新文件"><Button aria-label="刷新文件" onClick={refreshFiles}><RefreshCw size={13}/></Button></Tip>
        </div></div>
      {fileAction==='create'&&<form className="wb-file-form" onSubmit={e=>{e.preventDefault();void createFile()}}>
        <label>新建文件<Input ref={createInput} aria-label="新建文件路径" value={newPath} onChange={e=>setNewPath(e.target.value)} placeholder="例如 notes.md"/></label>
        <div><Button type="button" onClick={()=>setFileAction(null)}>取消</Button><Button disabled={!ready||busy||!newPath.trim()}>创建</Button></div>
      </form>}
      {fileAction==='search'&&<form className="wb-file-form" onSubmit={e=>{e.preventDefault();void search()}}>
        <label>筛选文件<Input ref={searchInput} aria-label="筛选或搜索文件" value={filter} onChange={e=>{searchVersion.current++;setFilter(e.target.value);setSearched(false)}} placeholder="输入文件名或内容"/></label>
        <div><Button type="button" onClick={()=>{searchVersion.current++;setFilter('');setSearched(false);setFileAction(null)}}>清除</Button><Button disabled={!ready||busy||!filter.trim()}>搜索内容</Button></div>
      </form>}
      {filter&&fileAction!=='search'&&<Button className="wb-filter-summary" onClick={()=>setFileAction('search')}><Search size={13}/><span>筛选：{filter}</span></Button>}
      <div className="wb-file-list">
        {searched?<>{matches.map(m=><Button disabled={!ready||busy} className="wb-search-result" key={`${m.path}:${m.line}`} title={m.excerpt} onClick={()=>openFile(m.path)}><strong>{m.path}:{m.line}</strong><small>{m.excerpt}</small></Button>)}{!matches.length&&<p className="wb-search-empty">没有文本匹配</p>}</>:
          <><FileTree key={filter} files={visibleFiles} selected={activeFile} disabled={!ready||busy} onOpen={openFile} expanded={!!filter}/>{!visibleFiles.length&&(ready?<p className="wb-search-empty">{filter?'没有匹配的文件':'还没有文件，点击上方新建。'}</p>:<LoadingState compact label="正在读取文件" className="wb-search-empty"/>)}</>}
      </div>
      </>}
    </aside>}
    {terminalLauncher&&<TerminalLauncher workspace={w} taskOwned={workspaceTasks.length>0} nativeBlockedReason={nativeBlockedReason} returnFocusTo={launcherFocus.current} onClose={()=>setTerminalLauncher(false)} onOpened={codexOpened}/>}
    {agentLauncher&&<AgentTerminalLauncher agent={agentLauncher} workspaceName={w.name} returnFocusTo={launcherFocus.current} onClose={()=>setAgentLauncher(null)} onLaunch={agentTerminal}/>}
    {hybridLauncher&&<HybridCodexLauncher workspaceId={w.id} onClose={()=>setHybridLauncher(false)} onOpened={id=>codexOpened(id,'native','codex')}/>}
    {sessionPicker&&<SessionHistory workspace={w} sessions={historySessions} openCount={currentSessions.length} busy={busy} error={error} initialSession={historyInitialSession} onClose={()=>{setSessionPicker(false);setHistoryInitialSession(undefined)}} onResume={s=>void resumeHistory(s)}/>}
  </div>
}
