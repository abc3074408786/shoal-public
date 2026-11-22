import {Modal} from './ui';
import {useEffect,useState} from 'react';
import type {TerminalPreferences} from './terminal-preferences';
import type {RendererStatus} from './terminal-renderer';

function NumberPreference({value,onChange,min,max,step=1}:{value:number;onChange:(value:number)=>void;min:number;max:number;step?:number}){
  const [draft,setDraft]=useState(String(value));
  useEffect(()=>setDraft(String(value)),[value]);
  const commit=()=>{const next=Number(draft);if(draft&&Number.isFinite(next)){const bounded=Math.min(max,Math.max(min,next));setDraft(String(bounded));onChange(bounded)}else setDraft(String(value))};
  return <input type="number" min={min} max={max} step={step} value={draft} onChange={event=>setDraft(event.target.value)} onBlur={commit} onKeyDown={event=>{if(event.key==='Enter')event.currentTarget.blur()}}/>;
}

export default function TerminalSettings({open,onOpenChange,value,onChange,renderer}:{open:boolean;onOpenChange:(open:boolean)=>void;value:TerminalPreferences;onChange:(value:TerminalPreferences)=>void;renderer:RendererStatus}){
  return <Modal open={open} onOpenChange={onOpenChange} title="终端设置"><p>设置自动保存，并立即应用到所有终端。运行中的会话会保留。</p><TerminalSettingsFields value={value} onChange={onChange} renderer={renderer}/></Modal>;
}
export function TerminalSettingsFields({value,onChange,renderer}:{value:TerminalPreferences;onChange:(value:TerminalPreferences)=>void;renderer?:RendererStatus}){
  const [font,setFont]=useState(value.fontFamily);
  useEffect(()=>setFont(value.fontFamily),[value.fontFamily]);
  const set=<K extends keyof TerminalPreferences>(key:K,next:TerminalPreferences[K])=>onChange({...value,[key]:next});
  return <div className="terminal-settings settings-terminal-fields">
    <label>字体<input value={font} onChange={event=>setFont(event.target.value)} onBlur={()=>set('fontFamily',font)} onKeyDown={event=>{if(event.key==='Enter')set('fontFamily',font)}} list="terminal-fonts"/></label>
    <datalist id="terminal-fonts">{['Cascadia Mono','Consolas','Cascadia Code','JetBrains Mono','MesloLGS Nerd Font'].map(font=><option key={font} value={font}/>)}</datalist>
    <label>字号<NumberPreference min={8} max={40} value={value.fontSize} onChange={next=>set('fontSize',next)}/></label>
    <label>行高<NumberPreference min={1} max={2} step={.01} value={value.lineHeight} onChange={next=>set('lineHeight',next)}/></label>
    <label>光标形状<select value={value.cursorStyle} onChange={event=>set('cursorStyle',event.target.value as TerminalPreferences['cursorStyle'])}><option value="block">方块</option><option value="bar">竖线</option><option value="underline">下划线</option></select></label>
    <label>光标闪烁<input type="checkbox" checked={value.cursorBlink} onChange={event=>set('cursorBlink',event.target.checked)}/></label>
    <label>渲染方式<select value={value.renderer} onChange={event=>set('renderer',event.target.value as TerminalPreferences['renderer'])}><option value="auto">自动</option><option value="webgl">WebGL</option><option value="dom">DOM</option></select></label>
    {renderer&&<p className="terminal-renderer-status" role="status">当前：{renderer.kind==='webgl'?'WebGL':'DOM'} · {renderer.reason}</p>}
    <label>滚动速度<NumberPreference min={.1} max={10} step={.05} value={value.scrollSensitivity} onChange={next=>set('scrollSensitivity',next)}/></label>
    <label>快速滚动倍数<NumberPreference min={1} max={20} value={value.fastScrollSensitivity} onChange={next=>set('fastScrollSensitivity',next)}/></label>
    <label>右键复制选区或粘贴<input type="checkbox" checked={value.rightClickPaste} onChange={event=>set('rightClickPaste',event.target.checked)}/></label>
    <label>复制时移除公共左侧空白<input type="checkbox" checked={value.trimCopyGutter} onChange={event=>set('trimCopyGutter',event.target.checked)}/></label>
    <p>Ctrl+C 复制选区；没有选区时中断。Ctrl+Shift+C 复制，Ctrl+V、Ctrl+Shift+V 或 Shift+Insert 粘贴。Ctrl+右键打开菜单。</p>
  </div>;
}
