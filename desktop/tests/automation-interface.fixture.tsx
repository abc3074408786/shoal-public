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
const sessions=[{id:'ui-session',kind:'terminal',program:'codex',title:'Codex',native_chat:false,task_id:null,execution_mode:'native',status:{exit:null,state:'waiting_input',input_protocol:1}}];
const defaultLayout:Layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:'tab-0',tabs:Array.from({length:5},(_,i)=>({id:`tab-${i}`,kind:'terminal',target:'ui-session',title:i===0?'Codex · 界面改进':`较长的会话标签 ${i}`}))}],closed:[]};
const empty:Layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]};
const layouts:Record<string,Layout>=JSON.parse(localStorage.getItem('fixture-layouts')||'null')||{'task:ui-task':defaultLayout,'ui-empty':empty};
const automations:any[]=[],runs:any[]=[];
let failNext=false;
const requests:Record<string,unknown>[]=[];
const tasks:any[]=params.has('task')?[{id:'ui-task',project_id:'ui-project',phase:'running',reason:'执行命令需要你授权。',verified:false,created_at:'2026-10-21',worktree:workspaces[0].path,branch,status:{state:'waiting_permission'},agent:null,command:null,shell:null,checks:[],spec:{name,agent:'codex',instruction:'完善主工作台界面',base:'main',policy:{budget:{wall_clock:'10m'}}}}]:[];
function seedConsole(){
 const spec={name:'OA 每日巡检',agent:'codex',base:'HEAD',instruction:'检查入口与关键工作流，整理发现与证据，等待人工审查。',done:[],policy:{budget:{wall_clock:'1h'}}};
 automations.splice(0,automations.length,
  {id:'automation-1',project_id:'ui-project',enabled:true,next_run:1792846800,interval_minutes:1440,spec},
  {id:'automation-2',project_id:'ui-project',enabled:true,next_run:1792850400,interval_minutes:10080,spec:{...spec,name:'每周开发进展与测试结果汇总'}},
  {id:'automation-3',project_id:'ui-project',enabled:false,next_run:1792843200,interval_minutes:60,last_error:'项目准备脚本已变化，需要重新确认配置。',spec:{...spec,name:'每小时项目检查'}});
 runs.splice(0,runs.length,
  {id:'run-1',automation_id:'automation-1',task_id:'ui-task',started_at:1792759000,state:'queued'},
  {id:'run-2',automation_id:'automation-2',task_id:'review-task',started_at:1792759100,state:'review'},
  {id:'run-3',automation_id:'automation-3',started_at:1792759200,state:'failed',error:'项目准备脚本已变化，需要重新确认配置。'});
 const task=tasks.find(t=>t.id==='ui-task');
 if(task){task.phase='running';task.reason='正在等待你确认一项终端权限请求。';task.checks=[{command:'npm run test',passed:false,output:'1 项测试失败：工作区名称未显示完整。'}]}
 if(!tasks.some(t=>t.id==='review-task'))tasks.push({id:'review-task',project_id:'ui-project',phase:'review',reason:'',verified:true,checks:[{command:'npm run test',passed:true,output:'26 checks passed.'}],spec:{...spec,name:'每周开发进展与测试结果汇总'}});
}
if(params.has('demo'))seedConsole();
function respond(r:any):any{
  requests.push(r);
  switch(r.cmd){
    case 'automation.list':if(failNext){failNext=false;throw Error('模拟连接失败')}return {automations,runs};
    case 'automation.save':{const a={...r.automation,id:r.automation.id||'automation-1'};const i=automations.findIndex(x=>x.id===a.id);if(i>=0)automations[i]=a;else automations.push(a);return {automation:a}}
    case 'automation.pause':automations.find(a=>a.id===r.id).enabled=false;return {};
    case 'automation.run':runs.push({id:'run-1',automation_id:r.id,task_id:'ui-task',started_at:1792530000,state:'running'});return {task_id:'ui-task'};
    case 'workbench.snapshot':return {projects:[{id:'ui-project',name:'Velella',path:'/fixture',setup:'',direct:false}],workspaces,tasks,source_links:[]};
    case 'workbench.layout':return {layout:layouts[r.workspace_id]};
    case 'workbench.layout.save':{const layout=structuredClone(r.layout);layout.revision++;layouts[r.workspace_id]=layout;localStorage.setItem('fixture-layouts',JSON.stringify(layouts));return {layout}};
    case 'workbench.sessions':return {sessions:r.workspace_id==='task:ui-task'?sessions:[]};
    case 'workbench.files':case 'task.files':return {files:['README.md','src/Workbench.tsx','src/WorkspaceApp.tsx','src/theme.css']};
    case 'workbench.git':return {branch,status:'',diff:'',staged:''};
    case 'workbench.terminal.output':{const bytes=new TextEncoder().encode('Codex · 界面验收（模拟会话）\r\n\r\n此页面使用本地模拟数据，没有运行命令或调用模型。\r\n\r\n> 完善工作台导航、标签和工具面板\r\n');const from=Math.min(bytes.length,Number(r.since)||0);return {from,end:bytes.length,total:bytes.length,data:btoa(String.fromCharCode(...bytes.slice(from))),geometry:[{offset:0,rows:24,cols:100,sequence:1}],geometry_error:null}};
    case 'workbench.terminal.resize':case 'workbench.terminal.input':case 'workbench.terminal.control':return {};
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
 Object.assign(window,{interfaceQA:{requests,layouts,sessions,automations,runs,tasks,seed:seedConsole,fail:()=>{failNext=true}}});

 createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
}
void start();
