// Isolated interaction fixture. Synthetic workspace data; no daemon or external RPC.
import {useState} from 'react';
import {createRoot} from 'react-dom/client';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {TooltipProvider} from '../src/ui';
import ProjectSidebar from '../src/ProjectSidebar';
import {useProjectLibrary} from '../src/useProjectLibrary';
import type {Project,Task} from '../src/api';
import type {Workspace} from '../src/workbench-types';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/workspace.css';
import '../src/workbench-a.css';
import '../src/project-shell.css';
import '../src/orca-light.css';
import '../src/project-orca.css';
const realFetch=window.fetch.bind(window);
window.fetch=async(input,init)=>{
  if(String(input).includes('/_velella')){const request=JSON.parse(String(init?.body||'{}'));return new Response(JSON.stringify(request.cmd==='workbench.git'?{ok:true,branch:request.workspace_id==='main:drive'?'main':'feature/preview'}:{ok:false,error:'Preview blocks runtime operations'}),{headers:{'Content-Type':'application/json'}})}
  return realFetch(input,init);
};
const projects:Project[]=[{id:'fish',name:'鱼群 Velella',path:'/preview/fish',setup:''},{id:'drive',name:'哼哼 OA',path:'/preview/drive',setup:''}];
const row=(id:string,project_id:string,name:string,extra:Partial<Workspace>={}):Workspace=>({id,project_id,name,path:'/preview/'+id,kind:'worktree',archived:false,favorite:false,position:0,...extra});
const initial=[row('main:fish','fish','主工作区',{kind:'primary'}),row('task:pr','fish','界面优化',{position:1,favorite:true}),row('sleep','fish','旧版布局',{position:2}),row('child','fish','搜索优化',{position:3,parent_workspace_id:'sleep'}),row('main:drive','drive','主工作区',{kind:'primary'}),row('review','drive','云盘验收',{position:1})];
const tasks:Task[]=[{id:'pr',project_id:'fish',phase:'review',reason:'',verified:false,created_at:'2026-11-04',worktree:'/preview/task:pr',branch:'feature/preview',native_id:null,agent:null,command:null,shell:null,pr_url:'https://github.com/example/fish/pull/1',checks:[],status:null,spec:{name:'界面优化',agent:'codex',instruction:'',base:'main',policy:{budget:{wall_clock:'1m'}}}}];
for(const host of ['A','B']){const key='fixture:workspace-options:'+host;if(!localStorage.getItem(key))localStorage.setItem(key,JSON.stringify({version:1,groups:[],projects:{},workspaces:{sleep:{sleeping:true},review:{status:'in_review'},'task:pr':{status:'in_progress',unread:true}}}))}
function Preview(){const [host,setHost]=useState('A'),[rows,setRows]=useState(initial),[active,setActive]=useState(initial[1]),[error,setError]=useState(''),[lastAction,setLastAction]=useState(''),[dark,setDark]=useState(true);
  const library=useProjectLibrary('fixture:workspace-options:'+host,setError,()=>true);
  const action=(text:string)=>setLastAction(text);
  return <div className="ide-shell project-shell options-preview" data-shell-page="project"><header className="app-titlebar"><strong>鱼群 · 工作区选项预览</strong><button onClick={()=>{setDark(!dark);document.documentElement.dataset.theme=dark?'light':'dark'}}>切换明暗</button><button onClick={()=>setHost(host==='A'?'B':'A')}>切换预览主机</button><span>主机 {host}</span></header><aside className="ide-sidebar" aria-label="项目导航"><ProjectSidebar key={host} projects={projects} workspaces={rows} tasks={tasks} workspace={active} disabled={false} library={library} hostLabel={'预览主机 '+host} remote={false} onHosts={()=>action('管理执行主机')} workspaceActions={{onOpen:w=>{setActive(w);action('打开 '+w.name)},onEdit:w=>action('编辑 '+w.name),onUpdate:(w,fields)=>setRows(current=>current.map(r=>r.id===w.id?{...r,...fields}:r)),onSleep:w=>{library.editWorkspace(w.id,{sleeping:true})},onArchive:w=>action('归档 '+w.name),onFolder:w=>action('目录 '+w.path),onError:setError}} onProject={p=>{setActive(rows.find(w=>w.project_id===p.id)!);action('打开项目 '+p.name)}} onWorkspace={setActive} onAdd={()=>action('添加项目')} onHome={()=>action('项目首页')} onSettings={()=>action('设置')} onContents={()=>{}} onNewWorkspace={p=>action('新建工作区 · '+p.name)} onProjectSettings={p=>action('项目设置 · '+p.name)} onRemove={p=>action('移除项目 · '+p.name)}/></aside><main><h1>工作区，按你的习惯排列</h1><p>从侧栏的显示选项切换项目范围、分组、排序与布局。<br/>选择会保留，切换主机时各自独立。</p><p>当前工作区：<strong>{active.name}</strong></p><p role="status">{lastAction}</p>{error&&<p role="alert">{error}</p>}<p>这是虚构数据预览，不会创建工作区或启动终端。</p></main></div>;
}
createRoot(document.getElementById('root')!).render(<TooltipProvider><ConfirmationProvider><Preview/></ConfirmationProvider></TooltipProvider>);
