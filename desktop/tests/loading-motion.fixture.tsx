import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import TerminalView from '../src/TerminalView';
import AppSettings from '../src/AppSettings';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/orca-light.css';
import '../src/motion.css';

const params=new URLSearchParams(location.search),scene=params.get('case')||'startup';
let release:()=>void=()=>{},fail=false,inputs=0;
const gate=new Promise<void>(resolve=>{release=resolve});
if(params.has('fast'))release();
const encoded=new TextEncoder().encode('\x1b[2J\x1b[HRESTORED_TERMINAL\r\n> ');
const output={ok:true,from:0,end:encoded.length,total:encoded.length,data:btoa(String.fromCharCode(...encoded)),geometry:[{offset:0,cols:80,rows:24,reset:true,sequence:0}]};
const nativeFetch=window.fetch.bind(window);
// All product RPCs are synthetic. No real runtime, account, host or model calls.
window.fetch=async(input,init)=>{
  if(!String(input).endsWith('/_velella'))return nativeFetch(input,init);
  const request=JSON.parse(String(init?.body||'{}'));
  if(request.cmd==='workbench.snapshot'){
    if(scene==='startup')await gate;
    return Response.json(fail?{ok:false,error:'Synthetic connection failure'}:{ok:true,projects:[],workspaces:[],tasks:[],source_links:[]});
  }
  if(request.cmd==='orchestration.capabilities'){
    if(scene==='settings')await gate;
    return Response.json(fail?{ok:false,error:'Synthetic settings failure'}:{ok:true,providers:['codex','claude-code','omp','opencode'].map(id=>({id,installed:true})),skills:[]});
  }
  if(request.cmd==='orchestration.templates.list')return Response.json({ok:true,templates:[]});
  if(request.cmd==='cli.inspect')return Response.json({ok:true,found:true,path:'C:/preview-only/velella-cli.exe',source:'path'});
  if(request.cmd==='agent.check'){if(params.has('compact'))await gate;return Response.json({ok:true,available:true,version:'fixture-only'});}
  if(request.cmd==='protocol'){if(params.has('compact'))await gate;return Response.json({ok:true,capabilities:['terminal-permissions-v1']});}
  if(request.cmd?.endsWith('.output')){
    await gate;
    return Response.json(request.since>=output.total?{...output,from:output.total,data:''}:output);
  }
  if(request.cmd?.endsWith('.input')){inputs++;document.documentElement.dataset.inputRequests=String(inputs);return Response.json({ok:true});}
  if(['terminal.resize','workbench.terminal.resize','workbench.terminal.control'].includes(request.cmd))return Response.json({ok:true});
  throw Error('Isolated loading fixture rejected request: '+request.cmd);
};
document.documentElement.dataset.theme=params.get('theme')==='dark'?'dark':'light';
localStorage.setItem('velella:ui:theme',JSON.stringify(document.documentElement.dataset.theme));
document.documentElement.dataset.inputRequests='0';
document.documentElement.dataset.loadingFeedbackShown='false';
new MutationObserver(()=>{if(document.querySelector('[data-loading-revealed="true"]'))document.documentElement.dataset.loadingFeedbackShown='true'}).observe(document.getElementById('root')!,{subtree:true,childList:true,attributes:true,attributeFilter:['data-loading-revealed']});
function Preview(){
  const [released,setReleased]=useState(false),[closed,setClosed]=useState(false),[offline,setOffline]=useState(false);
  return <div style={{height:'100dvh',display:'flex',flexDirection:'column'}}>
    <style>{'.loading-fixture-surface>.ide-shell.project-shell{flex:1;inline-size:100%;block-size:100%;}'}</style>
    <nav aria-label="动效预览" style={{display:'flex',flexWrap:'wrap',alignItems:'center',gap:12,padding:10,borderBlockEnd:'1px solid var(--border)',fontSize:12}}>
      <span>隔离预览</span>{[['startup','应用启动'],['terminal','终端恢复'],['settings','设置读取']].map(([id,name])=><a key={id} href={`?case=${id}`} aria-current={scene===id?'page':undefined}>{name}</a>)}
      <a href={`?case=${scene}&fast=1`}>快速加载</a>
      <button disabled={released} onClick={()=>{release();setReleased(true)}}>完成加载</button>
      {scene!=='terminal'&&<button disabled={released} onClick={()=>{fail=true;release();setReleased(true)}}>模拟失败</button>}
      {scene==='startup'&&released&&<button onClick={()=>{fail=!fail;setOffline(fail)}}>{offline?'恢复连接':'模拟后台断开'}</button>}
    </nav>
    <div className="loading-fixture-surface" style={{flex:1,minHeight:0,display:'flex'}}>
      {scene==='terminal'?<TerminalView workspace={{projectId:'loading-preview',workspaceId:'loading-preview',id:'loading-preview',program:'powershell',status:{exit:null,input_protocol:1,pid:1}}}/>:scene==='settings'?(closed?<p>设置已关闭</p>:<AppSettings onClose={()=>setClosed(true)}/>):<WorkspaceApp/>}
    </div>
  </div>;
}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><Preview/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
