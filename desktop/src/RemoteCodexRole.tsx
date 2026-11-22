import {useEffect,useRef,useState} from 'react';
import {rpc} from './api';
import {LOCAL_TARGET} from './execution-target';
import {Button} from './ui';

export type RemoteCodexBinding={ssh_profile_id:string;ssh_profile_fingerprint:string;ssh_target_fingerprint?:string;remote_model?:string;provider_fingerprint?:string};
type Profile={id:string;name:string;host:string;user:string};
type Preview={profile:Profile;ssh_profile_id:string;ssh_profile_fingerprint:string;target:{hostname:string;user:string;ssh_target_fingerprint:string};provider:{model:string;provider:string;provider_fingerprint:string}};

/** Selects a saved profile and records only its reviewed identity/fingerprint. */
export default function RemoteCodexRole({value,onChange}:{value?:RemoteCodexBinding;onChange:(value?:RemoteCodexBinding)=>void}){
  const [profiles,setProfiles]=useState<Profile[]|null>(null),[error,setError]=useState(''),[pending,setPending]=useState(false),[stale,setStale]=useState(false),[target,setTarget]=useState('');
  const generation=useRef(0);
  useEffect(()=>{let live=true;void rpc<{profiles:Profile[]}>({cmd:'ssh.list'},LOCAL_TARGET).then(r=>{if(live)setProfiles(r.profiles)}).catch(e=>{if(live){setProfiles([]);setError('无法读取 SSH 配置：'+String(e))}});return()=>{live=false;generation.current++}},[]);
  useEffect(()=>{if(!value){setStale(false);setTarget('');return}let live=true;void rpc<Preview>({cmd:'ssh.hybrid.target.preview',id:value.ssh_profile_id},LOCAL_TARGET).then(r=>{if(live){setTarget(`${r.target.user}@${r.target.hostname} · Codex / ${r.provider.model}`);setStale(r.ssh_profile_fingerprint!==value.ssh_profile_fingerprint||r.target.ssh_target_fingerprint!==value.ssh_target_fingerprint||r.provider.provider_fingerprint!==value.provider_fingerprint||r.provider.model!==value.remote_model)}}).catch(()=>{if(live){setTarget('');setStale(true)}});return()=>{live=false}},[value?.ssh_profile_id,value?.ssh_profile_fingerprint,value?.ssh_target_fingerprint,value?.provider_fingerprint,value?.remote_model]);
  async function choose(id:string){
    const current=++generation.current;setError('');
    if(!id){setStale(false);setTarget('');setPending(false);onChange(undefined);return}
    setPending(true);
    try{const result=await rpc<Preview>({cmd:'ssh.hybrid.target.preview',id},LOCAL_TARGET);if(generation.current===current){onChange({ssh_profile_id:result.ssh_profile_id,ssh_profile_fingerprint:result.ssh_profile_fingerprint,ssh_target_fingerprint:result.target.ssh_target_fingerprint,remote_model:result.provider.model,provider_fingerprint:result.provider.provider_fingerprint});setTarget(`${result.target.user}@${result.target.hostname} · Codex / ${result.provider.model}`);setStale(false)}}
    catch(e){if(generation.current===current)setError('无法核对 SSH 配置：'+String(e))}
    finally{if(generation.current===current)setPending(false)}
  }
  const missing=value&&profiles!==null&&!profiles.some(p=>p.id===value.ssh_profile_id);
  return <div>
    <label>执行位置<select aria-label="执行位置" value={value?.ssh_profile_id||''} disabled={pending||profiles===null} onChange={e=>void choose(e.target.value)}>
      <option value="">本机 Codex</option>
      {missing&&<option value={value.ssh_profile_id} disabled>已保存的 SSH 配置不可用</option>}
      {profiles?.map(p=><option key={p.id} value={p.id}>{p.name} · {p.user?p.user+'@':''}{p.host}</option>)}
    </select></label>
    {profiles===null&&<p role="status">正在读取 SSH 配置…</p>}
    {profiles?.length===0&&!error&&<p>没有 SSH 配置；请先在“连接与工具”中添加。</p>}
    {pending&&<p role="status">正在核对所选配置…</p>}
    {value&&target&&!stale&&<p>已核对 SSH 目标：{target}</p>}
    {stale&&<p role="alert">SSH 配置已更改或不可用，请重新选择并核对。</p>}
    {stale&&value&&profiles?.some(p=>p.id===value.ssh_profile_id)&&<Button type="button" onClick={()=>void choose(value.ssh_profile_id)} disabled={pending}>重新核对配置</Button>}
    {value&&<p className="orch-muted">远程 Codex 支持实现与探索角色；使用已核对的 DeepSeek 模型，本机写入和命令须逐项审批。</p>}
    {error&&<p role="alert">{error}</p>}
  </div>;
}
