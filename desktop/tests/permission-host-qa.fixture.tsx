import React,{useState,useSyncExternalStore} from 'react';
import {createRoot} from 'react-dom/client';
import AppSettings from '../src/AppSettings';
import {TooltipProvider} from '../src/ui';
import {currentTarget,selectTarget,subscribeTarget,type ExecutionTarget} from '../src/execution-target';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';import '../src/project-shell.css';import '../src/orca-light.css';
// Entirely in memory: never read browser/native preferences or contact any runtime.
const data=new Map<string,string>();
const memory={get length(){return data.size},clear(){data.clear()},getItem(key:string){return data.get(key)??null},setItem(key:string,value:string){data.set(key,String(value))},removeItem(key:string){data.delete(key)},key(index:number){return [...data.keys()][index]??null}};
Object.defineProperty(window,'localStorage',{value:memory,configurable:true});
const remote:ExecutionTarget={kind:'ssh',host_id:'11111111-1111-4111-8111-111111111111',server_id:'22222222-2222-4222-8222-222222222222'};
const records:unknown[]=[];let notify=()=>{},switched=false;
const query=new URLSearchParams(location.search);
window.velellaDesktop={listen:()=>()=>{},async invoke<T>(command:string,args?:unknown):Promise<T>{
 if(command!=='rpc')throw Error('Fixture refuses non-RPC bridge commands');
 const {request,target={kind:'local'}}=args as {request:{cmd:string};target?:ExecutionTarget};
 records.push({request,target});notify();
 if(request.cmd==='agent.check')return {ok:true,available:true,version:'synthetic fixture'} as T;
 if(request.cmd==='protocol'){
  if(query.has('switch')&&!switched&&target.kind==='local'){switched=true;queueMicrotask(()=>selectTarget(remote))}
  return {ok:true,capabilities:query.has('old-host')?[]:['terminal-permissions-v1']} as T;
 }
 throw Error('Fixture refuses unknown RPC');
}};
window.fetch=async()=>{throw Error('No network or daemon bridge permitted')};
function Preview(){
 const [open,setOpen]=useState(false),[revision,setRevision]=useState(0);
 const target=useSyncExternalStore(subscribeTarget,currentTarget);notify=()=>setRevision(n=>n+1);
 function settings(next:ExecutionTarget){selectTarget(next);setOpen(true)}
 return <div className="project-shell" style={{minHeight:'100vh',padding:30,background:'var(--background)',color:'var(--foreground)'}}>
  <h1>权限主机隔离测试</h1><button onClick={()=>settings({kind:'local'})}>本机设置</button><button onClick={()=>settings(remote)}>远端设置</button>
  <output id="qa-current-target">{target.kind}</output><pre id="qa-storage">{JSON.stringify(Object.fromEntries(data))}</pre><pre id="qa-requests">{JSON.stringify(records)}</pre><span hidden>{revision}</span>
  {open&&<AppSettings onClose={()=>{setOpen(false);setRevision(n=>n+1)}}/>}
 </div>;
}
createRoot(document.getElementById('root')!).render(<TooltipProvider><Preview/></TooltipProvider>);
