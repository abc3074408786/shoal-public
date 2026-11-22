import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import PluginsPage from '../src/PluginsPage';
import McpCatalogPicker from '../src/McpCatalogPicker';
import {AppearanceProvider} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {TooltipProvider} from '../src/ui';
import {selectTarget} from '../src/execution-target';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';import '../src/motion.css';
localStorage.setItem('velella:ui:theme',JSON.stringify('light'));
if(new URL(location.href).searchParams.has('remote'))selectTarget({kind:'ssh',host_id:'11111111-1111-4111-8111-111111111111',server_id:'22222222-2222-4222-8222-222222222222'});
const fixture=(window as any).__computerUsePlugin={requests:[],nativeRequests:[],prepareFail:false,previewFail:false,confirmFail:false,digest:'sha256:fixture-reviewed'};
const path='D:/synthetic profile/Computer Use/catalog.json';
const entries=[{id:'velella-computer-use-observe',command:'D:/synthetic/python.exe',args:['-m','velella_computer_use.server'],catalog:['computer_screenshot']},
  {id:'velella-computer-use-control',command:'D:/synthetic/python.exe',args:['-m','velella_computer_use.server','--allow-input'],catalog:['computer_screenshot','computer_acquire_control','computer_stop_control','computer_type_text']}];
window.velellaDesktop={invoke:async(command,args:any)=>{
  if(command==='desktop_extensions'){
    fixture.nativeRequests.push(args);
    if(args.action==='list')return {ok:true,result:{plugins:[]}} as any;
    if(args.action==='prepareBundledMcp')return (fixture.prepareFail?{ok:false,error:{message:'Python 环境尚未准备，请按接入说明创建环境后重试。'}}:{ok:true,result:{path}}) as any;
    throw Error('Unexpected extension operation');
  }
  if(command!=='rpc')throw Error('Unexpected fixture command: '+command);
  const request=args.request;fixture.requests.push(request);
  if(request.cmd==='orchestration.mcp.preview'){
    if(fixture.previewFail)throw Error('合成目录读取失败');
    return {ok:true,catalog:{path:request.path,digest:fixture.digest,entries}} as any;
  }
  if(request.cmd==='orchestration.mcp.confirm'){
    if(fixture.confirmFail||request.digest!==fixture.digest)throw Error('目录已变化，请重新预览');
    const entry=entries.find(e=>e.id===request.entry_id)!;
    return {ok:true,server:{...entry,tools:request.tools,source:{path:request.path,digest:request.digest,entry_id:entry.id,confirmation_id:'fixture-confirmation'}}} as any;
  }
  throw Error('Unexpected fixture RPC: '+request.cmd);
},listen:()=>()=>{}};
function App(){
  const [plugins,setPlugins]=useState(true),[chosen,setChosen]=useState('');
  return plugins?<PluginsPage onBack={()=>setPlugins(false)} onSettings={()=>{}}/>:<main><h1>鱼群 MCP 选择器</h1><button onClick={()=>setPlugins(true)}>返回插件</button><McpCatalogPicker onAdd={server=>setChosen(server.id+' '+server.tools.join(','))}/><output aria-label="角色所选工具">{chosen}</output></main>;
}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><App/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
