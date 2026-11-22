import React,{useEffect,useState} from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {TooltipProvider} from '../src/ui';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/project-shell.css';
import '../src/orca-light.css';
import '../src/motion.css';

// This isolated fixture never contacts a real daemon or changes native preferences.
const project={id:'settings-preview',name:'Velella 界面预览',path:'D:/preview-only/velella',setup:''};
const workspace={id:'main:settings-preview',project_id:project.id,name:'界面设计',path:project.path,kind:'primary',archived:false,favorite:false,position:0};
window.fetch=async(_input,init)=>{
  const request=JSON.parse(String(init?.body||'{}'));
  if(request.cmd==='cli.inspect'){
    const mode=new URLSearchParams(location.search).get('cli');
    if(mode==='unsupported')return new Response(JSON.stringify({ok:false,error:'Synthetic older backend'}));
    return new Response(JSON.stringify({ok:true,found:mode!=='missing',path:mode==='missing'?null:'C:/preview-only/Programs/Velella CLI/velella-cli.exe',source:mode==='missing'?null:'installation'}));
  }
  if(request.cmd==='workbench.snapshot')return new Response(JSON.stringify({ok:true,projects:[project],workspaces:[workspace],tasks:[],source_links:[]}),{headers:{'Content-Type':'application/json'}});
  if(request.cmd==='orchestration.capabilities')return new Response(JSON.stringify({ok:true,providers:['codex','claude-code','omp','opencode'].map(id=>({id,installed:true,tools:[],skills:true,native_validation:'not_verified'})),skills:[]}),{headers:{'Content-Type':'application/json'}});
  if(request.cmd==='orchestration.templates.list')return new Response(JSON.stringify({ok:true,templates:new URLSearchParams(location.search).has('no-templates')?[]:[{id:'fixture-review-workflow',name:'研发与审查 · 样例',revision:1,plan:{version:2,acceptance:'样例验收',parallelism:2,roles:['coordinate','explore','implement','review','integrate','verify'].map(id=>({id,name:id,kind:id,provider:'codex',model:'',instruction:'样例角色',skills:[],tools:[],mcp_servers:[]})),nodes:[{id:'build'},{id:'review'}],review_role:'review',integration_role:'integrate',verification_role:'verify'}}]}),{headers:{'Content-Type':'application/json'}});
  if(request.cmd==='orchestration.list')return new Response(JSON.stringify({ok:true,runs:[]}),{headers:{'Content-Type':'application/json'}});
  if(request.cmd==='orchestration.preflight')return new Response(JSON.stringify({ok:true,workspace_id:request.workspace_id,ready:true,issues:[]}),{headers:{'Content-Type':'application/json'}});
  if(request.cmd==='protocol')return new Response(JSON.stringify({ok:true,capabilities:new URLSearchParams(location.search).has('old-host')?[]:['terminal-permissions-v1']}),{headers:{'Content-Type':'application/json'}});
  if(request.cmd!=='agent.check')throw new Error('Fixture only allows synthetic agent.check');
  const failed=new URLSearchParams(location.search).get('failure')===request.agent;
  return new Response(JSON.stringify(failed?{ok:false,error:'Synthetic detection failure'}:{ok:true,available:true,version:'fixture-only'}),{headers:{'Content-Type':'application/json'}});
};
document.documentElement.dataset.theme=new URLSearchParams(location.search).get('theme')==='dark'?'dark':'light';
localStorage.setItem('velella:workspace',workspace.id);
localStorage.setItem('velella:ui:theme',JSON.stringify(document.documentElement.dataset.theme));
function ProjectPreview(){
  const [draft,setDraft]=useState('返回项目后，这段草稿会保留。');
  return <section style={{padding:32}} aria-label="隔离项目预览"><h1>Velella 界面预览</h1><p>此页面使用真实导航和设置组件，检测结果为样例，不连接真实终端。</p><label>项目草稿<input aria-label="项目草稿" value={draft} onChange={event=>setDraft(event.target.value)}/></label></section>;
}
function Preview(){
  useEffect(()=>{const frame=requestAnimationFrame(()=>window.dispatchEvent(new Event('velella:open-settings')));return()=>cancelAnimationFrame(frame)},[]);
  return <WorkspaceApp projectSurface={()=> <ProjectPreview/>}/>;
}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><Preview/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
