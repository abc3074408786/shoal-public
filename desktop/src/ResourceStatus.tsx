import {useEffect,useState} from 'react';
import {MemoryStick,Settings2,Terminal,Wifi,WifiOff} from 'lucide-react';
import {invoke} from './desktop-platform';
import {rpc} from './api';
import {currentTarget} from './execution-target';
import {Button,Modal,Tip} from './ui';
import type {Session} from './workbench-types';
import {connectionLabels,connectionState,formatResourceBytes,processLabels,runningTerminals,type DesktopResources} from './resource-status';
import './resource-status.css';
import ResourceManager from './ResourceManager';
import ActionMenu from './ActionMenu';

type Panel='memory'|'terminals'|'connection';
export default function ResourceStatus({workspaceId,workspaceName,connected,failures,connectionError,onOpenSession}:{workspaceId?:string;workspaceName?:string;connected:boolean;failures:number;connectionError:string;onOpenSession:(workspaceId:string,id:string)=>void}){
 const [manager,setManager]=useState(false);
 const [panel,setPanel]=useState<Panel|null>(null),[memory,setMemory]=useState<DesktopResources|null>(null),[memoryError,setMemoryError]=useState('');
 const [sessions,setSessions]=useState<{workspaceId?:string;value:Session[]|null;error:string}>({value:null,error:''});
 useEffect(()=>{setPanel(null);setSessions({workspaceId,value:null,error:''})},[workspaceId]);
 useEffect(()=>{
  if(!panel)return;
  let live=true,running=false,timer:ReturnType<typeof setTimeout>|undefined;
  async function sample(){
   if(!live||running||document.hidden)return;running=true;
   const results=await Promise.allSettled([
    window.velellaDesktop?invoke<DesktopResources>('desktop_resources',{target:currentTarget()}):Promise.reject(new Error('内存统计仅在桌面版可用。')),
    workspaceId?rpc({cmd:'workbench.sessions',workspace_id:workspaceId}):Promise.resolve({sessions:[]}),
   ]);
   if(live){
    const m=results[0],s=results[1];
    if(m.status==='fulfilled'){setMemory(m.value);setMemoryError('')}else{setMemory(null);setMemoryError(String(m.reason))}
    if(s.status==='fulfilled')setSessions({workspaceId,value:s.value.sessions,error:''});else setSessions({workspaceId,value:null,error:String(s.reason)});
   }
   running=false;if(live&&!document.hidden)timer=setTimeout(sample,5000);
  }
  const visibility=()=>{clearTimeout(timer);if(!document.hidden)void sample()};
  document.addEventListener('visibilitychange',visibility);void sample();
  return()=>{live=false;clearTimeout(timer);document.removeEventListener('visibilitychange',visibility)};
 },[workspaceId,panel]);
 const current=sessions.workspaceId===workspaceId?sessions:null;
 const terminals=workspaceId&&current?.value?runningTerminals(current.value):null;
 const state=connectionState(connected,failures),label=connectionLabels[state];
 const title=panel==='memory'?'桌面内存':panel==='terminals'?'运行终端':panel==='connection'?'后台连接':'资源与连接状态';
 const includes=(section:Panel)=>panel===section;
 return <><div className="resource-status" aria-label="资源与连接状态">
  {state!=='connected'&&<Tip label={`本机后台：${label}`}><Button className={`resource-detail-button resource-connection ${state}`} aria-label={`后台连接：${label}`} onClick={()=>setPanel('connection')}><WifiOff size={14}/><span>{label}</span></Button></Tip>}
  <ActionMenu label="状态与资源" icon={<><Settings2 size={14}/><span>状态与资源</span></>}>
   <Button onClick={()=>setManager(true)}><MemoryStick size={14}/>资源管理器</Button>
   <Button onClick={()=>setPanel('memory')}><MemoryStick size={14}/>桌面内存</Button>
   <Button onClick={()=>setPanel('terminals')}><Terminal size={14}/>运行终端</Button>
   <Button onClick={()=>setPanel('connection')}><Wifi size={14}/>连接详情</Button>
  </ActionMenu>
 </div><Modal open={panel!==null} onOpenChange={open=>{if(!open)setPanel(null)}} title={title}><div className="resource-details dialog-body">
  {includes('memory')&&<section><h3>桌面进程私有内存</h3><p className="resource-total">{formatResourceBytes(memory?.privateBytes)}</p><p>统计主进程、界面、GPU 和桌面辅助进程的私有内存，不含终端中的 Claude、Codex、Shell 和后台运行时。此值不是常驻物理内存。</p>
   {memoryError?<p role="status">暂时无法读取：{memoryError}</p>:memory?.privateBytes===null?<p role="status">当前平台无法提供完整的私有内存统计。</p>:!memory?<p role="status">正在读取…</p>:null}
   {!!memory?.processes.length&&<table><caption>桌面进程明细</caption><thead><tr><th>进程</th><th>PID</th><th>私有内存</th></tr></thead><tbody>{memory.processes.map(p=><tr key={p.pid}><td>{processLabels[p.type]||'其他进程'}</td><td>{p.pid}</td><td>{formatResourceBytes(p.privateBytes)}</td></tr>)}</tbody></table>}
   {memory&&<p className="resource-updated">更新于 {new Date(memory.sampledAt).toLocaleTimeString()}</p>}
  </section>}
  {includes('terminals')&&<section><h3>{workspaceName||'当前工作区'} · 运行终端</h3><p>统计当前工作区尚未结束且后台主机可用的终端。未打开的标签也计入，已结束的历史会话不计入。</p>
   {!workspaceId?<p>请先选择工作区。</p>:current?.error?<p role="status">暂时无法读取：{current.error}</p>:terminals===null?<p role="status">正在读取…</p>:terminals.length===0?<p>没有运行中的终端。</p>:<ul className="resource-session-list">{terminals.map(s=><li key={s.id}><Button onClick={()=>{setPanel(null);onOpenSession(workspaceId,s.id)}}><span>{s.title||s.program||'终端'}</span><small>{s.program||'Shell'} · PID {s.status?.pid}</small><span>打开</span></Button></li>)}</ul>}
  </section>}
  {includes('connection')&&<section><h3>本机后台 · {label}</h3><p>{connected?'桌面与本机后台通信正常。':'正在自动尝试连接，已有终端进程不会因此被停止。'}</p>{!connected&&connectionError&&<p className="resource-error" role="status">{connectionError}</p>}</section>}
 </div></Modal><ResourceManager open={manager} onOpenChange={setManager} onOpenSession={onOpenSession}/></>;
}
