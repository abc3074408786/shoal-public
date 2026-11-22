import {useEffect,useRef,useState,type CSSProperties} from 'react';
import {Terminal} from '@xterm/xterm';
import {Button,Modal} from './ui';
import type {TerminalFrames as Frames} from './terminal-frames';
import type {TerminalPreferences} from './terminal-preferences';
import {terminalAppearance} from './terminal-preferences';
import {configureTerminalUnicode} from './terminal-unicode';
import {useAppearance} from './Appearance';
import {surfaceThemes} from './surface-themes';
import './terminal-frames.css';

export default function TerminalFrames({open,onOpenChange,value,preferences}:{open:boolean;onOpenChange:(open:boolean)=>void;value:Frames;preferences:TerminalPreferences}){
  const [selected,setSelected]=useState(0),[fitWidth,setFitWidth]=useState(true),scroller=useRef<HTMLDivElement>(null),[canvas,setCanvas]=useState<HTMLDivElement|null>(null),{theme}=useAppearance();
  useEffect(()=>{if(open)setSelected(Math.max(0,value.frames.length-1))},[open,value]);
  useEffect(()=>{scroller.current?.scrollTo({top:0,left:0})},[selected]);
  const frame=value.frames[selected],surface=surfaceThemes[theme];
  useEffect(()=>{
    if(!open||!frame||!canvas)return;
    const term=new Terminal({...terminalAppearance(preferences),cols:frame.cols,rows:Math.max(1,frame.contentRows),scrollback:0,scrollbar:{width:0},disableStdin:true,cursorBlink:false,theme:surface,allowProposedApi:true});
    term.open(canvas);configureTerminalUnicode(term);
    let naturalWidth=0,disposed=false;
    const resize=()=>{
      if(disposed||!scroller.current)return;
      if(!naturalWidth)naturalWidth=canvas.querySelector<HTMLElement>('.xterm-screen')?.clientWidth||0;
      if(!naturalWidth)return;
      const style=getComputedStyle(scroller.current),available=scroller.current.clientWidth-parseFloat(style.paddingInlineStart)-parseFloat(style.paddingInlineEnd);
      term.options.fontSize=fitWidth?Math.max(6,Math.min(preferences.fontSize,Math.floor(preferences.fontSize*(available-4)/naturalWidth*4)/4)):preferences.fontSize;
    };
    term.write(frame.ansi,resize);const observer=new ResizeObserver(resize);if(scroller.current)observer.observe(scroller.current);
    window.dispatchEvent(new CustomEvent('velella:terminal-frame-created',{detail:{terminal:term}}));
    return()=>{disposed=true;observer.disconnect();term.dispose()};
  },[open,frame,preferences,surface,fitWidth,canvas]);
  return <Modal open={open} onOpenChange={onOpenChange} title="历史画面"><div className="terminal-frames">
    <p>查看清屏、程序退出或窗口调整前保留的画面。默认适应宽度，也可用原始字号左右滚动查看。</p>
    {frame?<><div className="terminal-frames-controls"><Button disabled={selected===0} onClick={()=>setSelected(n=>n-1)}>上一幅</Button><label><span className="sr-only">选择历史画面</span><select aria-label="选择历史画面" value={selected} onChange={e=>setSelected(Number(e.target.value))}>{value.frames.map((item,index)=><option key={`${item.offset}:${index}`} value={index}>画面 {index+1} / {value.frames.length} · {item.cols} 列 × {item.rows} 行</option>)}</select></label><Button disabled={selected===value.frames.length-1} onClick={()=>setSelected(n=>n+1)}>下一幅</Button><Button aria-pressed={!fitWidth} onClick={()=>setFitWidth(fit=>!fit)}>{fitWidth?'原始字号':'适应宽度'}</Button></div>
      <div ref={scroller} className="terminal-frame-scroll" tabIndex={0} aria-label="历史画面内容，可横向和纵向滚动" style={{'--frame-bg':surface.background,'--frame-fg':surface.foreground} as CSSProperties}><div ref={setCanvas} className="terminal-frame-canvas"/></div>
      {value.dropped>0&&<small>当前保留最近 {value.frames.length} 幅画面；更早的原始终端记录仍然保留。</small>}
    </>:<p>尚无可查看的历史画面。</p>}
  </div></Modal>;
}
