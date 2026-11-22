import {useEffect,useState} from 'react';
import {Check,Clock,RefreshCw} from 'lucide-react';
import {rpc} from './api';
import {Button} from './ui';
import {currentTarget,sameTarget} from './execution-target';
import {useExecutionTarget} from './ExecutionHostsPanel';
import {runStates} from './orchestration';
import './task-progress.css';

const states={planned:'待确认',pending:'待做',blocked:'等待前置',assigned:'已派发',in_progress:'正在做',awaiting_review:'已回报，待审查',waiting_gate:'等待你确认',completed:'已验收',failed:'需要处理',cancelled:'已停止',unknown:'状态未知'};
const providers:Record<string,string>={codex:'Codex','claude-code':'Claude',omp:'OMP',opencode:'OpenCode',gemini:'Gemini',kimi:'Kimi'};
type Step={id:string;title:string;status:keyof typeof states;provider:string;blocked_by:string[]};
type Run={id:string;goal:string;state:string;blocker?:string|null;confirmed:boolean;steps:Step[];completed:number;total:number};
function checked(value:unknown,workspace:string):Run[]{
  const data=value as {workspace_id?:string;runs?:Run[]};
  if(!data||data.workspace_id!==workspace||!Array.isArray(data.runs))throw Error('鱼群进度响应无效');
  const ids=new Set<string>();
  for(const run of data.runs){
    if(!run||typeof run.id!=='string'||ids.has(run.id)||typeof run.goal!=='string'||typeof run.state!=='string'||typeof run.confirmed!=='boolean'||!Array.isArray(run.steps)||run.steps.length>200||(run.blocker!=null&&typeof run.blocker!=='string'))throw Error('鱼群任务响应无效');
    ids.add(run.id);const steps=new Set<string>();
    for(const step of run.steps){if(!step||typeof step.id!=='string'||steps.has(step.id)||typeof step.title!=='string'||typeof step.provider!=='string'||!Object.hasOwn(states,step.status)||!Array.isArray(step.blocked_by)||step.blocked_by.some(id=>typeof id!=='string'))throw Error('鱼群步骤响应无效');steps.add(step.id)}
  }
  return data.runs;
}
export default function SwarmProgress({workspaceId}:{workspaceId:string}){
  const target=useExecutionTarget(),key=JSON.stringify([target,workspaceId]);
  const [receipt,setReceipt]=useState<{key:string;runs:Run[]|null;error:boolean}>(),[chosen,setChosen]=useState(''),[retry,setRetry]=useState(0);
  useEffect(()=>{
    const issued=currentTarget();let disposed=false,timer:ReturnType<typeof setTimeout>,failures=0;
    setReceipt({key,runs:null,error:false});
    async function poll(){try{
      const runs=checked(await rpc({cmd:'orchestration.progress',workspace_id:workspaceId},issued),workspaceId);
      if(disposed||!sameTarget(issued,currentTarget()))return;
      failures=0;setReceipt({key,runs,error:false});timer=setTimeout(poll,1500);
    }catch{if(!disposed&&sameTarget(issued,currentTarget())){failures++;setReceipt(old=>({key,runs:old?.key===key?old.runs:null,error:true}));if(failures<2)timer=setTimeout(poll,3000)}}}
    void poll();return()=>{disposed=true;clearTimeout(timer)};
  },[key,retry]);
  const data=receipt?.key===key?receipt:null,runs=data?.runs;
  const run=runs?.find(run=>run.id===chosen)||runs?.find(run=>!['accepted','stopped'].includes(run.state))||runs?.[0];
  const complete=run?.steps.filter(step=>step.status==='completed').length||0;
  return <section className="task-progress swarm-progress" aria-label="鱼群任务进度">
    <header><strong>鱼群任务</strong><Button className="task-progress-refresh" aria-label="重新读取鱼群进度" onClick={()=>setRetry(v=>v+1)}><RefreshCw size={13}/></Button></header>
    {data?.error&&<p className="task-progress-warning" role="status">鱼群进度读取中断{run?'，以下为上次记录':''}。可重新读取。</p>}
    {!data?.error&&!runs&&<p className="task-progress-empty">正在读取鱼群分派任务…</p>}
    {runs?.length===0&&<p className="task-progress-empty">此工作区尚无受管鱼群任务。原生 CLI 内部派出的子代理需由该 CLI 提供计划记录。</p>}
    {run&&<>
      {runs&&runs.length>1&&<select aria-label="选择鱼群任务" value={run.id} onChange={event=>setChosen(event.target.value)}>{runs.map(run=><option key={run.id} value={run.id}>{run.goal}</option>)}</select>}
      <h3>{run.goal}</h3><p className="task-progress-explanation">{runStates[run.state]||'状态未知'} · {run.confirmed?'已确认工作流':'方案尚未确认'}</p>
      {run.confirmed&&run.steps.length>0&&<><div className="task-progress-count"><strong>{complete}<span> / {run.steps.length} 项已验收</span></strong><span>{run.steps.length-complete} 项未验收</span></div><progress aria-label="已验收鱼群任务" value={complete} max={run.steps.length}/></>}
      {run.blocker&&<p className="task-progress-warning" role="status">需要处理：{run.blocker}</p>}
      {run.state==='awaiting_acceptance'&&<p className="task-progress-current">集成与整体验证已通过，等待你验收最终成果。</p>}
      <ol className="task-progress-steps" aria-label="鱼群分派步骤">{run.steps.map(step=><li key={step.id} data-step-state={step.status}><span className="step-mark" aria-hidden>{step.status==='completed'?<Check size={14}/>:<Clock size={13}/>}</span><div><strong>{step.title}</strong><small>{providers[step.provider]||step.provider}{step.blocked_by.length>0&&` · 等待 ${step.blocked_by.join('、')}`}</small><small>{states[step.status]}</small></div></li>)}</ol>
      <footer>按已确认分派任务计数；执行回报与总体交付分别核验。</footer>
    </>}
  </section>;
}
