import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {ArrowLeft,ArrowUpRight,ChevronRight,Monitor,ShieldCheck,Terminal,Layers3,Check,RotateCcw} from 'lucide-react';
import AgentLogo from '../../src/AgentLogo';
import AgentDefaultPicker from '../../src/AgentDefaultPicker';
import {agentCatalog,globalPermissionModes,permissionModesFor,resolvedAgentPermission,withPermissionDefault,type AgentPreferences,type GlobalPermissionMode,type AgentPermissionMode} from '../../src/agent-preferences';
import '../../src/theme.css';
import '../../src/orca-light.css';
import './preview.css';

type Design='list'|'descriptive'|'rail';
type Page='agents'|'permissions';
const designs:{id:Design;label:string;name:string;note:string}[]=[
  {id:'list',label:'A',name:'轻量列表',note:'安静的白色选中态，图标与文字清晰对齐。'},
  {id:'descriptive',label:'B',name:'说明导航',note:'名称下补充用途，Agent 和权限更容易区分。'},
  {id:'rail',label:'C',name:'窄图标栏',note:'图标与短标签上下排列，为内容腾出空间。'},
];
function Navigation({design,page,onSelect,onBack}:{design:Design;page:Page;onSelect:(page:Page)=>void;onBack:()=>void}){
  return <nav className={`sample-navigation navigation--${design}`} aria-label={`${designs.find(d=>d.id===design)!.label} 设置分类`}>
    <button className="nav-back" onClick={onBack} aria-label="返回样本对比" title="返回样本对比"><ArrowLeft size={16}/><span>返回工作界面</span></button>
    <header className="nav-heading"><h2>设置</h2></header>
    {design!=='rail'&&<span className="nav-caption">偏好设置</span>}
    <div className="nav-items">{[{id:'agents' as const,name:'Agent',description:'默认工具与启用清单',icon:Terminal},{id:'permissions' as const,name:'权限',description:'审批模式与权限覆盖',icon:ShieldCheck}].map(item=>{const Icon=item.icon;return <button key={item.id} type="button" aria-label={item.name} aria-current={page===item.id?'page':undefined} onClick={()=>onSelect(item.id)} className="nav-item">
      <span className="nav-icon"><Icon size={18} strokeWidth={1.65}/></span><span className="nav-copy"><strong>{item.name}</strong>{design==='descriptive'&&<small>{item.description}</small>}</span>
      {design==='list'&&<span className="nav-current-dot" aria-hidden="true"/>}{design==='descriptive'&&<ChevronRight className="nav-current-arrow" size={14} aria-hidden="true"/>}
    </button>})}</div>
    <div className="nav-host" title="本机 · 偏好独立保存"><span><Monitor size={17} strokeWidth={1.6}/></span><div><strong>本机</strong><small>偏好独立保存</small></div></div>
  </nav>;
}
function SettingsContent({page}:{page:Page}){
  const [preferences,setPreferences]=useState<AgentPreferences>({defaultAgent:'codex',disabled:[]});
  return <div className="sample-settings-content"><div className="sample-content-scroll">
    <header className="content-heading"><h1>{page==='agents'?'Agent':'默认权限'}</h1><p>{page==='agents'?'管理常用工具和新建终端的默认选择。':'为每个 Agent 选择新开终端时使用的权限。'}</p></header>
    {page==='agents'?<>
      <section className="default-setting"><span className="default-mark"><Layers3 size={22}/></span><div><strong>默认 Agent</strong><small>新建菜单中的首选项</small></div><AgentDefaultPicker id="preview-default-agent" value={preferences.defaultAgent} disabled={preferences.disabled} onChange={value=>setPreferences(old=>({...old,defaultAgent:value}))}/></section>
      <div className="content-list-heading"><h2>Agent 清单</h2><small>{7-preferences.disabled.length} / 7 已启用</small></div>
      <div>{agentCatalog.map(agent=><div className="agent-row" key={agent.id}><span className="agent-mark"><AgentLogo agent={agent.id} size={22}/></span><div><strong>{agent.name}</strong><code>{agent.program}</code></div>{preferences.defaultAgent===agent.id&&<span className="default-badge"><Check size={11}/>默认</span>}<label className="enable-switch"><input type="checkbox" role="switch" aria-label={`启用 ${agent.name}`} checked={!preferences.disabled.includes(agent.id)} onChange={event=>{const enabled=event.target.checked;setPreferences(old=>({...old,defaultAgent:!enabled&&old.defaultAgent===agent.id?'auto':old.defaultAgent,disabled:enabled?old.disabled.filter(id=>id!==agent.id):[...old.disabled,agent.id]}))}}/><span/></label></div>)}</div>
      <p className="content-footnote">关闭仅隐藏新建入口，已有会话仍保留。</p>
    </>:<>
      <p className="permission-scope">默认权限仅影响新终端，已有会话保留原权限。</p>
      <section className="default-setting"><span className="default-mark"><ShieldCheck size={22}/></span><div><strong>全局默认权限</strong><small>用于没有单独覆盖的 Agent</small></div><select aria-label="全局默认权限" value={preferences.permissionDefault||'inherit'} onChange={event=>setPreferences(old=>withPermissionDefault(old,event.target.value as GlobalPermissionMode))}>{globalPermissionModes.map(option=><option value={option.id} key={option.id}>{option.label}</option>)}</select></section>
      <div className="content-list-heading"><h2>Agent 权限覆盖</h2><small>单独设置优先于全局</small></div>
      {agentCatalog.map(agent=>{const mode=resolvedAgentPermission(preferences,agent.id).mode;return <div className="permission-row" key={agent.id}><span className="agent-mark"><AgentLogo agent={agent.id} size={21}/></span><div className="permission-copy"><strong>{agent.name}</strong><p>{permissionModesFor(agent.id).find(option=>option.id===mode)?.description||'请选择此 Agent 的权限模式。'}</p></div><select aria-label={`${agent.name} 权限`} value={preferences.permissions?.[agent.id]||'default'} onChange={event=>setPreferences(old=>{const permissions={...old.permissions};if(event.target.value==='default')delete permissions[agent.id];else permissions[agent.id]=event.target.value as AgentPermissionMode;return {...old,permissions}})}><option value="default">跟随全局默认</option>{permissionModesFor(agent.id).map(option=><option key={option.id} value={option.id}>{option.label}</option>)}</select></div>})}
    </>}
  </div><footer className="content-footer"><small>交互样本 · 不保存真实设置</small><span>仅用于比较导航样式</span></footer></div>;
}
function Sample({design,initialPage,onFull}:{design:typeof designs[number];initialPage:Page;onFull:()=>void}){
  const [page,setPage]=useState(initialPage);
  return <article className="sample"><header className="sample-heading"><div><span>{design.label}</span><h2>{design.name}</h2>{design.id==='list'&&<em>推荐</em>}</div><button onClick={onFull} aria-label={`展开${design.name}`} title="查看完整界面"><ArrowUpRight size={17}/></button></header><p className="sample-note">{design.note}</p><div className={`sample-stage stage--${design.id}`}><Navigation design={design.id} page={page} onSelect={setPage} onBack={onFull}/>{design.id==='rail'&&<div className="rail-context"><span>内容区域</span><h3>{page==='agents'?'Agent':'默认权限'}</h3><p>{page==='agents'?'默认工具与启用清单':'全局默认与单独覆盖'}</p><div>{(page==='agents'?agentCatalog.slice(0,4):agentCatalog.slice(0,3)).map(agent=><div className="rail-context-row" key={agent.id}><AgentLogo agent={agent.id} size={17}/><span>{agent.name}</span></div>)}</div></div>}</div><button className="sample-open" onClick={onFull}>查看完整界面<ArrowUpRight size={13}/></button></article>;
}
function App(){
  const [view,setView]=useState<Design|'all'>('all'),[page,setPage]=useState<Page>('agents'),[reset,setReset]=useState(0);
  const active=designs.find(design=>design.id===view);
  return <main className="navigation-design-preview"><header className="preview-toolbar"><strong>Velella <span>/</span> 设置导航</strong><small>设计样本 · 不修改真实偏好</small></header>
    <section className="preview-heading"><div><h1>Agent / 权限，三种导航样式</h1><p>沿用现在的字体、灰白配色和内容，比较左侧导航的结构与选中效果。</p></div><button className="reset-samples" onClick={()=>setReset(old=>old+1)}><RotateCcw size={14}/>重置样本</button></section>
    <div className="preview-controls"><nav aria-label="样本视图"><button aria-pressed={view==='all'} onClick={()=>setView('all')}>三版对比</button>{designs.map(design=><button key={design.id} aria-pressed={view===design.id} onClick={()=>setView(design.id)}>{design.label} {design.name}</button>)}</nav><div className="state-switch" aria-label="比较选中状态"><button aria-pressed={page==='agents'} onClick={()=>{setPage('agents');setReset(old=>old+1)}}>Agent</button><button aria-pressed={page==='permissions'} onClick={()=>{setPage('permissions');setReset(old=>old+1)}}>权限</button></div></div>
    {view==='all'?<div className="sample-comparison">{designs.map(design=><Sample key={design.id+page+reset} design={design} initialPage={page} onFull={()=>setView(design.id)}/>)}</div>:<>
      <p className="context-caption">{active?.label} · {active?.name}<span>{active?.note}</span></p>
      <div className={`full-context context--${view}`} key={view+reset}><Navigation design={view} page={page} onSelect={setPage} onBack={()=>setView('all')}/><SettingsContent page={page}/></div>
    </>}
    <p className="preview-instructions">每版都可点击 Agent / 权限；点「查看完整界面」检查与右侧内容的搭配。</p>
  </main>;
}
const mount=document.getElementById('root')! as HTMLElement&{designRoot?:ReturnType<typeof createRoot>};
(mount.designRoot??=createRoot(mount)).render(<App/>);
