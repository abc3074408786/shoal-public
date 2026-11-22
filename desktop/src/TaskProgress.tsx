import {useEffect,useState} from 'react';
import {Check,ChevronRight,Clock,RefreshCw,TriangleAlert} from 'lucide-react';
import {rpc} from './api';
import {Button} from './ui';
import {useExecutionTarget} from './ExecutionHostsPanel';
import {currentTarget,sameTarget} from './execution-target';
import './task-progress.css';

interface Step{id:string;title:string;active_form?:string;status:'pending'|'in_progress'|'completed';blocked_by:string[]}
interface ProgressSnapshot{
  state:'ready'|'no_plan'|'scanning'|'unbound'|'unavailable'|'unsupported';
  task_label?:string|null;steps:Step[];completed?:number;total?:number;
  explanation?:string|null;reported_at?:string|null;source?:string|null;reason?:string|null;
  plan_scope?:'session';
}
const labels={pending:'待做',in_progress:'正在做',completed:'已完成'};
const stateLabels={ready:'任务计划',no_plan:'CLI 尚未上报本轮计划',scanning:'正在读取任务计划',unbound:'等待绑定原生会话',unavailable:'暂时无法读取任务计划',unsupported:'此 CLI 的计划尚未接入'};
function checkedSnapshot(value:unknown,sessionId:string,nativeId?:string|null):ProgressSnapshot{
  const data=value as ProgressSnapshot&{session_id?:string;native_id?:string};
  if(!data||data.session_id!==sessionId||!Object.hasOwn(stateLabels,data.state)||!Array.isArray(data.steps)||data.steps.length>200)throw Error('任务计划响应无效');
  if(nativeId&&data.state==='ready'&&data.native_id!==nativeId)throw Error('任务计划会话不匹配');
  const ids=new Set<string>();
  for(const step of data.steps){
    if(!step||typeof step.id!=='string'||ids.has(step.id)||typeof step.title!=='string'||!Object.hasOwn(labels,step.status)||!Array.isArray(step.blocked_by)||step.blocked_by.some(id=>typeof id!=='string')||(step.active_form!=null&&typeof step.active_form!=='string'))throw Error('任务步骤响应无效');
    ids.add(step.id);
  }
  if(data.state==='ready'&&!data.steps.length)throw Error('任务计划没有步骤');
  for(const field of ['task_label','explanation','reported_at','reason'] as const)if(data[field]!=null&&typeof data[field]!=='string')throw Error('任务计划字段无效');
  return data;
}

export default function TaskProgress({workspaceId,sessionId,nativeId,taskName}:{workspaceId:string;sessionId:string;nativeId?:string|null;taskName?:string}){
  const target=useExecutionTarget(),key=JSON.stringify([target,workspaceId,sessionId,nativeId]);
  const [receipt,setReceipt]=useState<{key:string;data:ProgressSnapshot|null;error:string}>(),[retry,setRetry]=useState(0);
  useEffect(()=>{
    const issued=currentTarget();let disposed=false,timer:ReturnType<typeof setTimeout>,failures=0;
    setReceipt({key,data:null,error:''});
    async function poll(){
      try{
        const data=checkedSnapshot(await rpc<unknown>({cmd:'workbench.terminal.progress',workspace_id:workspaceId,session_id:sessionId},issued),sessionId,nativeId);
        if(disposed||!sameTarget(issued,currentTarget()))return;
        failures=0;
        setReceipt({key,data,error:''});
        if(data.state!=='unsupported')timer=setTimeout(poll,data.state==='scanning'?500:data.state==='unavailable'?3000:1500);
      }catch(error){if(!disposed&&sameTarget(issued,currentTarget())){
        failures++;setReceipt(old=>({key,data:old?.key===key?old.data:null,error:String(error)}));
        if(failures<2)timer=setTimeout(poll,3000);
      }}
    }
    void poll();return()=>{disposed=true;clearTimeout(timer)};
  },[key,retry]);
  const data=receipt?.key===key?receipt.data:null,error=receipt?.key===key?receipt.error:'';
  const ready=data?.state==='ready',steps=ready?data.steps:[],completed=steps.filter(s=>s.status==='completed').length;
  const active=steps.filter(s=>s.status==='in_progress'),remaining=steps.filter(s=>s.status!=='completed');
  const blocked=steps.filter(s=>s.status!=='completed'&&s.blocked_by.some(id=>!steps.some(other=>other.id===id&&other.status==='completed')));
  const complete=ready&&steps.length>0&&completed===steps.length;
  return <section className="task-progress" aria-label="整项任务进度">
    <header><strong>任务进度</strong><Button className="task-progress-refresh" aria-label="重新读取任务进度" onClick={()=>setRetry(v=>v+1)}><RefreshCw size={13}/></Button></header>
    {error&&<p className="task-progress-warning" role="status"><TriangleAlert size={14}/><span>进度读取中断{ready?'，以下为上次计划':''}。可重新读取。</span></p>}
    {!data&&!error&&<p className="task-progress-empty" role="status">正在读取此会话的任务计划…</p>}
    {data&&!ready&&<div className="task-progress-empty" role="status"><strong>{stateLabels[data.state]}</strong><p>{data.reason||'请让 CLI 先列出任务步骤，并在执行时更新完成情况。'}</p></div>}
    {ready&&<>
      {(taskName||data.task_label)&&<h3>{data.task_label||taskName}</h3>}
      <div className="task-progress-count"><strong>{completed}<span> / {steps.length} 步已完成</span></strong><span>{complete?'CLI 上报全部完成':`${remaining.length} 步未完成`}</span></div>
      <progress aria-label="已完成任务步骤" value={completed} max={steps.length}/>
      <div className="task-progress-current"><span>{error?'上次进行到':complete?'本轮计划':active.length?'正在做':'接下来'}</span><p>{complete?'所有计划步骤已标记完成，实际结果仍需核验。':(active.length?active:remaining.slice(0,1)).map(s=>s.active_form||s.title).join('；')||'等待 CLI 更新下一步。'}</p></div>
      {blocked.length>0&&<p className="task-progress-warning"><Clock size={14}/><span>{blocked.length} 个步骤等待前置任务完成。</span></p>}
      <ol className="task-progress-steps" aria-label="任务步骤">{steps.map(step=>{
        const waiting=step.status!=='completed'&&step.blocked_by.filter(id=>!steps.some(other=>other.id===id&&other.status==='completed'));
        return <li key={step.id} data-step-state={step.status}><span className="step-mark" aria-hidden>{step.status==='completed'?<Check size={14}/>:step.status==='in_progress'?<ChevronRight size={14}/>:<Clock size={13}/>}</span><div><strong>{step.title}</strong>{waiting&&waiting.length>0&&<small>等待步骤 {waiting.join('、')}</small>}</div><span className="step-status">{labels[step.status]}</span></li>;
      })}</ol>
      {data.explanation&&<p className="task-progress-explanation">{data.explanation}</p>}
      <footer><span>{data.plan_scope==='session'?'ZCode 会话任务清单':'步骤状态由 CLI 上报'}</span>{data.reported_at&&<time>更新于 {new Date(data.reported_at).toLocaleTimeString('zh-CN',{hour12:false})}</time>}</footer>
    </>}
  </section>;
}
