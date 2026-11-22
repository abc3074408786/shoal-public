import {useEffect,useRef,useState} from 'react';
import {CaseSensitive,Check,GitBranch,Github,Gitlab,Search,Sparkles,Ticket,X} from 'lucide-react';
import {Project,rpc} from './api';
import {Button,Input} from './ui';
import {Provider,providers,SourceItem,TaskSource} from './task-sources';

type Mode='smart'|'branch'|'name'|Provider;
const modes:{id:Mode;label:string;icon:typeof Search}[]=[
  {id:'smart',label:'智能',icon:Sparkles},{id:'github',label:'GitHub',icon:Github},
  {id:'linear',label:'Linear',icon:Ticket},{id:'gitlab',label:'GitLab',icon:Gitlab},
  {id:'jira',label:'Jira',icon:Ticket},{id:'branch',label:'分支',icon:GitBranch},{id:'name',label:'名称',icon:CaseSensitive},
];

export default function CreationSourcePicker({project,selected,name,base,disabled,onSelect,onName,onBase,onLoading,onSources}:{
  project:Project;selected?:SourceItem;name:string;base:string;disabled:boolean;
  onSelect:(item?:SourceItem)=>void;onName:(name:string)=>void;onBase:(base:string)=>void;
  onLoading:(loading:boolean)=>void;onSources?:()=>void;
}){
  const [mode,setMode]=useState<Mode>('smart'),[query,setQuery]=useState('');
  const [sources,setSources]=useState<TaskSource[]>([]),[branches,setBranches]=useState<string[]>([]);
  const [items,setItems]=useState<SourceItem[]>([]),[loading,setLoading]=useState(false),[error,setError]=useState(''),[catalogError,setCatalogError]=useState('');
  const [defaultBase,setDefaultBase]=useState('正在读取项目默认基准…');
  const generation=useRef(0);
  useEffect(()=>{let live=true;
    void Promise.allSettled([rpc({cmd:'sources.list'}),project.direct?Promise.resolve({branches:[]}):rpc({cmd:'worktree.list',project_id:project.id})]).then(results=>{
      if(!live)return;
      if(results[0].status==='fulfilled')setSources((results[0].value.sources||[]).filter((s:TaskSource)=>!s.project_id||s.project_id===project.id||['linear','jira'].includes(s.provider)));
      if(results[1].status==='fulfilled'){
        setBranches(results[1].value.branches||[]);
        const value=results[1].value.default_base;
        setDefaultBase(value?.error?`项目默认基准错误：${value.error}`:value?.resolved?`项目默认基准${value.resolved.inferred?'（推断）':''}：${value.resolved.reference} · ${value.resolved.oid}`:'项目默认基准尚未解析；提交时由执行主机解析。');
      }else setDefaultBase('项目默认基准读取失败；可重开窗口检查，或填写明确引用。');
      const errors=results.flatMap((r,i)=>r.status==='rejected'?[`${i?'分支':'任务来源'}读取失败：${String(r.reason)}`]:[]);
      setCatalogError(errors.join('；'));
    });return()=>{live=false;generation.current++};
  },[project.id,project.direct]);
  useEffect(()=>{let live=true;setItems([]);setError('');setLoading(false);
    const available=sources.filter(s=>mode==='smart'||s.provider===mode);
    if(selected||mode==='name'||mode==='branch'||!available.length||(mode==='smart'&&query.trim().length<2))return;
    setLoading(true);
    const timer=setTimeout(()=>{void Promise.allSettled(available.map(s=>rpc({cmd:'sources.items',source_id:s.id,state:'open',search:query.trim(),cursor:'',scopes:['linear','jira'].includes(s.provider)&&s.scope?[s.scope]:[]}))).then(results=>{
      if(!live)return;
      setItems(results.flatMap(r=>r.status==='fulfilled'?r.value.items:[]).slice(0,12));
      setError(results.flatMap((r,i)=>r.status==='rejected'?[`${available[i].name}：${String(r.reason)}`]:[]).join('；'));
      setLoading(false);
    })},300);
    return()=>{live=false;clearTimeout(timer)};
  },[mode,query,sources,selected]);
  async function select(item:SourceItem){const serial=++generation.current;onLoading(true);setError('');
    try{const r=await rpc({cmd:'sources.detail',source_id:item.source_id,key:item.key});if(serial===generation.current)onSelect(r.item)}
    catch(e){if(serial===generation.current)setError(String(e))}
    finally{if(serial===generation.current)onLoading(false)}
  }
  const providerMode=mode!=='smart'&&mode!=='name'&&mode!=='branch';
  const matchingBranches=branches.filter(b=>b.toLowerCase().includes(query.toLowerCase())).slice(0,8);
  return <section className="creation-source" aria-label="创建来源">
    <div className="creation-label-row"><span>创建来源 <small>可选</small></span>{!project.direct&&<label className="creation-base"><GitBranch size={13}/><select aria-label="基准分支" value={base} disabled={disabled} onChange={e=>onBase(e.target.value)}><option value="">项目默认基准</option><option value="HEAD">HEAD · 当前提交（显式）</option>{base&&!["HEAD",...branches].includes(base)&&<option value={base}>{base}</option>}{branches.filter(b=>b!=='HEAD').map(b=><option key={b} value={b}>{b}</option>)}</select></label>}</div>
    {!project.direct&&<p className="creation-hint">{defaultBase}</p>}
    {selected?<div className="creation-selected"><Ticket size={16}/><div><strong>{selected.identifier} · {selected.title}</strong><small>{providers[selected.provider]} · 已关联任务</small></div><Button type="button" aria-label="取消任务关联" disabled={disabled} onClick={()=>{onSelect(undefined);setQuery('')}}><X size={14}/></Button></div>:<>
      <div className="creation-source-tabs" role="tablist" aria-label="来源类型">{modes.filter(m=>!project.direct||m.id!=='branch').map(({id,label,icon:Icon})=><Button key={id} id={`creation-tab-${id}`} type="button" role="tab" aria-selected={mode===id} aria-controls="creation-source-panel" tabIndex={mode===id?0:-1} disabled={disabled} onKeyDown={e=>{if(['ArrowLeft','ArrowRight','Home','End'].includes(e.key)){e.preventDefault();const tabs=Array.from(e.currentTarget.parentElement!.querySelectorAll<HTMLButtonElement>('[role=tab]'));const index=tabs.indexOf(e.currentTarget);const next=e.key==='Home'?0:e.key==='End'?tabs.length-1:(index+(e.key==='ArrowRight'?1:-1)+tabs.length)%tabs.length;tabs[next].focus();tabs[next].click()}}} onClick={()=>{setMode(id);setQuery('');setError('')}}><Icon size={13}/>{label}</Button>)}</div>
      <div id="creation-source-panel" role="tabpanel" aria-labelledby={`creation-tab-${mode}`}>
        <label className="creation-search"><Search size={15}/><Input autoFocus aria-label={mode==='name'?'名称':'搜索创建来源'} disabled={disabled} value={mode==='name'?name:query} maxLength={mode==='name'?50:200} placeholder={mode==='name'?'为工作树起个名字':mode==='branch'?'搜索本地或远程分支':providerMode?'搜索已连接来源的任务标题':'输入名称、分支或任务关键词'} onChange={e=>{setQuery(e.target.value);if(mode==='name'||mode==='smart')onName(e.target.value.slice(0,50))}}/></label>
        {(mode==='branch'||mode==='smart'&&query.trim())&&matchingBranches.length>0&&<div className="creation-results" aria-label="匹配分支">{matchingBranches.map(branch=><Button type="button" key={branch} disabled={disabled} onClick={()=>{onBase(branch);onName(branch.replace(/^(origin|upstream)\//,'').slice(0,50));setQuery('');setMode('name')}}><GitBranch size={14}/><span>{branch}</span>{base===branch&&<Check size={14}/>}</Button>)}</div>}
        {items.length>0&&<div className="creation-results" aria-label="匹配任务">{items.map(item=><Button type="button" key={item.key} disabled={disabled} onClick={()=>void select(item)}><Ticket size={14}/><span><strong>{item.identifier} · {item.title}</strong><small>{providers[item.provider]}</small></span></Button>)}</div>}
        {loading&&<p className="creation-hint" role="status">正在查找任务…</p>}
        {providerMode&&!loading&&!items.length&&<p className="creation-hint">{sources.some(s=>s.provider===mode)?'没有匹配的未完成任务，试试其他关键词。':`尚未连接 ${providers[mode as Provider]}。`}{!sources.some(s=>s.provider===mode)&&onSources&&<Button type="button" disabled={disabled} onClick={onSources}>连接任务来源</Button>}</p>}
        {mode==='branch'&&!matchingBranches.length&&<p className="creation-hint">没有匹配分支，可在高级设置中填写提交或引用。</p>}
        {mode==='smart'&&query.trim()&&<p className="creation-hint">未选择任务或分支时，将以“{name}”创建。</p>}
      </div>
    </>}
    {(error||catalogError)&&<p className="creation-error" role="alert">{error||catalogError}</p>}
  </section>;
}
