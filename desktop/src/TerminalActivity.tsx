import {useEffect,useRef,useState} from 'react';
import {Check,ChevronDown,ChevronRight,Clock,FileText,TriangleAlert} from 'lucide-react';
import {rpc} from './api';
import {Button} from './ui';
import {mergeActivity,type ActivityEvent,type ActivityItem} from './terminal-activity';
import './terminal-activity.css';

const names={running:'进行中',returned:'已返回',failed:'失败',waiting:'等待授权',interrupted:'已中断',unknown:'结果未确认'};
export default function TerminalActivity({workspaceId,sessionId,visible,exited,variant='strip'}:{workspaceId:string;sessionId:string;visible:boolean;exited:boolean;variant?:'strip'|'sidebar'}){
  const [items,setItems]=useState<ActivityItem[]>([]),[error,setError]=useState(''),[limited,setLimited]=useState(false),[open,setOpen]=useState(variant==='sidebar'),[retry,setRetry]=useState(0),[availability,setAvailability]=useState<'loading'|'enabled'|'disabled'>('loading');
  const cursor=useRef(0),follow=useRef(true),list=useRef<HTMLDivElement>(null);
  useEffect(()=>{
    if(!visible)return;let disposed=false,failures=0,timer:ReturnType<typeof setTimeout>;setError('');
    async function poll(){
      try{
        const r=await rpc<{enabled:boolean;events:ActivityEvent[];next:number;total:number;limited?:boolean;error?:{message:string}} >({cmd:'workbench.terminal.activity',workspace_id:workspaceId,session_id:sessionId,since:cursor.current});
        if(disposed)return;
        if(r.next<cursor.current){cursor.current=0;setItems([]);timer=setTimeout(poll,0);return}
        failures=0;cursor.current=r.next;setLimited(!!r.limited);setError(r.error?.message||'');setAvailability(r.enabled?'enabled':'disabled');
        if(r.events.length)setItems(old=>mergeActivity(old,r.events));
        if(r.enabled&&(!exited||r.next<r.total))timer=setTimeout(poll,r.next<r.total&&r.events.length?100:1000);
      }catch(e){if(!disposed){setError(`工作记录暂时不可用：${String(e)}`);if(++failures<2)timer=setTimeout(poll,3000)}}
    }
    void poll();return()=>{disposed=true;clearTimeout(timer)};
  },[workspaceId,sessionId,visible,exited,retry]);
  useEffect(()=>{if(follow.current&&list.current)list.current.scrollTop=list.current.scrollHeight},[items,open]);
  const current=[...items].reverse().find(r=>['running','waiting'].includes(r.status));
  if(variant==='strip'&&!items.length&&(!error||exited))return null;
  return <section className="terminal-activity" aria-label="Agent 工作记录">
    <header><Button aria-expanded={open} onClick={()=>setOpen(!open)}>{open?<ChevronDown size={14}/>:<ChevronRight size={14}/>}工作记录</Button><span className="activity-current">{error?'记录暂不可用':exited?'会话已结束':current?.title||''}</span>{items.length>0&&<small>{items.length} 项</small>}</header>
    {open&&<div className="activity-list" ref={list} onScroll={()=>{const e=list.current;if(e)follow.current=e.scrollHeight-e.scrollTop-e.clientHeight<40}}>
      {variant==='sidebar'&&!items.length&&!error&&<p className="activity-note" role="status">{availability==='loading'?'正在读取操作记录…':availability==='disabled'?'此会话尚未提供操作记录，请打开会话查看 CLI 输出。':exited?'会话已结束，没有操作记录。':'尚未收到工具操作。等待 CLI 上报，可打开会话查看完整输出。'}</p>}
      {items.map(item=><details key={item.id} className={`activity-item ${item.status}`}><summary>
        {item.status==='returned'?<Check size={14}/>:item.status==='failed'?<TriangleAlert size={14}/>:item.status==='running'||item.status==='waiting'?<Clock size={14}/>:<FileText size={14}/>}
        <span className="activity-title">{item.title}</span><span className="activity-state">{exited&&item.status==='running'?'结果未确认':names[item.status]}</span>
      </summary><div className="activity-detail"><small>{new Date(item.time).toLocaleTimeString()}{item.agentId?' · 子代理':''}</small>{item.input&&<><strong>操作内容</strong><pre>{item.input}</pre></>}{item.output&&<><strong>执行结果</strong><pre>{item.output}</pre></>}{!item.output&&<p>尚未收到结果，请结合终端查看当前状态。</p>}</div></details>)}
      {(limited||items.length===500)&&<p className="activity-note">{limited?'本次会话的工作记录已达到存储上限，后续操作请查看终端。':'显示最近 500 项操作。'}</p>}
      {error&&<div className="activity-note" role="status">{error}<Button onClick={()=>setRetry(v=>v+1)}>重新读取</Button></div>}
      {items.length>0&&<footer><Button onClick={()=>{follow.current=true;if(list.current)list.current.scrollTop=list.current.scrollHeight}}>最新操作 ↓</Button></footer>}
    </div>}
  </section>;
}
