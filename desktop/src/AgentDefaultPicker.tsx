import {useCallback,useState,type KeyboardEvent} from 'react';
import * as Menu from '@radix-ui/react-dropdown-menu';
import {Check,ChevronDown,Layers3,Terminal} from 'lucide-react';
import AgentLogo from './AgentLogo';
import {agentCatalog,type AgentId,type AgentPreferences} from './agent-preferences';
import './agent-default-picker.css';

type Choice=AgentPreferences['defaultAgent'];
function Logo({id}:{id:Choice}){
  return id==='auto'?<Layers3 size={20} aria-hidden="true"/>:id==='terminal'?<Terminal size={20} aria-hidden="true"/>:<AgentLogo agent={id} size={20}/>;
}

export default function AgentDefaultPicker({id,value,disabled,onChange}:{id:string;value:Choice;disabled:readonly AgentId[];onChange:(value:Choice)=>void}){
  const [container,setContainer]=useState<HTMLDialogElement|null>(null);
  const [keyboard,setKeyboard]=useState(false);
  // A native modal makes portals on document.body inert. Its own top layer owns this menu.
  const bindTrigger=useCallback((button:HTMLButtonElement|null)=>setContainer(button?.closest<HTMLDialogElement>('dialog')??null),[]);
  const enabled=agentCatalog.filter(agent=>!disabled.includes(agent.id));
  const modes=[{id:'auto' as const,name:'自动选择',description:'按 Agent 清单顺序选择可用工具'},{id:'terminal' as const,name:'空白终端',description:'只打开终端，不启动 Agent'}];
  const selected=[...modes,...enabled].find(option=>option.id===value);

  function navigate(event:KeyboardEvent<HTMLDivElement>){
    if(!['ArrowDown','ArrowUp','ArrowLeft','ArrowRight'].includes(event.key))return;
    const items=Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitemradio"]'));
    const current=items.indexOf(document.activeElement as HTMLElement);
    if(current<0)return;
    let next=current;
    if(event.key==='ArrowDown')next=current<2?current+1:current+2;
    if(event.key==='ArrowUp')next=current<2?Math.max(0,current-1):current<4?1:current-2;
    if(event.key==='ArrowLeft'&&current>=2&&(current-2)%2===1)next=current-1;
    if(event.key==='ArrowRight'&&current>=2&&(current-2)%2===0)next=current+1;
    event.preventDefault();event.stopPropagation();
    items[next<items.length?next:current]?.focus();
  }

  return <Menu.Root modal={false}>
    <Menu.Trigger asChild><button type="button" id={id} ref={bindTrigger} className="agent-picker-trigger" aria-label="默认 Agent" onPointerDown={()=>setKeyboard(false)} onKeyDown={()=>setKeyboard(true)}>
      <Logo id={value}/><span>{selected?.name||'自动选择'}</span><ChevronDown size={14} aria-hidden="true"/>
    </button></Menu.Trigger>
    <Menu.Portal container={container??undefined}>
      <Menu.Content className="agent-picker-panel" data-keyboard={keyboard} align="end" sideOffset={7} collisionPadding={12} aria-label="默认 Agent" onKeyDownCapture={navigate}>
        <div className="agent-picker-heading">选择启动工具<span>仅影响新终端</span></div>
        <Menu.RadioGroup value={value} onValueChange={choice=>{if(choice==='auto'||choice==='terminal'||enabled.some(agent=>agent.id===choice))onChange(choice as Choice)}}>
          <div className="agent-picker-modes">{modes.map(mode=><Menu.RadioItem key={mode.id} value={mode.id} textValue={mode.name} className="agent-picker-option">
            <span className="agent-picker-logo"><Logo id={mode.id}/></span><span className="agent-picker-copy"><strong>{mode.name}</strong><small>{mode.description}</small></span><Menu.ItemIndicator className="agent-picker-check"><Check size={14}/></Menu.ItemIndicator>
          </Menu.RadioItem>)}</div>
          <Menu.Separator className="agent-picker-divider"/>
          <div className="agent-picker-caption">Agent<span>{enabled.length}</span></div>
          {enabled.length?<div className="agent-picker-grid">{enabled.map(agent=><Menu.RadioItem key={agent.id} value={agent.id} textValue={agent.name} className="agent-picker-option">
            <span className="agent-picker-logo"><Logo id={agent.id}/></span><span className="agent-picker-copy"><strong>{agent.name}</strong><small>{agent.program}</small></span><Menu.ItemIndicator className="agent-picker-check"><Check size={12}/></Menu.ItemIndicator>
          </Menu.RadioItem>)}</div>:<p className="agent-picker-empty">启用 Agent 后可在这里选择。</p>}
        </Menu.RadioGroup>
      </Menu.Content>
    </Menu.Portal>
  </Menu.Root>;
}
