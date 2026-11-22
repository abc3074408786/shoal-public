import {useCallback,useEffect,useRef,useState} from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import {ChevronDown,ChevronRight,HardDrive,MemoryStick,RefreshCw,Trash2,X} from 'lucide-react';
import {invoke} from './desktop-platform';
import {currentTarget} from './execution-target';
import {Button,Modal} from './ui';
import {formatResourceBytes} from './resource-status';

type Metrics={cpu:number|null;workingSetBytes:number|null;privateBytes:number|null};
type SessionResource=Metrics&{id:string;title:string;pid:number;processCount:number};
type WorkspaceResource=Metrics&{id:string;name:string;path:string;archived:boolean;error:string|null;sessions:SessionResource[]};
type ProjectResource=Metrics&{id:string;name:string;workspaces:WorkspaceResource[]};
type Resources=Metrics&{sampledAt:number;projects:ProjectResource[];desktop:Metrics&{name:string;processes:Array<Metrics&{pid:number;name:string}>}};
type Cache={key?:string;relative:string;path?:string;bytes?:number;partial?:boolean;skipped?:number;error?:string};
type Space={token:string;scannedAt:number;rows:Array<{id:string;name:string;path:string;bytes:number|null;partial:boolean;skipped?:number;error:string|null;caches:Cache[]}>};
const cpu=(n:number|null|undefined)=>n==null?'—':n.toFixed(1)+'%';
const metric=(value:Metrics|null,key:'workingSetBytes'|'privateBytes')=>formatResourceBytes(value?.[key]);

