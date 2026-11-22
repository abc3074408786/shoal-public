import React,{useId,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {ArrowLeft,ArrowUpRight,Check,ChevronRight,Monitor,RefreshCw,ShieldCheck,Star,Terminal} from 'lucide-react';
import AgentLogo from '../../src/AgentLogo';
import AgentDefaultPicker from '../../src/AgentDefaultPicker';
import {agentCatalog,permissionModesFor,type AgentId,type AgentPreferences} from '../../src/agent-preferences';
import '../../src/theme.css';
import '../../src/orca-light.css';
import './preview.css';

type Design='table'|'cards'|'inspector';
const variants:{id:Design;label:string;name:string;description:string}[]=[
  {id:'table',label:'01',name:'紧凑工具列表',description:'名称、命令与开关对齐，减少重复信息。'},
  {id:'cards',label:'02',name:'双列工具卡片',description:'以 Logo 识别工具，独立管理默认和启用。'},
  {id:'inspector',label:'03',name:'工具与详情',description:'左侧选工具，右侧查看启动方式和默认设置。'},
];
function Toggle({agent,checked,onChange}:{agent:AgentId;checked:boolean;onChange:(checked:boolean)=>void}){
  const name=agentCatalog.find(item=>item.id===agent)!.name;
  return <label className="agent-toggle"><input type="checkbox" role="switch" aria-label={`启用 ${name}`} checked={checked} onChange={event=>onChange(event.target.checked)}/><span aria-hidden="true"/></label>;
}
function AgentContent({design}:{design:Design}){
  const [preferences,setPreferences]=useState<AgentPreferences>({defaultAgent:'codex',disabled:[]});
  const [selected,setSelected]=useState<AgentId>('codex');
  const uid=useId();
  const count=agentCatalog.length-preferences.disabled.length;
  function enable(id:AgentId,checked:boolean){setPreferences(old=>({...old,defaultAgent:!checked&&old.defaultAgent===id?'auto':old.defaultAgent,disabled:checked?old.disabled.filter(value=>value!==id):[...old.disabled,id]}))}
  function setDefault(id:AgentPreferences['defaultAgent']){setPreferences(old=>({...old,defaultAgent:id,disabled:old.disabled.filter(value=>value!==id)}))}
  const current=agentCatalog.find(agent=>agent.id===selected)!;
  return <section className={`agent-content design--${design}`} aria-label={`${variants.find(item=>item.id===design)!.name}样本`}>
    <header className="agent-page-heading"><h1>Agent</h1><p>选择常用工具，设置新终端的默认启动方式。</p></header>
    <div className="default-choice"><div><strong>默认启动</strong><small>新建终端时优先选择</small></div><AgentDefaultPicker id={uid} value={preferences.defaultAgent} disabled={preferences.disabled} onChange={setDefault}/></div>
    <div className="tools-heading"><h2>工具清单 <span>{count} / 7</span></h2><button type="button" className="detect-button" disabled title="设计样本不执行真实检测"><RefreshCw size={12}/><span>重新检测</span></button></div>
    {design==='table'?<div className="tool-table"><div className="table-heading"><span>工具</span><span>启动命令</span><span>启用</span></div>{agentCatalog.map(agent=>{const enabled=!preferences.disabled.includes(agent.id),isDefault=preferences.defaultAgent===agent.id;return <div key={agent.id} className={`table-row${enabled?'':' is-disabled'}`}>
      <div className="table-identity"><AgentLogo agent={agent.id} size={24}/><div><strong>{agent.name}</strong>{isDefault&&<small>默认</small>}</div></div><code>{agent.program}</code><Toggle agent={agent.id} checked={enabled} onChange={checked=>enable(agent.id,checked)}/>
    </div>})}<p className="table-note">仅显示已启用工具的新建入口</p></div>:design==='cards'?<div className="tool-cards">{agentCatalog.map(agent=>{const enabled=!preferences.disabled.includes(agent.id),isDefault=preferences.defaultAgent===agent.id;return <article key={agent.id} className={`tool-card${isDefault?' is-default':''}${enabled?'':' is-disabled'}`}>
      <div className="card-top"><AgentLogo agent={agent.id} size={28}/><Toggle agent={agent.id} checked={enabled} onChange={checked=>enable(agent.id,checked)}/></div><h3>{agent.name}</h3><code>{agent.program}</code><div className="card-bottom"><span>未检测</span><button type="button" onClick={()=>setDefault(agent.id)} aria-label={`将 ${agent.name} 设为默认`} aria-pressed={isDefault}>{isDefault?<Check size={12}/>:<Star size={12}/>}<span>{isDefault?'默认工具':'设为默认'}</span></button></div>
    </article>})}</div>:<div className="tool-inspector"><div className="inspector-list" aria-label="工具选择">{agentCatalog.map(agent=><button type="button" key={agent.id} aria-label={`查看 ${agent.name}`} aria-pressed={selected===agent.id} onClick={()=>setSelected(agent.id)}><AgentLogo agent={agent.id} size={20}/><span>{agent.name}</span>{preferences.defaultAgent===agent.id?<span className="tiny-default" title="默认工具"><Check size={12}/></span>:<ChevronRight size={12}/>}</button>)}</div><article className="inspector-detail"><div className="detail-header"><span><AgentLogo agent={current.id} size={40}/></span><h2>{current.name}</h2><p>命令行工具</p></div><dl><div><dt>启动命令</dt><dd><code>{current.program}</code></dd></div><div><dt>检测状态</dt><dd>未检测</dd></div></dl><div className="detail-toggle"><span>显示在新建菜单</span><Toggle agent={current.id} checked={!preferences.disabled.includes(current.id)} onChange={checked=>enable(current.id,checked)}/></div><button type="button" className="detail-default" onClick={()=>setDefault(current.id)}>{preferences.defaultAgent===current.id?<Check size={14}/>:<Star size={14}/>}<span>{preferences.defaultAgent===current.id?'当前默认工具':'设为默认工具'}</span></button><p className="detail-note">默认选择仅影响新终端，已有会话保留。</p></article></div>}
    <p className="agent-page-note">关闭入口不会结束已有会话。</p>
  </section>;
}
function Sidebar({page,onSelect,onBack}:{page:'agents'|'permissions';onSelect:(page:'agents'|'permissions')=>void;onBack:()=>void}){return <nav className="approved-sidebar" aria-label="设置分类"><button className="side-back" type="button" onClick={onBack}><ArrowLeft size={16}/>返回样本</button><h2>设置</h2><span className="side-caption">偏好设置</span><button type="button" className="side-item" aria-current={page==='agents'?'page':undefined} onClick={()=>onSelect('agents')}><Terminal size={18}/><span>Agent</span><i/></button><button type="button" className="side-item" aria-current={page==='permissions'?'page':undefined} onClick={()=>onSelect('permissions')}><ShieldCheck size={18}/><span>权限</span><i/></button><div className="side-host"><Monitor size={17}/><div><strong>本机</strong><small>偏好独立保存</small></div></div></nav>}
function Permissions(){return <section className="agent-content permission-existing"><header className="agent-page-heading"><h1>默认权限</h1><p>本轮比较 Agent 页面，权限保留现有分层。</p></header><div className="permission-explanation"><ShieldCheck size={18}/><div><strong>全局默认 → Agent 覆盖 → 单次启动选择</strong><p>当前仅展示权限目录，未修改真实执行策略。</p></div></div>{agentCatalog.map(agent=><div className="permission-row" key={agent.id}><AgentLogo agent={agent.id} size={22}/><div><strong>{agent.name}</strong><p>{permissionModesFor(agent.id).map(option=>option.label).join(' / ')}</p></div></div>)}</section>}
function App(){
  const [view,setView]=useState<Design|'all'>('all'),[page,setPage]=useState<'agents'|'permissions'>('agents');
  const selected=variants.find(variant=>variant.id===view);
  return <main className="agent-design-preview"><header className="preview-toolbar"><strong>Velella <span>/</span> Agent 页面</strong><small>左侧导航 A 已选定 · 内容设计样本</small></header><section className="preview-intro"><div><h1>Agent 页面，重新排一版。</h1><p>相同的七个工具、相同的 Logo。比较内容怎么摆、工具怎么选。</p></div><span className="preview-only">不保存真实设置</span></section><nav className="design-tabs" aria-label="页面方案"><button aria-pressed={view==='all'} onClick={()=>setView('all')}>三版对比</button>{variants.map(variant=><button key={variant.id} aria-pressed={view===variant.id} onClick={()=>{setPage('agents');setView(variant.id)}}>{variant.label} {variant.name}</button>)}</nav>
    {view==='all'?<div className="agent-comparison">{variants.map(variant=><article className="comparison-sample" key={variant.id}><header className="sample-title"><div><span>{variant.label}</span><h2>{variant.name}</h2></div><button type="button" onClick={()=>{setPage('agents');setView(variant.id)}} aria-label={`展开${variant.name}`} title="查看完整设置页"><ArrowUpRight size={17}/></button></header><p className="sample-description">{variant.description}</p><div className="sample-page"><AgentContent design={variant.id}/></div><button className="open-sample" type="button" onClick={()=>{setPage('agents');setView(variant.id)}}>查看完整设置页<ArrowUpRight size={13}/></button></article>)}</div>:<><div className="context-caption"><span>{selected?.label} · {selected?.name}</span><small>左侧固定为你选的 A 导航</small></div><div className="full-settings-page"><Sidebar page={page} onSelect={setPage} onBack={()=>setView('all')}/><div className="settings-main"><div className="settings-scroll">{page==='agents'?<AgentContent key={view} design={view}/>:<Permissions/>}</div><footer><span>交互样本 · 不保存真实偏好</span><button type="button" onClick={()=>setView('all')}>返回三版对比</button></footer></div></div></>}
    <p className="preview-instructions">三版均可选择默认 Agent 和切换启用；03 还可点击左侧工具，查看对应详情。</p>
  </main>;
}
const mount=document.getElementById('root')! as HTMLElement&{designRoot?:ReturnType<typeof createRoot>};
(mount.designRoot??=createRoot(mount)).render(<App/>);
