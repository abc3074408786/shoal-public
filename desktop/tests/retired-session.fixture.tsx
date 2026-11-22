import React from 'react';
import {createRoot} from 'react-dom/client';
import Workbench from '../src/Workbench';
import {ChromeTarget} from '../src/WindowChrome';
import {TooltipProvider} from '../src/ui';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {AppearanceProvider} from '../src/Appearance';
import '../src/style.css';import '../src/theme.css';import '../src/workspace.css';import '../src/orca-shell.css';

const sessions:any[]=[{id:'legacy',title:'旧聊天',program:'codex',native_chat:true,kind:'agent',task_id:null,execution_mode:'native',status:null,recoverable:false}];
let layout:any={revision:0,axis:'horizontal',ratio:.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]};
const qa:any={requests:[],sessions};Object.assign(window,{retiredQA:qa});
window.fetch=async(_url,init)=>{
 const q=JSON.parse(String(init?.body));qa.requests.push(q);let result:any={};
 switch(q.cmd){
  case 'workbench.git':result={branch:'main',status:'',diff:'',staged:''};break;
  case 'workbench.files':result={files:[]};break;
  case 'workbench.sessions':result={sessions};break;
  case 'workbench.layout':result={layout};break;
  case 'workbench.layout.save':layout=structuredClone(q.layout);layout.revision++;result={layout};break;
  case 'sandbox.capabilities':result={sandbox:{platform:'windows',backend:'unimplemented',available:false,reason:'fixture'}};break;
  case 'workbench.terminal.open':
   if(q.launch!=='codex')throw Error('Only native terminal creation is supported');
   sessions.push({id:q.request_id,title:'Codex',kind:'terminal',program:'codex',task_id:null,execution_mode:'native',status:{exit:null,input_protocol:1},recoverable:false});result={session_id:q.request_id};break;
  case 'workbench.terminal.output':result={from:0,end:0,total:0,data:'',geometry:[]};break;
  case 'workbench.terminal.resize':break;
  default:throw Error(`Unexpected RPC ${q.cmd}`);
 }
 return new Response(JSON.stringify({ok:true,...result}));
};
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><ChromeTarget.Provider value={document.getElementById('chrome')}><Workbench workspace={{id:'w',project_id:'p',name:'终端入口验收',path:'/fixture',kind:'main',position:0,archived:false,favorite:false}} workspaceActions={null}/></ChromeTarget.Provider></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
