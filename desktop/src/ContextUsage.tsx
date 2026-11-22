import {Button} from './ui';

export type ContextObservation={
  session_id:string;workspace_id:string;provider:string;state:string;
  receipt?:{source:string;basis:string;tokens:number|null;context_window:number|null;percent:number|null;observed_at:string;handoff_required:boolean}|null;
};
type Handoff={state?:string;from_session?:string;to_session?:string;package?:unknown};
export default function ContextUsage({observations,handoffs={},onOpen}:{observations:ContextObservation[];handoffs?:Record<string,Handoff>;onOpen:(workspace:string,session:string)=>void}){
  return <details className="orch-report"><summary>会话上下文与交接详情</summary>
    <p className="orch-muted">最近一次原生观测；缺失数据保持未知。达到 40% 后停止后续自动输入，当前回合可完成。空闲后会持久保存交接包并启动同一成员的替代会话；登录或信任提示仍须由用户在该隔离会话中处理。</p>
    {observations.length===0?<p>尚无会话用量观测</p>:observations.map(item=><div key={item.session_id}>
      <p><strong>{item.provider||'CLI'}</strong> · {item.receipt?.percent==null?'上下文未知':item.receipt.percent.toFixed(1)+'% · '+item.receipt.tokens?.toLocaleString()+' / '+item.receipt.context_window?.toLocaleString()+' tokens'}
        {item.receipt?.handoff_required&&' · 已达到交接阈值'}
      </p>
      {item.receipt?<p className="orch-muted">{item.receipt.basis==='native_estimate'?'原生估算':'最近 API 输入用量'} · {item.receipt.source} · {new Date(item.receipt.observed_at).toLocaleString()}</p>:<p className="orch-muted">{item.state==='unavailable'?'用量回执不可读取':'尚未获得可核验的原生用量来源'}</p>}
      <Button onClick={()=>onOpen(item.workspace_id,item.session_id)}>打开对应会话</Button>
    </div>)}
    {Object.entries(handoffs).map(([member,handoff])=><p key={member} className="orch-muted">交接 {member.slice(0,8)}：{handoff.state||'已准备'}{handoff.to_session?' · 已迁移至替代会话':''}</p>)}
  </details>;
}
