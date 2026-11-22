import {useCallback,useEffect,useRef,useState} from 'react';
import {CheckCircle2,Clock3,ExternalLink,GitPullRequest,ListChecks,RefreshCw,XCircle} from 'lucide-react';
import {Button,Tip} from './ui';
import {openSourceUrl} from './task-sources';
import {readPrChecks,type PrChecksState,type CheckStatus} from './pr-checks';
import './pr-checks.css';

const labels:Record<CheckStatus,string>={success:'通过',pending:'进行中',failure:'失败',cancelled:'取消',skipped:'跳过',unknown:'未知'};
const order:CheckStatus[]=['failure','pending','cancelled','unknown','success','skipped'];
export default function PullRequestChecks({workspaceId}:{workspaceId:string}){
  const [state,setState]=useState<PrChecksState|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[selected,setSelected]=useState<number|null>(null);
  const alive=useRef(false),version=useRef(0),running=useRef(false);
  const refresh=useCallback(async()=>{
    if(running.current)return;
    running.current=true;const request=++version.current;setBusy(true);setError('');
    try{const value=await readPrChecks(workspaceId);if(alive.current&&request===version.current){setState(value);setSelected(old=>value.pull_requests?.some(p=>p.number===old)?old:value.pull_requests?.[0]?.number??null)}}
    catch{if(alive.current&&request===version.current)setError('读取失败，请重试。上次结果可能已过期。')}
    finally{if(request===version.current){running.current=false;if(alive.current)setBusy(false)}}
  },[workspaceId]);
  useEffect(()=>{alive.current=true;running.current=false;setState(null);setSelected(null);void refresh();const changed=()=>void refresh();window.addEventListener('velella:git-changed',changed);return()=>{alive.current=false;version.current++;running.current=false;window.removeEventListener('velella:git-changed',changed)}},[refresh]);
  const pull=state?.pull_requests?.find(p=>p.number===selected);
  useEffect(()=>{if(!pull?.summary.pending)return;const timer=setInterval(()=>{if(document.visibilityState==='visible')void refresh()},30000);return()=>clearInterval(timer)},[pull?.summary.pending,refresh]);
  async function open(url:string){try{await openSourceUrl(url)}catch{setError('无法打开检查链接，请稍后重试。')}}
  return <section className="pr-checks-panel" aria-label="评审检查" aria-busy={busy}>
    <div className="panel-toolbar"><ListChecks size={15}/><strong>评审检查</strong><Tip label="刷新评审检查"><Button aria-label="刷新评审检查" disabled={busy} onClick={()=>void refresh()}><RefreshCw size={14} className={busy?'pr-checks-spinning':''}/></Button></Tip></div>
    {error&&<p className="pr-checks-error" role="alert">{error}</p>}
    {!state?<div className="pr-checks-empty" role="status"><Clock3 size={24}/><p>{error?'暂时无法显示检查':'正在读取 PR 检查…'}</p></div>:state.state!=='ready'?<div className="pr-checks-empty" role="status"><GitPullRequest size={28}/><h3>{state.state==='not_git'?'尚未打开 Git 仓库':state.state==='no_pr'?'尚未关联 PR':state.state==='auth_required'?'需要连接 GitHub':state.state==='remote_unsupported'?'远端主机暂不支持评审检查':state.state==='error'?'检查读取失败':'暂时无法显示检查'}</h3><p>{state.message}</p>{state.branch&&<small>分支：{state.branch}</small>}</div>:pull&&<>
      {(state.pull_requests?.length??0)>1&&<label className="pr-checks-select">关联 PR<select aria-label="选择关联 PR" value={selected??''} onChange={event=>setSelected(Number(event.target.value))}>{state.pull_requests!.map(pr=><option key={pr.number} value={pr.number}>#{pr.number} · {pr.title}</option>)}</select></label>}
      <div className="pr-checks-heading"><Button className="pr-checks-link" onClick={()=>void open(pull.url)}><GitPullRequest size={15}/><span>#{pull.number} {pull.title}</span><ExternalLink size={12}/></Button><div className="pr-checks-meta"><span>{pull.state==='MERGED'?'已合并':pull.state==='CLOSED'?'已关闭':pull.draft?'草稿':'进行中'}</span><span title={pull.head_sha}>{pull.head_sha.slice(0,8)}</span></div><p title={`${pull.head_branch} → ${pull.base_branch}`}>{pull.head_branch} → {pull.base_branch}</p>{pull.review_decision&&<p>{pull.review_decision==='APPROVED'?'代码评审已批准':pull.review_decision==='CHANGES_REQUESTED'?'代码评审要求修改':'等待代码评审'}</p>}</div>
      {pull.checks.length?<><div className="pr-checks-summary" aria-label="检查结果汇总">{order.filter(status=>pull.summary[status]>0).map(status=><span key={status} data-status={status}>{pull.summary[status]} {labels[status]}</span>)}</div><ul className="pr-checks-list">{[...pull.checks].sort((a,b)=>order.indexOf(a.status)-order.indexOf(b.status)).map(check=><li key={check.id} data-status={check.status}><span className="pr-checks-icon" aria-hidden="true">{check.status==='success'?<CheckCircle2 size={15}/>:check.status==='pending'?<Clock3 size={15}/>:<XCircle size={15}/>}</span><div><strong>{check.name}</strong>{check.workflow&&<small>{check.workflow}</small>}<span className="pr-checks-state">{check.label}</span></div>{check.url&&<Button aria-label={`查看检查 ${check.name}`} title="打开检查详情" onClick={()=>void open(check.url!)}><ExternalLink size={14}/></Button>}</li>)}</ul></>:<div className="pr-checks-empty"><CheckCircle2 size={24}/><h3>尚无检查结果</h3><p>此 PR 的当前提交尚未产生自动检查记录。</p></div>}
      <footer className="pr-checks-footer">更新于 {new Date(state.checked_at).toLocaleTimeString('zh-CN',{hour12:false})}{pull.summary.pending>0?' · 进行中的检查每 30 秒刷新':''}<span>仅显示检查结果；合并条件以 GitHub 为准。</span></footer>
    </>}
  </section>;
}
