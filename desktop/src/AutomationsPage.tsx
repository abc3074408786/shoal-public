import {useEffect,useRef,useState} from 'react';
import {ArrowUpRight,CalendarClock,FileText,History,Pause,Plus,RefreshCw,Search,SlidersHorizontal,X} from 'lucide-react';
import {Project,Task,rpc,phases} from './api';
import {Button,Input,NativeDialog,Tip} from './ui';
import {AutomationPanel} from './CapabilitiesPanel';
import './automations.css';

const templates=[
 {category:'仓库健康',name:'每日仓库审计',description:'检查依赖关系、失败的测试和有风险的变更。',minutes:1440},
 {category:'发布准备',name:'每周发布准备',description:'根据当前项目状态，整理每周发布风险摘要。',minutes:10080},
 {category:'定期评审',name:'每日变更回顾',description:'回顾最近的工作，指出正确性、体验和测试覆盖风险。',minutes:1440},
 {category:'维护',name:'每小时项目检查',description:'查找未完成的工作、陈旧的生成文件和失败的本地验证。',minutes:60},
];
type Automation={id:string;project_id:string;enabled:boolean;next_run:number;interval_minutes:number;last_error?:string;spec:{name:string;instruction:string;agent:string}};
type Run={id:string;automation_id:string;task_id?:string;started_at:number;state:string;error?:string};
const beijing=(epoch:number)=>Number.isFinite(epoch)?new Date(epoch*1000).toLocaleString('zh-CN',{timeZone:'Asia/Shanghai',hour12:false}):'时间不可用';
const shortTime=(epoch:number)=>Number.isFinite(epoch)?new Date(epoch*1000).toLocaleString('zh-CN',{timeZone:'Asia/Shanghai',month:'numeric',day:'numeric',hour:'2-digit',minute:'2-digit',hour12:false}):'时间不可用';
const schedule=(n:number)=>n===0?'仅一次':n===1440?'每天':n===10080?'每周':n===60?'每小时':n%1440===0?`每 ${n/1440} 天`:n%60===0?`每 ${n/60} 小时`:`每 ${n} 分钟`;
const phaseName=(phase:string)=>phases[phase]||({dispatching:'正在派发',failed:'派发失败'} as Record<string,string>)[phase]||phase;
const activePhases=['dispatching','queued','preparing','running','verifying'];
const attentionPhases=['failed','needs_attention','needs_recovery'];

