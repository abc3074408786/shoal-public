import {useEffect,useRef,useState} from 'react';
import {Monitor,X,Server,Smartphone,Unplug,Trash2} from 'lucide-react';
import {executionHosts,rpc} from './api';
import {useConfirm} from './ConfirmAction';
import {LOCAL_TARGET,currentTarget,selectTarget,subscribeTarget,type ExecutionTarget} from './execution-target';
import {Button,Input,NativeDialog} from './ui';

type Host={id:string;revision:number;name:string;ssh_profile_id:string;data_root:string|null;confirmation:{server_id:string;data_root:string;os:string;profile_fingerprint:string}|null};
type Connection={host_id:string;inspection_id:string;state:string;identity:any;target:ExecutionTarget|null};
type View={hosts:Host[];connections:Connection[];profiles:{id:string;name:string;host:string;user?:string;port?:number}[]};
const blank={id:'',expected_revision:'',name:'',ssh_profile_id:'',data_root:''};
const label=(profile?:{host:string;user?:string;port?:number})=>profile?(profile.user?profile.user+'@':'')+profile.host+(profile.port?':'+profile.port:''):'SSH 配置已删除';

export function useExecutionTarget(){
  const [target,setTarget]=useState<ExecutionTarget>(()=>({...currentTarget()}));
  useEffect(()=>subscribeTarget(next=>setTarget({...next})),[]);
  return target;
}

