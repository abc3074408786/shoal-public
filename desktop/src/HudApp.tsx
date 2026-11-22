import {useEffect,useRef,useState} from 'react';
import {Activity,ExternalLink,X} from 'lucide-react';
import {invoke} from './desktop-platform';
import type {HudDigest,HudTask} from './hud-digest';
import {HUD_STUCK_MS} from './hud-digest';

const ACTIVE_PHASES=new Set(['verifying','running','preparing','queued']);
const STALE_TOOLTIP_MS=15000;

function hud(command:string,args:Record<string,unknown>={}){
  return invoke('hud',{action:command,...args}).catch(()=>{});
}
function minutes(ms:number){
  const value=Math.floor(ms/60000);
  if(value<1)return '不到 1 分钟';
  if(value<60)return `${value} 分钟`;
  return `${Math.floor(value/60)} 小时 ${value%60} 分钟`;
}
// A waiting CLI is not stuck; only an actively running phase that stopped
// producing turns/bytes/state changes deserves the stall warning.
function taskHudState(task:HudTask){
  if(task.phase==='needs_attention'||task.phase==='needs_recovery')return {tone:'danger',label:task.phase==='needs_recovery'?'待恢复':'需要处理',stalled:false};
  if(task.human_control)return {tone:'muted',label:'人工接管中',stalled:false};
  if(task.state==='waiting_permission')return {tone:'amber',label:'等待权限确认',stalled:false};
  if(task.state==='waiting_input')return {tone:'amber',label:'等待输入指令',stalled:false};
  if(ACTIVE_PHASES.has(task.phase)&&task.state==='running'){
    const stalled=task.since_changed>HUD_STUCK_MS;
    return {tone:stalled?'danger':'live',label:stalled?'疑似卡住':'执行中',stalled};
  }
  if(task.phase==='queued')return {tone:'muted',label:'排队中',stalled:false};
  return {tone:'live',label:task.phase_label,stalled:false};
}

function TaskCard({task,onOpen}:{task:HudTask;onOpen:(id:string)=>void}){
  const hudState=taskHudState(task);
  return <button className="hud-task" onClick={()=>onOpen(task.id)} title={task.reason}>
    <span className="hud-task-head">
      <strong className="hud-task-name">{task.name}</strong>
      <span className={`hud-pill tone-${hudState.tone}`}>{hudState.label}</span>
      <span className="hud-pill tone-phase">{task.phase_label}</span>
    </span>
    <span className="hud-task-reason">{task.reason||'—'}</span>
    <span className="hud-task-meta">
      {task.state==='running'&&task.tool&&<span className="hud-tool">工具 · {task.tool}</span>}
      {task.state==='running'&&task.turns>0&&<span>第 {task.turns} 轮</span>}
      {hudState.stalled&&<span className="hud-stalled">无进展 {minutes(task.since_changed)}</span>}
      {task.project&&<span className="hud-project">{task.project}</span>}
    </span>
  </button>;
}

export default function HudApp(){
  const [digest,setDigest]=useState<HudDigest|null>(null),[ready,setReady]=useState(false);
  const root=useRef<HTMLDivElement>(null);
  useEffect(()=>{
    if(!window.velellaDesktop)return;
    setReady(true);
    const off=window.velellaDesktop.listen('velella:hud-state',event=>setDigest(event.payload as HudDigest));
    void hud('ready');
    return()=>off();
  },[]);
  // Follow the content height so one quiet task does not leave a tall empty frame.
  useEffect(()=>{
    if(!root.current)return;
    const height=Math.ceil(root.current.getBoundingClientRect().height);
    if(height>0)void hud('set_height',{height});
  },[digest]);
  return <div className="hud-root" ref={root}>
    <header className="hud-titlebar" data-hud-drag>
      <span className={`hud-dot ${digest?.connected?'online':'offline'}`} aria-hidden/>
      <span className="hud-title"><Activity size={13}/> 任务监视</span>
      <button className="hud-icon" aria-label="隐藏监视窗" onClick={()=>void hud('hide')}><X size={14}/></button>
    </header>
    <div className="hud-body">
      {!ready&&<p className="hud-note">监视窗仅在桌面应用中可用。</p>}
      {ready&&digest&&!digest.connected&&<p className="hud-note stale" role="status">连接已断开，显示最后已知状态。</p>}
      {ready&&digest&&digest.tasks.length===0&&<p className="hud-note">暂无进行中的任务。</p>}
      {ready&&!digest&&<p className="hud-note">正在等待主窗口的数据…</p>}
      {ready&&digest&&digest.tasks.map(task=><TaskCard key={task.id} task={task} onOpen={id=>void hud('open_task',{task_id:id})}/>)}
    </div>
    <footer className="hud-footer">
      <button className="hud-link" onClick={()=>void hud('open_main')}><ExternalLink size={12}/> 打开主窗口</button>
      {digest&&<small className={Date.now()-digest.ts>STALE_TOOLTIP_MS?'hud-muted':'hud-quiet'}>更新于 {new Date(digest.ts).toLocaleTimeString()}</small>}
    </footer>
  </div>;
}