export default function AutomationsPage({projects,onOpenTask}:{projects:Project[];onOpenTask:(id:string)=>Promise<void>}){
 const [data,setData]=useState<{automations:Automation[];runs:Run[]}>({automations:[],runs:[]});
 const [tasks,setTasks]=useState<Task[]>([]),[taskError,setTaskError]=useState('');
 const [busy,setBusy]=useState(false),[loaded,setLoaded]=useState(false),[error,setError]=useState('');
 const [search,setSearch]=useState(''),[projectFilter,setProjectFilter]=useState(''),[status,setStatus]=useState(''),[filters,setFilters]=useState(false),[history,setHistory]=useState(false),[selectedId,setSelectedId]=useState('');
 const [editor,setEditor]=useState<{initial?:any;projectId:string}|null>(null);
 const dialog=useRef<HTMLDialogElement>(null),returnFocus=useRef<HTMLElement|null>(null);
 const gitProjects=projects.filter(p=>!p.direct);
 async function refresh(){
  setBusy(true);setError('');
  const [list,snapshot]=await Promise.allSettled([rpc<{automations:Automation[];runs:Run[]}>({cmd:'automation.list'}),rpc<{tasks:Task[]}>({cmd:'workbench.snapshot'})]);
  if(list.status==='fulfilled'){setData(list.value);setLoaded(true)}else setError(String(list.reason));
  if(snapshot.status==='fulfilled'){setTasks(snapshot.value.tasks||[]);setTaskError('')}else{setTasks([]);setTaskError('任务详情暂不可用，请刷新重试。')}
  setBusy(false);
 }
 useEffect(()=>{void refresh()},[]);
 useEffect(()=>{if(editor&&!dialog.current?.open)dialog.current?.showModal()},[editor]);
 function close(){setEditor(null);void refresh();requestAnimationFrame(()=>returnFocus.current?.isConnected?returnFocus.current.focus():document.querySelector<HTMLButtonElement>('.automation-create')?.focus())}
 function create(t?:typeof templates[number]){returnFocus.current=document.activeElement as HTMLElement;setEditor({projectId:gitProjects.find(p=>p.id===projectFilter)?.id||gitProjects[0]?.id||'',initial:t?{id:'',enabled:false,next_run:Math.floor(Date.now()/1000)+3600,interval_minutes:t.minutes,spec:{name:t.name,instruction:t.description+' 输出发现、证据和建议，等待人工审查；不删除文件，不提交或推送。',agent:'codex',base:'HEAD',done:[],policy:{budget:{wall_clock:'1h'}}}}:undefined})}
 function edit(a:Automation){returnFocus.current=document.activeElement as HTMLElement;setEditor({initial:a,projectId:a.project_id})}
 async function openTask(id:string){setBusy(true);setError('');try{await onOpenTask(id)}catch(e){setError(String(e))}finally{setBusy(false)}}
 async function pause(a:Automation){setBusy(true);setError('');try{await rpc({cmd:'automation.pause',id:a.id});await refresh()}catch(e){setError(String(e));setBusy(false)}}
 const orderedRuns=[...data.runs].sort((a,b)=>b.started_at-a.started_at);
 const latest=(id:string)=>orderedRuns.find(r=>r.automation_id===id);
 const taskFor=(run?:Run)=>run?.task_id?tasks.find(t=>t.id===run.task_id):undefined;
 const stateFor=(run?:Run)=>run?(taskFor(run)?.phase||run.state):'';
 function resultText(run?:Run){
  if(!run)return '尚未运行，首次执行后显示结果。';
  const task=taskFor(run);
  if(task?.reason.trim())return task.reason;
  if(run.error?.trim())return run.error;
  if(task?.checks?.length){const failed=task.checks.filter(c=>c.passed===false).length,passed=task.checks.filter(c=>c.passed===true).length;return failed?`${failed} 项验证未通过，请查看检查输出。`:`${passed} / ${task.checks.length} 项验证通过。`}
  if(task?.verified)return '验证已通过，请打开任务审查结果。';
  return `当前状态：${phaseName(stateFor(run))}。${activePhases.includes(stateFor(run))?'任务仍在执行，尚无最终结论。':'打开任务查看完整内容。'}`;
 }
 function stateBadge(run?:Run){const state=stateFor(run);return <span className={'automation-state '+(attentionPhases.includes(state)?'attention':activePhases.includes(state)?'active':['review','delivered'].includes(state)?'complete':'')}>{run?phaseName(state):'尚未运行'}</span>}
 const needsAttention=(a:Automation)=>attentionPhases.includes(stateFor(latest(a.id)))||stateFor(latest(a.id))==='review'||taskFor(latest(a.id))?.status?.state==='waiting_permission';
 const visible=data.automations.filter(a=>(!projectFilter||a.project_id===projectFilter)&&(!status||(status==='attention'?needsAttention(a):(status==='enabled')===a.enabled))&&(a.spec.name+' '+a.spec.instruction+' '+(projects.find(p=>p.id===a.project_id)?.name||'')).toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));
 const runs=orderedRuns.filter(r=>visible.some(a=>a.id===r.automation_id));
 const selected=visible.find(a=>a.id===selectedId)||visible[0],selectedRun=selected?latest(selected.id):undefined,selectedTask=taskFor(selectedRun);
 const selectedProject=projects.find(p=>p.id===editor?.projectId);
 const enabled=data.automations.filter(a=>a.enabled),next=[...enabled].sort((a,b)=>a.next_run-b.next_run)[0];
 const clear=()=>{setProjectFilter('');setStatus('');setSearch('')};
 return <section className="automations-page automation-a" aria-labelledby="automations-title">
  <div className="automation-a-content">
  <header className="automations-heading"><div><div className="automation-eyebrow">AUTOMATION / OPERATIONS</div><h1 id="automations-title">让重复工作，按计划发生。</h1><p>管理每一项作业，随时知道它做到了哪一步。</p></div><Button className="primary automation-create" onClick={()=>create()}><Plus size={16}/>从工作区创建</Button></header>
  <div className="automation-overview" aria-label="自动化概况"><div><strong>{enabled.length}</strong><span>已启用</span></div><div><strong>{data.automations.filter(needsAttention).length}</strong><span>需要确认</span></div><div><strong>{data.automations.filter(a=>!a.enabled).length}</strong><span>已暂停</span></div><p className="automation-next">{next?<>下一项 · <b>{next.spec.name}</b></>:'暂无已启用日程'}</p></div>
  <div className="automations-toolbar"><div className="automation-view-switch" aria-label="筛选任务"><Button aria-pressed={!status} onClick={()=>{setStatus('');setHistory(false)}}>全部任务</Button><Button aria-pressed={status==='attention'} onClick={()=>{setStatus('attention');setHistory(false)}}>需我确认</Button><Button aria-pressed={status==='paused'} onClick={()=>{setStatus('paused');setHistory(false)}}>已暂停</Button></div><div className="automation-page-tools"><Button aria-pressed={history} onClick={()=>setHistory(!history)}><History size={14}/>{history?'返回作业':'运行记录'}</Button><Tip label="筛选自动化"><Button aria-label="筛选自动化" aria-expanded={filters} aria-controls="automation-filters" onClick={()=>setFilters(!filters)}><SlidersHorizontal size={14}/></Button></Tip><Tip label="刷新自动化"><Button aria-label="刷新自动化" disabled={busy} onClick={()=>void refresh()}><RefreshCw size={14}/></Button></Tip></div><label className="automations-search"><Search size={14}/><Input aria-label="搜索自动化" placeholder="搜索任务或工作区" value={search} onChange={e=>setSearch(e.target.value)}/></label></div>
  {filters&&<div id="automation-filters" className="automation-filters"><label>项目<select value={projectFilter} onChange={e=>setProjectFilter(e.target.value)}><option value="">全部项目</option>{projects.map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</select></label><label>状态<select value={status} onChange={e=>setStatus(e.target.value)}><option value="">全部状态</option><option value="enabled">已启用</option><option value="attention">需我确认</option><option value="paused">已暂停</option></select></label><Button onClick={clear}>清除筛选</Button></div>}
  {error&&<p role="alert" className="ide-error">{error}</p>}
  <div className="automations-body" aria-busy={busy}>
   {!loaded?<p className="automations-empty" role="status">{busy?'正在加载自动化…':'未能加载自动化，请重试。'}</p>:history?<section className="automation-history"><h2 className="automation-section-title">运行记录</h2>{runs.length?runs.map(r=><article className="automation-row" key={r.id}><History size={16}/><div><strong>{data.automations.find(a=>a.id===r.automation_id)?.spec.name||'自动化'}</strong><small>{beijing(r.started_at)} · {phaseName(stateFor(r))}</small><p className="automation-run-summary">{resultText(r)}</p></div><Button disabled={busy||!r.task_id} onClick={()=>r.task_id&&void openTask(r.task_id)}>打开任务<ArrowUpRight size={14}/></Button></article>):<p className="automations-empty">{search||projectFilter||status?'没有符合筛选条件的运行记录':'尚无运行记录'}</p>}</section>:<>
    {visible.length?<div className="automation-console"><section className="automations-list" aria-label="自动化作业"><div className="automation-list-head"><span>任务 / 最近结论</span><span>下次执行</span><span>最近状态</span></div>{visible.map(a=>{const run=latest(a.id);return <article className={'automation-row automation-job '+(a.id===selected?.id?'selected':'')} key={a.id}><Button className="automation-job-select" aria-pressed={a.id===selected?.id} onClick={()=>setSelectedId(a.id)}><span className="automation-job-main"><strong>{a.spec.name}</strong><small>{projects.find(p=>p.id===a.project_id)?.name||'项目不可用'} · {resultText(run)}</small></span><span className="automation-job-schedule"><b title={beijing(a.next_run)}>{a.enabled?shortTime(a.next_run):'已暂停'}</b><small>{schedule(a.interval_minutes)}</small></span>{stateBadge(run)}</Button></article>})}<p className="automation-list-foot">{visible.length} 项作业 · 点击一行查看目标、日程与结果</p></section>
     {selected&&<aside className="automation-inspector" aria-label="所选作业详情"><header><span className="automation-detail-caption">所选作业</span>{stateBadge(selectedRun)}<h2>{selected.spec.name}</h2><p>{projects.find(p=>p.id===selected.project_id)?.name||'项目不可用'}</p></header><div className="automation-detail-content"><section className="automation-result"><h3>最近结论</h3>{selectedRun&&<small>{beijing(selectedRun.started_at)}（北京时间）</small>}<p className="automation-result-text">{resultText(selectedRun)}</p>{taskError&&selectedRun&&<p className="automation-detail-warning" role="status">{taskError}</p>}{selectedTask&&<p className="automation-verification">{selectedTask.verified?'验证已通过':'验证尚未通过或未完成'}</p>}{selectedTask?.checks?.length?<details className="automation-checks"><summary>查看 {selectedTask.checks.length} 项验证结果</summary>{selectedTask.checks.map((c,i)=><div key={i}><b>{c.passed===true?'通过':c.passed===false?'未通过':'尚未完成'}</b><code>{c.command}</code><pre>{c.output||'暂无输出'}</pre></div>)}</details>:null}{selectedRun?.task_id&&<Button className="automation-open-task" disabled={busy} onClick={()=>void openTask(selectedRun.task_id!)}><FileText size={15}/>查看完整结果</Button>}</section>
      <dl className="automation-detail-grid"><dt>执行日程</dt><dd>{schedule(selected.interval_minutes)}</dd><dt>{selected.enabled?'下次执行':'原计划时间'}</dt><dd>{beijing(selected.next_run)}<small>{selected.enabled?'北京时间':'日程已暂停，不会按此时间触发'}</small></dd><dt>执行 Agent</dt><dd>{selected.spec.agent}</dd></dl><section className="automation-goal"><h3>执行目标</h3><p>{selected.spec.instruction}</p></section>{selected.last_error&&<p className="automation-error">最近调度提示：{selected.last_error}</p>}</div><footer><Button disabled={busy||!projects.some(p=>p.id===selected.project_id)} onClick={()=>edit(selected)}>编辑日程</Button>{selected.enabled?<Button disabled={busy} onClick={()=>void pause(selected)}><Pause size={14}/>暂停</Button>:<Button disabled={busy} onClick={()=>edit({...selected,enabled:true})}>恢复</Button>}<Button className="automation-run-once" disabled={busy||!projects.some(p=>p.id===selected.project_id)} onClick={()=>edit(selected)}>运行一次</Button></footer></aside>}
    </div>:<div className="automations-empty"><CalendarClock size={26}/><h2>{data.automations.length?'没有符合筛选条件的自动化':'把第一项重复工作安排起来'}</h2><p>{data.automations.length?'调整关键词或筛选条件，查看其他作业。':'选择一个项目，设置执行目标与时间。运行进展和结果会显示在这里。'}</p>{data.automations.length?<Button onClick={clear}>清除筛选</Button>:<Button onClick={()=>create()}><Plus size={14}/>新建自动化</Button>}</div>}
    {!search&&!status&&!projectFilter&&<details className="automation-template-section" open={data.automations.length===0?true:undefined}><summary>从模板开始<span>保留常用任务，按需预填配置</span></summary><div className="automation-templates">{templates.map(t=><Button className="automation-template" key={t.name} onClick={()=>create(t)}><small>{t.category}</small><strong>{t.name}</strong><span>{t.description}</span></Button>)}</div><Button className="automation-add" onClick={()=>create()}><Plus size={16}/>添加新的</Button><p className="automation-template-note">模板仅预填配置；保存或手动运行前仍需确认执行配置。</p></details>}
   </>}
  </div>
  </div>
  {editor&&<NativeDialog ref={dialog} className="capabilities-dialog automation-editor" aria-labelledby="automation-editor-title" onClose={close}><div className="dialog-heading"><h2 id="automation-editor-title">{editor.initial?.id?'配置自动化':'新建自动化'}</h2><Button aria-label="关闭自动化配置" onClick={close}><X size={16}/></Button></div><div className="dialog-body capability-content"><label>项目<select disabled={!!editor.initial?.id} value={editor.projectId} onChange={e=>setEditor({...editor,projectId:e.target.value})}><option value="" disabled>选择 Git 项目</option>{gitProjects.map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</select></label>{selectedProject?<AutomationPanel key={selectedProject.id} project={selectedProject} initial={editor.initial} onSaved={close} onCancel={close}/>:<p>请先从侧栏打开一个 Git 项目。自动化会在独立工作树中运行。</p>}</div></NativeDialog>}
 </section>;
}
