import React from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {agentCatalog} from '../src/agent-preferences';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';import '../src/motion.css';

// Synthetic, read-only transport. Never reaches a daemon, model or OS terminal.
const workspace={id:'main:light-preview',project_id:'light-preview',name:'默认工作区',path:'D:\\projects\\velella',kind:'folder',archived:false,favorite:false,position:0};
const project={id:'light-preview',name:'Velella',path:workspace.path,direct:true};
const session={id:'light-preview-codex',title:'Codex',kind:'terminal',program:'codex',task_id:null,native_id:null,status:{pid:42,exit:0,state:'exited',input_protocol:1,input_sequence:0},recoverable:false};
const fixtureSessions=[session],fixtureState={requests:[] as Record<string,unknown>[],failNextLayoutSave:new URL(location.href).searchParams.has('failLayout')};
const fixtureReceipt=document.createElement('output');fixtureReceipt.setAttribute('aria-label','隔离启动回执');
if(new URL(location.href).searchParams.has('agentTest')){fixtureReceipt.style.cssText='position:fixed;right:12px;bottom:12px;z-index:1000;background:#eee;padding:6px;font:11px monospace;max-width:60vw;overflow-wrap:anywhere;pointer-events:none';document.body.append(fixtureReceipt)}
(window as any).velellaSettingsFixture=fixtureState;
let layout={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:'codex-tab',tabs:[{id:'codex-tab',kind:'terminal',target:session.id,title:'Codex'}]}],closed:[]};
const text='\x1b[2J\x1b[H\x1b[1mVelella 浅色预览\x1b[0m · 隔离终端样例\r\n\x1b[90mD:\\projects\\velella\x1b[0m\r\n\r\n'+
  '\x1b[38;2;223;232;227;48;2;67;78;74m\x1b[K\r\n  › 你好\x1b[K\r\n\x1b[K\x1b[0m\r\n\r\n  • 你好，有什么需要我一起处理的？\r\n\r\n\x1b[2m    Worked for 5s · 09:26\x1b[0m\r\n\r\n'+
  '\x1b[38;2;223;232;227;48;2;67;78;74m\x1b[K\r\n  › 看一下当前项目\x1b[K\r\n\x1b[K\x1b[0m\r\n\r\n  • 这是一份用于检查白色界面层次的样例输出。\r\n    消息区、输入区和当前选中项保持清晰区分。\r\n\r\n\x1b[34m    src/\x1b[0m       源代码\r\n\x1b[32m    README.md\x1b[0m  项目说明\r\n\r\n\x1b[2m    Worked for 4s · 09:27\x1b[0m'+
  '\x1b[29;1H\x1b[38;2;223;232;227;48;2;60;69;65m\x1b[K\r\n  › Ask Codex to do anything\x1b[K\r\n\x1b[K\x1b[0m\r\n\x1b[90m    Codex · D:\\projects\\velella\x1b[0m';
const output=new TextEncoder().encode(text);
localStorage.setItem('velella:ui:theme',JSON.stringify('light'));
localStorage.setItem('velella:workspace',workspace.id);
localStorage.setItem('velella:ui:left-open','true');
localStorage.setItem('velella:ui:project-bar-open','false');
localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',fontFamily:'Cascadia Mono',fontSize:14,cursorBlink:false}));
const nativeFetch=window.fetch.bind(window);
window.fetch=async(input,init)=>{
  if(!String(input).endsWith('/_velella'))return nativeFetch(input,init);
  const q=JSON.parse(String(init?.body));let result:any={ok:true};
  if(q.cmd==='workbench.terminal.open'||q.cmd==='agent.check')fixtureState.requests.push(q);
  fixtureReceipt.textContent=JSON.stringify(fixtureState.requests.filter(request=>request.cmd==='workbench.terminal.open'));
  if(q.cmd==='workbench.snapshot'||q.cmd==='snapshot')result={ok:true,projects:[project],workspaces:[workspace],tasks:[],source_links:[]};
  else if(q.cmd==='workbench.sessions')result={ok:true,sessions:fixtureSessions};
  else if(q.cmd==='agent.check')result={ok:true,available:q.agent==='codex',version:q.agent==='codex'?'codex-cli fixture':'',authenticated:false};
  else if(q.cmd==='workbench.terminal.open'){
    const agent=agentCatalog.find(agent=>agent.id===q.launch);
    fixtureSessions.push({...session,id:q.request_id,title:agent?.name||'Shell',program:agent?.program||'powershell'});
    result={ok:true,session_id:q.request_id};
  }
  else if(q.cmd==='workbench.layout')result={ok:true,layout,split_layout_version:1};
  else if(q.cmd==='workbench.layout.save'){if(fixtureState.failNextLayoutSave){fixtureState.failNextLayoutSave=false;result={ok:false,error:'隔离样例：布局保存失败'}}else{layout={...q.layout,revision:layout.revision+1};result={ok:true,layout}}}
  else if(q.cmd==='workbench.git')result={ok:true,status:'',branch:''};
  else if(q.cmd==='workbench.files')result={ok:true,files:['src/main.rs','src/ui.tsx','README.md']};
  else if(q.cmd==='workbench.terminal.activity')result={ok:true,enabled:false,events:[],next:0,total:0};
  else if(q.cmd.endsWith('.output')){
    const from=Math.min(q.since||0,output.length),bytes=output.slice(from);
    result={ok:true,from,total:output.length,end:output.length,data:btoa(String.fromCharCode(...bytes)),geometry:from===0?[{offset:0,cols:100,rows:34,reset:true,sequence:1}]:[]};
  }
  return Response.json(result);
};
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
