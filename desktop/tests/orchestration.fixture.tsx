import React from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';
const workspace={id:'fixture',project_id:'p',name:'研发测试工作区',path:'D:\\fixture',kind:'folder',position:0,archived:false,favorite:false};
const nodes=[{id:'inspect',title:'定位输入延迟',instruction:'只读分析输入路径，返回源码位置。',role:'scout',depends_on:[],scope:'终端输入模块',checks:['返回 file:line 和根因']},{id:'fix',title:'修复并验证',instruction:'修复确认的问题并运行回归测试。',role:'lead',depends_on:['inspect'],scope:'终端输入模块',checks:['输入测试通过']}];
const requests:any[]=[];let run:any=null;
function respond(r:any){requests.push(r);switch(r.cmd){
case 'workbench.snapshot':return {projects:[{id:'p',name:'测试项目',path:'D:\\fixture',setup:''}],workspaces:[workspace],tasks:[],source_links:[]};
case 'workbench.layout':return {layout:{revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]}};
case 'workbench.files':return {files:[]};case 'workbench.sessions':return {sessions:[]};case 'workbench.git':return {branch:'main',status:'',diff:'',staged:''};
case 'orchestration.list':return {runs:run?[run]:[]};
case 'orchestration.snapshot':return {run:structuredClone(run)};
case 'orchestration.create':run={id:r.request_id,goal:r.goal,workspace_id:r.workspace_id,state:'draft',agent:r.agent,scout_agent:r.scout_agent,lead_session:'lead',revision:1,plan:{acceptance:r.acceptance,parallelism:1,nodes:structuredClone(nodes)},events:[],status:{members:[{id:'lead',name:'主 Agent',state:'joined',identity:{session_id:'lead'}}],work:[],reviews:[],attempts:[]}};return {id:run.id};
case 'orchestration.plan.save':if(r.revision!==run.revision)throw Error('stale revision');run.plan=structuredClone(r.plan);run.revision++;return {id:run.id};
case 'orchestration.plan.confirm':if(r.revision!==run.revision)throw Error('stale revision');run.state='running';run.nodes=[...run.plan.nodes,{id:'__final',title:'整体验证与交付',role:'lead',depends_on:run.plan.nodes.map((n:any)=>n.id),scope:'整体',instruction:'验证整体目标',checks:[run.plan.acceptance]}];return {id:run.id};
case 'orchestration.pause':run.state='paused';return {id:run.id};case 'orchestration.resume':run.state='running';run.blocker=null;return {id:run.id};case 'orchestration.stop':run.state='stopped';return {id:run.id};
case 'orchestration.accept':if(run.state!=='awaiting_acceptance')throw Error('not ready');run.state='accepted';run.events.push({at:new Date().toISOString(),kind:'accepted',text:r.summary});return {id:run.id};
default:throw Error('Unexpected fixture RPC '+r.cmd);
}}
if('__TAURI_INTERNALS__' in window)throw Error('Fixture must not run in native profile');
localStorage.clear();localStorage.setItem('velella:workspace','fixture');localStorage.setItem('velella:ui:theme',JSON.stringify('light'));
window.fetch=async(_input,init)=>{try{return new Response(JSON.stringify({ok:true,...respond(JSON.parse(String(init?.body)))}))}catch(e){return new Response(JSON.stringify({ok:false,error:String(e)}))}};
Object.assign(window,{orchestrationFixture:{requests,block(){run.blocker='等待原生授权，请打开会话处理'},finish(){run.state='awaiting_acceptance';run.blocker=null;run.status.work=run.nodes.map((n:any,i:number)=>({id:n.id,work_key:n.id,title:n.title,instruction:n.instruction,state:'accepted',assignee:'lead',attempt_id:'a'+i,attempt_count:1,write:n.role==='lead'}));run.status.attempts=run.nodes.map((n:any,i:number)=>({id:'a'+i,report:{summary:n.title+'：实际检查记录（测试数据）',outcome:'completed'}}));run.final_candidate={revision:'fixture-source-fingerprint'};}}});
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
