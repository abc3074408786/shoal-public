import React,{useEffect,useState} from 'react';
import {createRoot} from 'react-dom/client';
import * as monaco from 'monaco-editor';
import WorkspaceApp from '../src/WorkspaceApp';
import WorkbenchFile from '../src/WorkbenchFile';
import {AppearanceProvider} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {TooltipProvider} from '../src/ui';
import {useWorkbenchPreferences} from '../src/useWorkbenchPreferences';
import {selectTarget,currentTarget} from '../src/execution-target';
import {agentCatalog} from '../src/agent-preferences';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';import '../src/project-shell.css';import '../src/orca-light.css';import '../src/motion.css';
// Synthetic RPCs only. No real runtime, provider, credential file or model calls.
const project={id:'settings-fixture',name:'设置验收项目',path:'D:/synthetic/project',setup:''};
const workspace={id:'main:settings-fixture',project_id:project.id,name:'设置验收',path:project.path,kind:'primary',archived:false,favorite:false,position:0};
const fixture=(window as any).__fixture={requests:[],notifications:[],phase:'running',snapshotOk:true,content:'const answer = 42;\n',writes:0,failSave:false,capabilities:true,connections:[],selectTarget,currentTarget,editor:()=>monaco.editor.getEditors()[0],editorFontSize:()=>monaco.editor.getEditors()[0]?.getOption(monaco.editor.EditorOption.fontSize),handlers:{}};
Object.assign(fixture,{probeStarts:[],probeActive:0,probeMaximum:0,holdProbes:!!(window as any).__kbProbeHold,probeReleases:[],failNextProbe:false,expectedAgents:agentCatalog.filter(agent=>agent.check).map(agent=>agent.id),releaseProbes:()=>{fixture.holdProbes=false;fixture.probeReleases.splice(0).forEach((release:()=>void)=>release())}});
const reply=(value:object)=>({ok:true,...value});
async function rpc(request:any,target:any={kind:'local'}){fixture.requests.push(request);
  switch(request.cmd){
    case 'protocol':return reply({capabilities:['terminal-permissions-v1',...(fixture.capabilities?['terminal-accounts-v1']:[])]});
    case 'workbench.snapshot':{
      if(!fixture.snapshotOk)throw Error('Synthetic disconnect');
      const snapshot=reply({projects:[{...project,name:target.kind==='ssh'?'远端设置验收项目':project.name}],workspaces:[workspace],tasks:[{id:target.kind==='ssh'?'remote-task-a':'task-a',project_id:project.id,phase:fixture.phase,worktree:project.path,reason:'',status:null,agent:null,shell:null,command:null,spec:{name:'合成任务',agent:'codex',instruction:'',policy:{},checks:[]}}],source_links:[]});
      if(fixture.delayNextSnapshot){fixture.delayNextSnapshot=false;return new Promise(resolve=>{fixture.releaseSnapshot=()=>resolve(snapshot)})}
      return snapshot;
    }
    case 'agent.check':{
      fixture.probeStarts.push({request,target});fixture.probeActive++;fixture.probeMaximum=Math.max(fixture.probeMaximum,fixture.probeActive);
      try{
        if(fixture.holdProbes)await new Promise<void>(resolve=>fixture.probeReleases.push(resolve));
        if(fixture.failNextProbe){fixture.failNextProbe=false;throw Error('Synthetic CLI probe failed')}
        return reply({available:true,authenticated:!request.account_home||request.account_home.includes('work'),version:'synthetic-only'});
      }finally{fixture.probeActive--}
    }
    case 'orchestration.capabilities':return reply({providers:[{id:'codex',installed:true}]});
    case 'orchestration.templates.list':return reply({templates:[]});
    case 'cli.inspect':return reply({found:true,path:'D:/synthetic/velella-cli.exe',source:'path'});
    case 'workbench.file.read':return reply({content:fixture.content});
    case 'workbench.file.write':fixture.writes++;if(fixture.failSave||request.previous!==fixture.content)return {ok:false,error:'合成文件冲突：磁盘版本已变化'};fixture.content=request.content;return reply({});
    case 'ssh.list':return reply({profiles:[]});
    case 'workbench.terminal.open':return reply({session_id:request.request_id});
    default:throw Error('Unimplemented synthetic RPC: '+request.cmd);
  }
}
window.velellaDesktop={invoke:async(command,args:any)=>{
  if(command==='rpc')return rpc(args.request,args.target) as any;
  if(command==='notify'){fixture.notifications.push(args.options);return undefined as any}
  if(command==='window')return false as any;
  if(command==='execution_hosts')return reply({hosts:[],connections:fixture.connections,profiles:[]}) as any;
  throw Error('Unimplemented synthetic desktop command: '+command);
},listen:(event,handler)=>{fixture.handlers[event]=handler;return()=>{delete fixture.handlers[event]}}};
localStorage.setItem('velella:workspace',workspace.id);
if(!localStorage.getItem('velella:ui:theme'))localStorage.setItem('velella:ui:theme',JSON.stringify('light'));
function Project(){const [draft,setDraft]=useState('保留的项目草稿'),preferences=useWorkbenchPreferences();return <div style={{height:'100%',display:'flex',flexDirection:'column'}}><label>项目草稿<input aria-label="项目草稿" value={draft} onChange={e=>setDraft(e.target.value)}/></label><output aria-label="编辑器偏好">{preferences.editor.fontSize} / {preferences.editor.wordWrap}</output><WorkbenchFile workspaceId={workspace.id} path="answer.ts"/></div>}
function App(){useEffect(()=>{const timer=setTimeout(()=>window.dispatchEvent(new Event('velella:open-settings')),30);return()=>clearTimeout(timer)},[]);return <WorkspaceApp projectSurface={context=>{fixture.refresh=context.onRefresh;fixture.snapshotProject=context.projects[0]?.name;return <Project/>}}/>}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><App/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
