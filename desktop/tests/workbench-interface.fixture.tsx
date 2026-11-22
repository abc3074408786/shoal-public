// Isolated UI fixture: no requests reach the daemon; no terminal process is started.
import React from 'react';
import {createRoot} from 'react-dom/client';
import type {Layout} from '../src/workbench-types';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/motion.css';
const params=new URLSearchParams(location.search);
const name='完善工作台导航与审查体验 · 长名称工作区';
const branch='codex/workbench-interface-refresh-long-branch-name';
const workspaces=[{id:'task:ui-task',project_id:'ui-project',name,path:'/fixture/workbench',kind:'worktree',position:0,archived:false,favorite:true},
  {id:'ui-empty',project_id:'ui-project',name:'准备开始的新工作区',path:'/fixture/empty',kind:'worktree',position:1,archived:false,favorite:false}];
const sessions=[{id:'ui-session',kind:'terminal',program:'codex',title:'Codex',native_chat:false,task_id:null,execution_mode:'native',status:{exit:null,state:'waiting_input',input_protocol:1,human_control:params.has('human-control')}}];
const defaultLayout:Layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:'tab-0',tabs:Array.from({length:5},(_,i)=>({id:`tab-${i}`,kind:'terminal',target:'ui-session',title:i===0?'Codex · 界面改进':`较长的会话标签 ${i}`}))}],closed:[]};
const empty:Layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]};
const layouts:Record<string,Layout>=JSON.parse(localStorage.getItem('fixture-layouts')||'null')||{'task:ui-task':defaultLayout,'ui-empty':empty};
const requests:Record<string,unknown>[]=[];
const tasks=params.has('task')?[{id:'ui-task',project_id:'ui-project',phase:'running',reason:'执行命令需要你授权。',verified:false,created_at:'2026-10-21',worktree:workspaces[0].path,branch,status:{state:'waiting_permission'},agent:null,command:null,shell:null,checks:[],spec:{name,agent:'codex',instruction:'完善主工作台界面',base:'main',policy:{budget:{wall_clock:'10m'}}}}]:[];
function respond(r:any):any{
  requests.push(r);
  switch(r.cmd){
    case 'orchestration.list':return {runs:[]};
    case 'workbench.snapshot':return {projects:[{id:'ui-project',name:'Velella',path:'/fixture',setup:'',direct:false}],workspaces,tasks,source_links:[]};
    case 'workbench.layout':return {layout:layouts[r.workspace_id]};
    case 'workbench.layout.save':{const layout=structuredClone(r.layout);layout.revision++;layouts[r.workspace_id]=layout;localStorage.setItem('fixture-layouts',JSON.stringify(layouts));return {layout}};
    case 'workbench.sessions':return {sessions:r.workspace_id==='task:ui-task'?sessions:[]};
    case 'workbench.files':case 'task.files':return {files:['README.md','src/Workbench.tsx','src/WorkspaceApp.tsx','src/theme.css']};
    case 'workbench.git':return {branch,status:'',diff:'',staged:''};
    case 'task.events':return {events:[{seq:1,ts:'2026-10-24T00:00:00Z',reason:'完成工具栏验证'}]};
    case 'task.diff':return {status:' M README.md',diff:'',staged:''};
    case 'workbench.terminal.activity':return {enabled:true,events:[],next:0,total:0};
    case 'workbench.terminal.output':{const bytes=new TextEncoder().encode('Codex · 界面验收（模拟会话）\r\n\r\n此页面使用本地模拟数据，没有运行命令或调用模型。\r\n\r\n> 完善工作台导航、标签和工具面板\r\n');const from=Math.min(bytes.length,Number(r.since)||0);return {from,end:bytes.length,total:bytes.length,data:btoa(String.fromCharCode(...bytes.slice(from))),geometry:[{offset:0,rows:24,cols:100,sequence:1}],geometry_error:null}};
    case 'workbench.terminal.control':sessions[0].status.human_control=r.human;return {};
    case 'workbench.terminal.resize':case 'workbench.terminal.input':return {};
    case 'sandbox.capabilities':return {sandbox:{available:false,reason:'UI fixture',restricted:{available:false,reason:'UI fixture'}}};
    default:throw Error(`Fixture refused RPC ${r.cmd}`);
  }
}
async function start(){
 if('__TAURI_INTERNALS__' in window)throw Error('Fixture must not use native RPC');
 if(!localStorage.getItem('velella:workspace'))localStorage.setItem('velella:workspace',JSON.stringify('task:ui-task').slice(1,-1));
 // Deterministic screenshots: avoid software WebGL canvas artifacts in headless Chrome.
 localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));
 if(params.has('theme'))localStorage.setItem('velella:ui:theme',JSON.stringify(params.get('theme')));
 window.fetch=async(_input,init)=>{try{return new Response(JSON.stringify({ok:true,...respond(JSON.parse(String(init?.body)))}))}catch(e){return new Response(JSON.stringify({ok:false,error:String(e)}))}};
 Object.assign(window,{interfaceQA:{requests,layouts,sessions}});

 createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
}
void start();
