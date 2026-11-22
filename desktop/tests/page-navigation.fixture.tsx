import React from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {TooltipProvider} from '../src/ui';
import {selectTarget} from '../src/execution-target';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';import '../src/motion.css';

// No daemon, filesystem, credentials or terminal process is used by this fixture.
const projects=['A','B','C'].map(id=>({id,name:'项目 '+id,path:'D:/synthetic/'+id,setup:''}));
const workspaces=projects.flatMap(p=>[0,1].map(i=>({id:p.id+'-'+i,project_id:p.id,name:p.id+' 工作区 '+i,path:p.path+'/'+i,kind:i?'worktree':'primary',archived:false,favorite:false,position:i})));
const requests:any[]=[];
window.fetch=async(_input,init)=>{
  const r=JSON.parse(String(init?.body));requests.push(r);
  switch(r.cmd){
    case 'workbench.snapshot':return Response.json({ok:true,projects,workspaces,tasks:[],source_links:[]});
    case 'protocol':return Response.json({ok:true,capabilities:[]});
    case 'cli.inspect':return Response.json({ok:true,found:false});
    case 'orchestration.capabilities':return Response.json({ok:true,providers:[]});
    case 'orchestration.templates.list':return Response.json({ok:true,templates:[]});
    default:throw Error('Fixture refused RPC '+r.cmd);
  }
};
window.velellaDesktop={invoke:async(command,args:any)=>{
  if(command==='rpc')return (await window.fetch('',{body:JSON.stringify(args.request)})).json();
  if(command==='execution_hosts')return {ok:true,hosts:[],connections:[],profiles:[]} as any;
  if(command==='window'||command==='hud')return false as any;
  throw Error('Fixture refused desktop command '+command);
},listen:()=>()=>{}};
Object.assign(window,{navigationQA:{requests,selectTarget}});
localStorage.removeItem('velella:workspace');
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp projectSurface={({workspace})=><output data-workspace={workspace.id}>{workspace.name}</output>}/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
