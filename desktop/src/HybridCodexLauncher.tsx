import {useEffect,useRef,useState} from 'react';
import {rpc} from './api';
import {currentTarget,sameTarget,LOCAL_TARGET} from './execution-target';
import {Button,NativeDialog} from './ui';
import './hybrid-codex.css';
type Profile={id:string;name:string;host:string;user:string};
export default function HybridCodexLauncher({workspaceId,onOpened,onClose}:{workspaceId:string;onOpened:(id:string)=>Promise<void>;onClose:()=>void}){
 const dialog=useRef<HTMLDialogElement>(null),request=useRef(crypto.randomUUID());
 const [profiles,setProfiles]=useState<Profile[]|null>(null),[profile,setProfile]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false);
 const [target]=useState(currentTarget),sid=useRef<string>(undefined);
 useEffect(()=>{const previous=document.activeElement as HTMLElement|null;dialog.current?.showModal();let live=true;void rpc({cmd:'ssh.list'},LOCAL_TARGET).then(r=>{if(live){setProfiles(r.profiles);if(r.profiles.length===1)setProfile(r.profiles[0].id)}}).catch(e=>{if(live)setError(String(e))});return()=>{live=false;previous?.focus()}},[]);
 async function launch(){if(busy)return;setBusy(true);setError('');try{
  if(target.kind!=='local'||!sameTarget(target,currentTarget()))throw Error('请回到原本机工作区启动。');
  if(!sid.current){const r=await rpc({cmd:'workbench.terminal.open',workspace_id:workspaceId,launch:'hybrid-codex',ssh_profile_id:profile,request_id:request.current},target);sid.current=r.session_id;}
  await onOpened(sid.current!);onClose();
 }catch(e){setError(String(e))}finally{setBusy(false)}}
 return <NativeDialog ref={dialog} className="workspace-composer" aria-label="远程 Codex · 本地项目" onCancel={e=>{e.preventDefault();if(!busy)onClose()}}><form onSubmit={e=>{e.preventDefault();void launch()}} aria-busy={busy}>
  <div className="dialog-heading"><h2>远程 Codex · 本地项目</h2></div><div className="dialog-body">
   <p>项目文件留在本机。Codex 和计算运行在远端 Linux；本机写入、运行程序会在工作区等待你审批。</p>
   <label htmlFor="hybrid-host">远程 SSH 配置</label><select id="hybrid-host" value={profile} disabled={busy||!!sid.current} onChange={e=>setProfile(e.target.value)}><option value="">请选择</option>{profiles?.map(p=><option key={p.id} value={p.id}>{p.name} · {p.user?p.user+'@':''}{p.host}</option>)}</select>
   {profiles===null&&!error&&<p role="status">正在读取 SSH 配置…</p>}{profiles?.length===0&&<p>请先在“连接与工具”的 SSH 主机中保存配置，可使用已有别名 agent。</p>}
   <p className="muted">需要远端已有 Codex、Node.js、非交互 SSH 认证和允许的隧道；启动失败会保留错误和会话。</p>{error&&<p role="alert">{error}</p>}
  </div><div className="dialog-footer"><Button type="button" disabled={busy} onClick={onClose}>取消</Button><Button className="primary" disabled={busy||!profile}>{busy?'正在打开…':sid.current?'继续打开':'创建并打开'}</Button></div>
 </form></NativeDialog>
}
type Operation={id:string;digest:string;state:string;tool:string;arguments:Record<string,unknown>;error?:string;result?:unknown};
export function HybridApprovals({workspaceId,sessionId}:{workspaceId:string;sessionId:string}){
 const [operations,setOperations]=useState<Operation[]>([]),[error,setError]=useState(''),[busy,setBusy]=useState(false),[target]=useState(currentTarget);
 useEffect(()=>{let live=true;async function refresh(){try{const r=await rpc({cmd:'workbench.hybrid.status',workspace_id:workspaceId,session_id:sessionId},target);if(live&&sameTarget(target,currentTarget())){setOperations(r.operations);setError('')}}catch(e){if(live)setError(String(e))}}void refresh();const timer=setInterval(()=>void refresh(),1500);return()=>{live=false;clearInterval(timer)}},[workspaceId,sessionId,target]);
 async function decide(op:Operation,approved:boolean){if(busy)return;setBusy(true);try{if(!sameTarget(target,currentTarget()))throw Error('执行主机已切换，审批未发送。');await rpc({cmd:'workbench.hybrid.decide',workspace_id:workspaceId,session_id:sessionId,operation_id:op.id,digest:op.digest,approved,confirmed:true},target);setOperations(old=>old.map(o=>o.id===op.id?{...o,state:approved?'approved':'rejected'}:o));setError('')}catch(e){setError(String(e))}finally{setBusy(false)}}
 const pending=operations.filter(o=>o.state==='pending'),uncertain=operations.filter(o=>o.state==='uncertain');
 return <section className="hybrid-approvals" aria-label="混合执行本机审批"><details open={pending.length>0||uncertain.length>0}><summary>远程 Codex · 本机操作（{pending.length} 项待审批）</summary><p>以下操作在这台电脑执行。命令审批不会创建操作系统沙箱。远端运行状态请查看原生终端。</p>
  {error&&<p role="alert">审批状态读取失败：{error}</p>}{pending.map(op=><div key={op.id}><strong>{op.tool==='local_run_command'?'运行本机程序':op.tool==='local_write_evidence'?'保存本机证据':'写入本机文件'}</strong><pre>{JSON.stringify(op.arguments,null,2)}</pre><Button disabled={busy} onClick={()=>void decide(op,false)}>拒绝</Button><Button disabled={busy} onClick={()=>void decide(op,true)}>批准本次操作</Button></div>)}
  {uncertain.map(op=><p role="alert" key={op.id}>操作 {op.id} 的结果未知；请检查文件或进程，应用不会自动重发。</p>)}
  {!operations.length&&!error&&<p>尚无本机写入或命令请求。</p>}
  <details><summary>操作记录</summary>{operations.map(op=><p key={op.id}>{op.id} · {({pending:'待审批',approved:'已批准',executing:'执行中',done:'已返回结果',rejected:'已拒绝',failed:'失败',uncertain:'结果未知'} as Record<string,string>)[op.state]||'未知'} {op.error}</p>)}</details>
 </details></section>
}