export default function ExecutionHostsPanel({onClose}:{onClose:()=>void}){
  const ask=useConfirm(),dialog=useRef<HTMLDialogElement>(null),[data,setData]=useState<View>({hosts:[],connections:[],profiles:[]});
  const [form,setForm]=useState<typeof blank>(blank),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
  const [review,setReview]=useState<{host:Host;inspection_id:string;identity:any;profile:any;confirmation_valid:boolean}|null>(null);
  const [ack,setAck]=useState(false),[active,setActive]=useState<ExecutionTarget>(()=>({...currentTarget()}));
  useEffect(()=>{dialog.current?.showModal()},[]);
  useEffect(()=>subscribeTarget(next=>setActive({...next})),[]);
  async function refresh(){
    const [managed,ssh]=await Promise.all([executionHosts<any>({action:'list'}),rpc({cmd:'ssh.list'},LOCAL_TARGET)]);
    setData({hosts:managed.hosts||[],connections:managed.connections||[],profiles:ssh.profiles||[]});
  }
  useEffect(()=>{void run(refresh)},[]);
  async function run(action:()=>Promise<void>){if(busy)return;setBusy(true);setError('');try{await action()}catch(e){setError(String(e))}finally{setBusy(false)}}
  function edit(host?:Host){
    setAck(false);
    setForm(host?{id:host.id,expected_revision:String(host.revision),name:host.name,ssh_profile_id:host.ssh_profile_id,data_root:host.data_root||''}:blank);
    setReview(null);
  }
  const connection=(id:string)=>data.connections.find(c=>c.host_id===id);
  const field=(key:'name'|'data_root',text:string,placeholder='')=><label>{text}<Input value={form[key]} placeholder={placeholder} onChange={e=>{setForm({...form,[key]:e.target.value});setReview(null);setAck(false)}}/></label>;
  return <NativeDialog ref={dialog} className="capabilities-dialog execution-hosts-dialog" aria-labelledby="execution-hosts-title" onClose={onClose} onCancel={e=>{if(busy)e.preventDefault()}}>
    <div className="dialog-heading"><div className="creation-heading-copy"><h2 id="execution-hosts-title">执行主机</h2><p>SSH 主机上的项目、工作树、准备脚本和 Agent 由远端 Velella runtime 执行；断线只关闭本机通道，不会停止远端任务。</p></div><Button aria-label="关闭执行主机" disabled={busy} onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body capability-content">
      <p className="capability-note">远端需事先安装兼容的 <code>velella-runtime</code> 并加入 SSH 非交互环境 PATH，凭据仍由系统 OpenSSH 或 ssh-agent 管理。首次信任、密钥认证和目标数据根都需要在此明确确认；远端持续运行尚未核验，请先在远端独立登录会话中启动 runtime。</p>
      <div className="capability-list">{data.hosts.map(host=>{const c=connection(host.id),profile=data.profiles.find(p=>p.id===host.ssh_profile_id);
        return <div className="capability-item" key={host.id}><div><strong>{host.name}<small className="execution-host-state">{c?.state==='connected'?'已连接':c?c.state:'未连接'}</small></strong><small>{label(profile)} · 数据根 {host.data_root||'远端默认'}</small>{host.confirmation&&<small>已信任服务器 {host.confirmation.server_id}（{host.confirmation.os}）</small>}{c?.state==='awaiting_confirmation'&&host.confirmation&&<small role="status">服务器身份与已记录的不一致，需要重新确认。</small>}</div>
          <div className="capability-row">
            {c?.state==='connected'?<Button disabled={busy} onClick={()=>run(async()=>{await executionHosts({action:'disconnect',id:host.id});await refresh()})}><Unplug size={14}/>断开</Button>
              :<Button disabled={busy||c?.state==='connecting'||c?.state==='inspecting'} onClick={()=>run(async()=>{const r=await executionHosts<any>({action:'inspect',id:host.id});setReview({host,inspection_id:r.inspection_id,identity:r.identity,profile:r.profile,confirmation_valid:!!host.confirmation});setAck(false)})}>检查</Button>}
            {c?.state==='connected'?!active||active.kind==='local'||active.host_id!==host.id
              ?<Button disabled={busy} onClick={()=>{selectTarget(c.target!);setMessage('后续请求将使用 '+host.name+'；已发起的请求仍属于原主机。')}}><Monitor size={14}/>切到此主机</Button>
              :<Button disabled={busy} onClick={()=>selectTarget(LOCAL_TARGET)}><Server size={14}/>切回本机</Button>:null}
            <Button disabled={busy} onClick={()=>edit(host)}>编辑</Button>
            <Button disabled={busy} onClick={()=>run(async()=>{if(!await ask('删除执行主机记录只会移除本机保存的信任信息，远端项目和任务不会停止。确认删除？'))return;await executionHosts({action:'delete',id:host.id,confirmed:true,expected_revision:host.revision});if(review?.host.id===host.id)setReview(null);await refresh()})}><Trash2 size={14}/>删除记录</Button>
          </div></div>;})}
        {!data.hosts.length&&<p>尚未登记执行主机。先在“工作区工具 › SSH 主机”保存 OpenSSH 配置，再在此把它登记为执行主机。</p>}</div>
      {review&&<section className="capability-section"><h3>连接确认</h3>
        <pre>{JSON.stringify({host:review.host.name,ssh:label(review.profile||data.profiles.find(p=>p.id===review.host.ssh_profile_id)),server_id:review.identity?.server_id,os:review.identity?.os,data_root:review.identity?.data_root,runtime_version:review.identity?.runtime_version,persistence:review.identity?.persistence},null,2)}</pre>
        <p className="capability-note">检查只读取远端身份，不创建项目、不启动任务。{review.identity?.persistence==='prestarted_unverified'?'此入口不会自动启动远端 daemon，也不会上传二进制、代码或认证配置。':''}</p>
        <label className="ack"><input type="checkbox" checked={ack} onChange={e=>setAck(e.target.checked)}/>我确认上面的 SSH 目标、服务器身份和远端数据根，允许在该主机读写项目并启动 Agent。</label>
        <div className="capability-row"><Button disabled={busy||!ack} onClick={()=>run(async()=>{const r=await executionHosts<any>({action:'connect',id:review.host.id,inspection_id:review.inspection_id});setReview(null);setAck(false);await refresh();if(r.target)selectTarget(r.target);setMessage('已连接 '+review.host.name+'，工作台现在显示该主机的项目。')})}>信任并连接</Button><Button disabled={busy} onClick={()=>run(async()=>{await executionHosts({action:'disconnect',id:review.host.id});setReview(null);await refresh()})}>取消</Button></div></section>}
      <h3>{form.id?'编辑执行主机':'登记执行主机'}</h3>
      <form onSubmit={e=>{e.preventDefault();void run(async()=>{const saved=await executionHosts<any>({action:'save',host:{...(form.id?{id:form.id,expected_revision:Number(form.expected_revision)}:{}),name:form.name.trim(),ssh_profile_id:form.ssh_profile_id,data_root:form.data_root.trim()||null}});
        edit();await refresh();setMessage('已保存记录：'+saved.host.name+'。保存不会建立连接，也不会执行任何远端命令。')})}} onChange={()=>{setReview(null);setAck(false)}}>
        <div className="capability-grid">
          <label>名称<Input required value={form.name} maxLength={60} onChange={e=>{setForm({...form,name:e.target.value});setReview(null)}}/></label>
          <label>SSH 配置<select required value={form.ssh_profile_id} onChange={e=>{setForm({...form,ssh_profile_id:e.target.value});setReview(null)}}><option value="">选择已有配置</option>{data.profiles.map(p=><option key={p.id} value={p.id}>{p.name} · {label(p)}</option>)}</select></label>
          {field('data_root','远端数据根（可留空使用远端默认）','/srv/velella 或 D:\\Velella')}
        </div>
        <p className="capability-note">数据根按远端系统的路径语义校验，本机不会替它规范化；修改 SSH 配置或数据根会清除已保存的服务器身份，需要重新检查确认。</p>
        <div className="capability-row"><Button disabled={busy||!form.ssh_profile_id}>{form.id?'保存修改':'登记执行主机'}</Button>{form.id&&<Button type="button" onClick={()=>edit()}>取消编辑</Button>}</div>
      </form>
      {error&&<p role="alert" className="ide-error">{error}</p>}
      {message&&<p role="status">{message}</p>}
    </div>
  </NativeDialog>;
}
