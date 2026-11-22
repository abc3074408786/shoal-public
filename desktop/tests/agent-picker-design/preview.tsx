import React,{useEffect,useId,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {Check,ChevronDown,Layers3,Terminal,SlidersHorizontal,Shield,Monitor,X,RotateCcw,ArrowUpRight} from 'lucide-react';
import AgentLogo from '../../src/AgentLogo';
import {agentCatalog,type AgentId} from '../../src/agent-preferences';
import '../../src/theme.css';
import '../../src/orca-light.css';
import './preview.css';

type Choice='auto'|'terminal'|AgentId;
type Variant='compact'|'grid'|'detail';
const options=[{id:'auto',name:'自动选择',command:'按 Agent 清单顺序选择可用工具'},{id:'terminal',name:'空白终端',command:'只打开终端，不启动 Agent'},...agentCatalog.map(a=>({...a,command:a.program}))];
const versions:{id:Variant;number:string;name:string;note:string}[]=[
  {id:'compact',number:'01',name:'紧凑列表',note:'轻巧、直接，延续桌面菜单的使用习惯。'},
  {id:'grid',number:'02',name:'双列面板',note:'用 Logo 快速定位，减少纵向滚动。'},
  {id:'detail',number:'03',name:'说明列表',note:'名称与启动方式一起看，选择更清楚。'},
];
function Logo({id,size=20}:{id:Choice;size?:number}){return id==='auto'?<Layers3 size={size}/>:id==='terminal'?<Terminal size={size}/>:<AgentLogo agent={id} size={size}/>}

function Picker({variant,reset}:{variant:Variant;reset:number}){
  const [selected,setSelected]=useState<Choice>('codex');
  const [open,setOpen]=useState(true);
  const [keyboard,setKeyboard]=useState(false);
  const [animate,setAnimate]=useState(false);
  const root=useRef<HTMLDivElement>(null),trigger=useRef<HTMLButtonElement>(null);
  const uid=useId();
  const chosen=options.find(a=>a.id===selected)!;
  useEffect(()=>{setAnimate(false);setOpen(true);setSelected('codex');},[reset]);
  useEffect(()=>{const outside=(event:PointerEvent)=>{if(!root.current?.contains(event.target as Node))setOpen(false)};document.addEventListener('pointerdown',outside);return()=>document.removeEventListener('pointerdown',outside)},[]);
  function focus(index:number){const items=root.current?.querySelectorAll<HTMLButtonElement>('[role=option]');items?.[Math.max(0,Math.min(options.length-1,index))]?.focus()}
  function pick(id:Choice){setSelected(id);setOpen(false);trigger.current?.focus()}
  return <div ref={root} className={`picker picker--${variant}`} data-keyboard={keyboard} data-animate={animate} onPointerDown={()=>setKeyboard(false)}>
    <button ref={trigger} className="picker-trigger" aria-label={`${versions.find(v=>v.id===variant)!.name}：默认 Agent`} aria-haspopup="listbox" aria-expanded={open} aria-controls={uid} onClick={()=>{setAnimate(!open);setOpen(!open)}} onKeyDown={event=>{if(event.key==='ArrowDown'||event.key==='ArrowUp'){event.preventDefault();setKeyboard(true);setOpen(true);requestAnimationFrame(()=>focus(event.key==='ArrowDown'?0:options.length-1))}}}>
      <Logo id={selected} size={18}/><span>{chosen.name}</span><ChevronDown size={15}/>
    </button>
    {open&&<div className="picker-popover" id={uid} role="listbox" aria-label="默认 Agent" onKeyDown={event=>{const current=Number((event.target as HTMLElement).closest('[data-index]')?.getAttribute('data-index')??0);const delta=variant==='grid'&&current>=2?2:1;if(['ArrowDown','ArrowUp','ArrowLeft','ArrowRight','Home','End','Escape','Tab'].includes(event.key)){setKeyboard(true);if(event.key==='Escape'){event.preventDefault();setOpen(false);trigger.current?.focus();return}if(event.key==='Tab'){setOpen(false);return}event.preventDefault();focus(event.key==='Home'?0:event.key==='End'?options.length-1:current+(event.key==='ArrowDown'?delta:event.key==='ArrowUp'?-delta:event.key==='ArrowRight'?1:-1))}}}>
      {variant==='detail'?<div className="selection-summary"><span className="summary-logo"><Logo id={selected} size={24}/></span><div><small>当前默认</small><strong>{chosen.name}</strong></div><span className="summary-check"><Check size={14}/></span></div>:<div className="menu-caption">{variant==='grid'?'选择启动工具':'默认启动方式'}<span>仅影响新终端</span></div>}
      <div className="mode-options">{options.slice(0,2).map((a,index)=>renderOption(a,index))}</div>
      <div className="agents-caption">Agent<span>7</span></div>
      <div className="agent-options">{options.slice(2).map((a,index)=>renderOption(a,index+2))}</div>
    </div>}
  </div>;
  function renderOption(option:typeof options[number],index:number){const active=selected===option.id;return <button type="button" role="option" aria-selected={active} tabIndex={-1} key={option.id} data-index={index} className={`picker-option${active?' is-selected':''}`} onClick={()=>pick(option.id as Choice)}>
    <span className="option-logo"><Logo id={option.id as Choice} size={20}/></span>
    <span className="option-copy"><strong>{option.name}</strong>{variant!=='compact'&&<small>{option.command}</small>}</span>
    <span className="option-check">{active&&<Check size={15}/>}</span>
  </button>}
}
function AgentRows(){return <div className="context-list"><div className="list-heading"><strong>Agent 清单</strong><span>7 / 7 已启用</span></div>{agentCatalog.map(agent=><div className="context-row" key={agent.id}><span className="context-logo"><AgentLogo agent={agent.id} size={20}/></span><div><strong>{agent.name}</strong><code>{agent.program}</code></div><span className="fake-switch"/></div>)}</div>}
function Setting({variant,reset,full=false}:{variant:Variant;reset:number;full?:boolean}){return <div className={`setting-surface${full?' is-full':''}`}>
  <div className="setting-heading"><h2>Agent</h2><p>管理常用工具和新建终端的默认选择。</p></div>
  <div className="default-row"><div className="default-copy"><span className="default-logo"><Layers3 size={21}/></span><div><strong>默认 Agent</strong><small>新建菜单中的首选项</small></div></div><Picker variant={variant} reset={reset}/></div>
  <AgentRows/>
</div>}
function App(){const initial=document.body.dataset.variant as Variant|undefined;const [view,setView]=useState<Variant|'all'>(initial||'all');const [reset,setReset]=useState(0);return <main>
  <header className="preview-header"><div className="preview-brand"><span><SlidersHorizontal size={14}/></span>Velella <i>/</i> 组件设计</div><span className="preview-only">交互预览 · 不保存设置</span></header>
  <section className="preview-intro"><div><span className="eyebrow">DEFAULT AGENT</span><h1>让选择，更顺手。</h1><p>同一套字体与 Logo，三种不同的菜单结构。</p></div><button className="reset" onClick={()=>setReset(x=>x+1)}><RotateCcw size={14}/>重新展开</button></section>
  <nav className="view-tabs" aria-label="设计方案"><button aria-pressed={view==='all'} onClick={()=>setView('all')}>三版对比</button>{versions.map(v=><button key={v.id} aria-pressed={view===v.id} onClick={()=>setView(v.id)}>{v.number} {v.name}</button>)}</nav>
  {view==='all'?<div className="comparison">{versions.map(v=><article className="variant" key={v.id}><div className="variant-heading"><span>{v.number}</span><div><h2>{v.name}{v.id==='compact'&&<em>推荐</em>}</h2><p>{v.note}</p></div><a href={`./${v.id}.html`} aria-label={`单独查看${v.name}`}><ArrowUpRight size={18}/></a></div><Setting variant={v.id} reset={reset}/><div className="variant-footer">{v.id==='compact'?'单行选项 · 分组分隔 · 轻阴影':v.id==='grid'?'双列排列 · 图标识别 · 紧凑高度':'当前选择摘要 · 两行信息 · 更从容'}</div></article>)}</div>:<div className="full-preview"><aside><div className="nav-title"><SlidersHorizontal size={20}/>设置</div><small>偏好设置</small><div className="nav-active"><Terminal size={17}/>Agent</div><div className="nav-inactive"><Shield size={17}/>权限</div><div className="nav-host"><Monitor size={17}/><span>本机<small>偏好独立保存</small></span></div></aside><div className="full-main"><span className="mock-close"><X size={18}/></span><Setting key={view} variant={view} reset={reset} full/><footer><span>仅影响新开的终端</span><span>设计预览</span></footer></div></div>}
  <p className="interaction-note">点击任意选项可预览选中状态；点击控件再次展开。支持方向键、Enter 和 Esc。</p>
</main>}
const mount=document.getElementById('root')! as HTMLElement&{designRoot?:ReturnType<typeof createRoot>};
const designRoot=mount.designRoot??=createRoot(mount);
designRoot.render(<App/>);