export default function ResourceManager({open,onOpenChange,onOpenSession}:{open:boolean;onOpenChange:(value:boolean)=>void;onOpenSession:(workspaceId:string,sessionId:string)=>void}){
 const [data,setData]=useState<Resources|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false),[review,setReview]=useState(false);
 const [collapsed,setCollapsed]=useState<Set<string>>(new Set(['desktop'])),[column,setColumn]=useState<'workingSetBytes'|'privateBytes'>('workingSetBytes');
 const [history,setHistory]=useState<Record<string,number[]>>({}),[view,setView]=useState<'resources'|'space'>('resources');
 const [space,setSpace]=useState<Space|null>(null),[scanning,setScanning]=useState(false),[spaceError,setSpaceError]=useState(''),[selected,setSelected]=useState<Set<string>>(new Set()),[cleaning,setCleaning]=useState(false),[receipt,setReceipt]=useState('');
 const serial=useRef(false),active=useRef(false),mounted=useRef(true);
 useEffect(()=>{mounted.current=true;return()=>{mounted.current=false}},[]);
 const sample=useCallback(async()=>{
  if(serial.current||!active.current||document.hidden)return;serial.current=true;setBusy(true);
  try{const r=await invoke<Resources>('resource_manager',{target:currentTarget()});if(active.current){setData(r);setError('');setHistory(old=>{const next={...old};for(const item of [...r.projects,...r.projects.flatMap(p=>p.workspaces).flatMap(w=>[w,...w.sessions])]){if(item.cpu!==null)next[item.id]=[...(old[item.id]||[]),item.cpu].slice(-24)}return next})}}
  catch(e){if(active.current){setData(null);setError(String(e))}}finally{serial.current=false;if(mounted.current)setBusy(false)}
 },[]);
 useEffect(()=>{
  active.current=open;if(!open)return;let live=true,timer:ReturnType<typeof setTimeout>;
  const poll=async()=>{await sample();if(live&&!document.hidden){clearTimeout(timer);timer=setTimeout(poll,5000)}};
  const visibility=()=>{clearTimeout(timer);if(!document.hidden)void poll()};void poll();document.addEventListener('visibilitychange',visibility);
  return()=>{live=false;active.current=false;clearTimeout(timer);document.removeEventListener('visibilitychange',visibility)};
 },[open,sample]);
 const toggle=(key:string)=>setCollapsed(old=>{const next=new Set(old);next.has(key)?next.delete(key):next.add(key);return next});
 function trend(id:string){const values=history[id]||[];if(values.length<2)return null;const max=Math.max(1,...values);return <svg className="resource-trend" viewBox="0 0 56 18" aria-label="最近 CPU 趋势"><polyline points={values.map((v,i)=>`${i*56/(values.length-1)},${17-v/max*16}`).join(' ')} fill="none" stroke="currentColor"/></svg>}
 function values(item:Metrics){return <><span>{cpu(item.cpu)}</span><span title={`工作集 ${metric(item,'workingSetBytes')} · 私有内存 ${metric(item,'privateBytes')}`}>{metric(item,column)}</span></>}
 async function scan(workspaceId?:string){if(scanning||cleaning)return;setScanning(true);setSpaceError('');setReceipt('');setView('space');setSelected(new Set());try{const r=await invoke<Space>('resource_space_scan',{target:currentTarget(),...(workspaceId?{workspace_id:workspaceId}:{})});if(mounted.current)setSpace(r)}catch(e){if(mounted.current)setSpaceError(String(e))}finally{if(mounted.current)setScanning(false)}}
 async function clean(){
  const targets=space?.rows.flatMap(w=>w.caches.filter(c=>c.key&&selected.has(c.key)))||[];if(!space||!targets.length||cleaning||scanning)return;
  setReview(false);
  setCleaning(true);setSpaceError('');
  try{const r=await invoke<{ok:boolean;error?:string;completed:Array<{path:string;bytes:number}>}>('resource_space_clean',{target:currentTarget(),token:space.token,keys:[...selected],confirmed:true});setSelected(new Set());setSpace(null);setReceipt(`已移入回收站 ${r.completed.length} 个目录。${r.completed.map(c=>c.path).join('；')}`);if(!r.ok)setSpaceError(r.error||'清理未完成')}catch(e){setSpaceError(String(e))}finally{setCleaning(false)}
 }
 return <Dialog.Root open={open} onOpenChange={onOpenChange}><Dialog.Portal><Dialog.Overlay className="resource-manager-overlay"/><Dialog.Content className="resource-manager" aria-describedby="resource-manager-scope">
  <header className="resource-manager-heading"><Dialog.Title><MemoryStick size={14}/>资源管理器</Dialog.Title><div><Button aria-label="刷新资源" disabled={busy} onClick={()=>void sample()}><RefreshCw size={14} className={busy?'resource-spinning':''}/></Button><Button aria-label="预览工作区清理" onClick={()=>setView('space')}><Trash2 size={14}/></Button><Dialog.Close asChild><Button aria-label="关闭资源管理器"><X size={14}/></Button></Dialog.Close></div></header>
  {view==='resources'?<>
   <div className="resource-manager-summary"><strong>{cpu(data?.cpu)}</strong><span>·</span><strong>{metric(data,'workingSetBytes')} ∑ WS</strong><span>·</span><strong>{metric(data,'privateBytes')} ∑ Private</strong></div>
   <div className="resource-table-heading"><span>名称</span><span>CPU</span><Button aria-label="切换内存列" title="切换工作集 / 私有内存" onClick={()=>setColumn(c=>c==='workingSetBytes'?'privateBytes':'workingSetBytes')}>{column==='workingSetBytes'?'WS':'Private'}</Button></div>
   <div className="resource-tree">
    {error&&<p role="status" className="resource-manager-message">暂时无法读取：{error}</p>}{!data&&!error&&<p className="resource-manager-message">正在读取真实进程统计…</p>}
    {data?.projects.map(p=><section key={p.id}><Button className="resource-tree-row resource-project" aria-expanded={!collapsed.has(p.id)} onClick={()=>toggle(p.id)}><span>{collapsed.has(p.id)?<ChevronRight size={12}/>:<ChevronDown size={12}/>}<b>{p.name}</b>{trend(p.id)}</span>{values(p)}</Button>
     {!collapsed.has(p.id)&&p.workspaces.map(w=><div key={w.id}><Button className="resource-tree-row resource-workspace" aria-expanded={!collapsed.has(w.id)} title={w.path} onClick={()=>toggle(w.id)}><span>{collapsed.has(w.id)?<ChevronRight size={12}/>:<ChevronDown size={12}/>}<b>{w.name}{w.archived?' · 已归档':''}</b>{trend(w.id)}</span>{values(w)}</Button>
      {!collapsed.has(w.id)&&<>{w.error?<p className="resource-manager-message" role="status">{w.error}</p>:!w.sessions.length?<p className="resource-empty-row">没有运行中的终端</p>:w.sessions.map(s=><Button key={s.id} className="resource-tree-row resource-terminal" title={`PID ${s.pid} · 含 ${s.processCount} 个进程；点击打开已有会话`} onClick={()=>{onOpenChange(false);onOpenSession(w.id,s.id)}}><span><i className={s.processCount?'live':''}/><span>{s.title}</span>{trend(s.id)}</span>{values(s)}</Button>)}</>}
     </div>)}
    </section>)}
    {data&&<section><Button className="resource-tree-row resource-project" aria-expanded={!collapsed.has('desktop')} onClick={()=>toggle('desktop')}><span>{collapsed.has('desktop')?<ChevronRight size={12}/>:<ChevronDown size={12}/>}<b>Velella 桌面</b></span>{values(data.desktop)}</Button>{!collapsed.has('desktop')&&data.desktop.processes.map(p=><div className="resource-tree-row resource-workspace" key={p.pid}><span title={p.name}>pid {p.pid}</span>{values(p)}</div>)}</section>}
   </div>
   <p id="resource-manager-scope" className="resource-manager-note">含终端主机及其子进程、Velella 桌面；不含未归属的后台进程。CPU 按整机容量计，首轮为 —。WS 含共享页，不等于独占物理内存。{data&&` 更新于 ${new Date(data.sampledAt).toLocaleTimeString()}`}</p>
   <div className="resource-clean-entry"><Button onClick={()=>setView('space')}>清理工作区<ChevronRight size={14}/></Button></div>
  </>:<div className="resource-space-view"><Button className="resource-back" onClick={()=>setView('resources')}>‹ 返回资源列表</Button><h3>工作区空间与缓存清理</h3><p id="resource-manager-scope">按需统计文件逻辑大小；嵌套工作区可能重叠，不做跨目录总和。清理仅识别 .vite、.turbo、.parcel-cache、node_modules/.cache，不自动选择，不删除源码。</p>
   {spaceError&&<p role="alert">{spaceError}</p>}{receipt&&<p role="status">{receipt}</p>}{scanning&&<p role="status">正在扫描…大目录最多扫描约 15 秒，未扫完会标注。</p>}
   {!space&&!scanning&&!receipt&&<p>尚未扫描。扫描后可预览路径与大小。</p>}
   {space?.rows.map(w=><section className="resource-space-workspace" key={w.id}><div><strong>{w.name}</strong><span>{w.bytes===null?'—':`${w.partial?'至少 ':''}${formatResourceBytes(w.bytes)}`}</span><Button disabled={scanning||cleaning} onClick={()=>void scan(w.id)}>扫描此项</Button></div><p className="resource-path">{w.path}</p>{w.error&&<p>{w.error}</p>}{w.partial&&<p>扫描未完成，显示已读取部分。</p>}{!!w.skipped&&<p>跳过 {w.skipped} 项链接或不可访问条目。</p>}{!w.caches.length&&<p>未发现可识别的构建缓存。</p>}{w.caches.map(c=><label key={c.relative} className="resource-cache"><input type="checkbox" aria-label={`选择缓存 ${c.path||c.relative}`} disabled={!c.key||c.partial||!!c.skipped||!!c.error||scanning||cleaning} checked={!!c.key&&selected.has(c.key)} onChange={e=>setSelected(old=>{const next=new Set(old);if(c.key)e.target.checked?next.add(c.key):next.delete(c.key);return next})}/><span title={c.path}>{c.relative}<small>{c.error||`${c.partial?'扫描不完整 · ':''}${c.skipped?'含链接，不能清理 · ':''}${formatResourceBytes(c.bytes)}`}</small></span></label>)}</section>)}
   <Button className="resource-clean-selected" disabled={!selected.size||scanning||cleaning} onClick={()=>setReview(true)}>{cleaning?'正在移入回收站…':`预览并清理所选缓存 (${selected.size})`}</Button>
  </div>}
  <footer className="resource-space-footer"><HardDrive size={16}/><div><strong>空间 <small>测试版</small></strong><span>{space?`上次扫描 ${new Date(space.scannedAt).toLocaleTimeString()}`:'不自动扫描工作区磁盘使用情况。'}</span></div><Button disabled={scanning||cleaning} onClick={()=>void scan()}><RefreshCw size={13}/>{scanning?'扫描中':'扫描'}</Button><Button onClick={()=>setView('space')}>查看</Button></footer>
  <Modal open={review} onOpenChange={setReview} title="确认操作"><div className="dialog-body"><p>将以下构建缓存移入系统回收站？缓存将在工具下次运行时重新生成，源码保留。工作区有运行或启动中的会话时会拒绝清理。</p><ul>{space?.rows.flatMap(w=>w.caches.filter(c=>c.key&&selected.has(c.key))).map(c=><li key={c.key} style={{overflowWrap:'anywhere'}}>{c.path} · {formatResourceBytes(c.bytes)}</li>)}</ul></div><div className="dialog-footer"><Button onClick={()=>setReview(false)}>取消</Button><Button className="primary" onClick={()=>void clean()}>确认</Button></div></Modal>
 </Dialog.Content></Dialog.Portal></Dialog.Root>;
}
