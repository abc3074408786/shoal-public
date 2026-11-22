import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import WorkspaceApp from '../src/WorkspaceApp';
import McpCatalogPicker from '../src/McpCatalogPicker';
import {AppearanceProvider} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {TooltipProvider} from '../src/ui';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';import '../src/motion.css';
const project={id:'plugin-fixture',name:'插件验收',path:'D:/synthetic/plugins',setup:''};
const workspace={id:'main:plugin-fixture',project_id:project.id,name:'默认工作区',path:project.path,kind:'primary',archived:false,favorite:false,position:0};
const fixture=(window as any).__plugins={requests:[],fail:false,digest:'sha256:reviewed',skillsFail:false,nativeFail:false,nativeRequests:[]};
const native=JSON.parse(localStorage.getItem('synthetic-native-plugin')||'null')||{id:'ontology',name:'开发本体',version:'1.1.0',description:'用角色、需求、任务与交付证据组织项目协作。',enabled:true,installed:true,dataDir:'D:/synthetic/ontology-data',engine:{version:'semantic-1'}};
window.velellaDesktop={invoke:async(command,args:any)=>{
  if(command==='window')return false as any;
  if(command==='desktop_extensions'){
    fixture.nativeRequests.push(args);
    if(args.action==='list')return {ok:true,result:{plugins:[{...native}]}} as any;
    if(fixture.nativeFail)return {ok:false,error:{message:'合成插件状态写入失败'}} as any;
    if(args.action==='setEnabled'){if(!native.installed)return {ok:false,error:{message:'请先安装'}} as any;native.enabled=args.enabled;}
    if(args.action==='uninstall'){if(!args.confirmed)return {ok:false,error:{message:'需要确认'}} as any;native.installed=false;native.enabled=false;}
    if(args.action==='install'){native.installed=true;native.enabled=false;}
    if(args.action==='selectDataDirectory')native.dataDir='D:/synthetic/selected-data';
    if(args.action==='call'){
      if(!native.installed||!native.enabled)return {ok:false,error:{message:'插件不可用'}} as any;
      const operation=args.request.operation;
      if(operation==='describe')return {ok:true,result:{initialized:false,engineReady:true}} as any;
      if(operation==='schema')return {ok:true,result:{definitions:{entity_types:[],roles:[],relations:[]}}} as any;
      throw Error('Unexpected ontology operation: '+operation);
    }
    localStorage.setItem('synthetic-native-plugin',JSON.stringify(native));return {ok:true,result:{...native}} as any;
  }
  if(command!=='rpc')throw Error('Unexpected fixture bridge: '+command);
  const req=args.request;fixture.requests.push(req);
  switch(req.cmd){
    case 'workbench.snapshot':return {ok:true,projects:[project],workspaces:[workspace],tasks:[],source_links:[]} as any;
    case 'orchestration.capabilities':if(fixture.skillsFail)throw Error('Synthetic unavailable');return {ok:true,providers:[],skills:[{name:'design',path:'D:/synthetic/skills/design/SKILL.md',digest:'sha256:skill'},{name:'review',path:'D:/synthetic/skills/review/SKILL.md',digest:'sha256:review'}]} as any;
    case 'orchestration.mcp.preview':if(fixture.fail)throw Error('目录文件不可用');return {ok:true,catalog:{path:req.path,digest:fixture.digest,entries:[{id:'docs',command:'synthetic-docs-mcp',args:['--stdio'],catalog:['search','read']}]}} as any;
    case 'orchestration.mcp.confirm':if(req.digest!==fixture.digest)throw Error('目录已变化，请重新预览');return {ok:true,server:{id:req.entry_id,command:'synthetic-docs-mcp',args:['--stdio'],catalog:['search','read'],tools:req.tools,source:{path:req.path,digest:req.digest,entry_id:req.entry_id,confirmation_id:'synthetic-receipt'}}} as any;
    case 'protocol':return {ok:true,capabilities:[]} as any;
    case 'agent.check':return {ok:true,available:false} as any;
    default:throw Error('Unexpected synthetic RPC: '+req.cmd);
  }
},listen:()=>()=>{}};
localStorage.setItem('velella:workspace',workspace.id);
localStorage.setItem('velella:ui:theme',JSON.stringify('light'));
function Project(){const [text,setText]=useState('保留项目草稿'),[added,setAdded]=useState('');return <div style={{padding:20,overflow:'auto'}}><label>项目草稿<input aria-label="项目草稿" value={text} onChange={e=>setText(e.target.value)}/></label><h2>编排 MCP 选择器</h2><McpCatalogPicker onAdd={s=>setAdded(s.id+' '+s.tools.join(','))}/><output aria-label="所选插件">{added}</output></div>}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><WorkspaceApp projectSurface={()=><Project/>}/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
