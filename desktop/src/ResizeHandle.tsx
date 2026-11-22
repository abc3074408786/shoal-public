import {useEffect,useRef} from 'react';
import {clampPanelWidth} from './ui-preferences';
export default function ResizeHandle({side,width,min,max,onChange}:{side:'left'|'right';width:number;min:number;max:number;onChange:(width:number)=>void}){
  const drag=useRef<{x:number;width:number}|null>(null);
  useEffect(()=>()=>{drag.current=null},[]);
  return <div className={`resize-handle ${side}`} role="separator" aria-label={side==='left'?'调整项目栏宽度':'调整工具栏宽度'} aria-orientation="vertical" aria-valuenow={Math.round(width)} aria-valuemin={min} aria-valuemax={max} tabIndex={0}
    onPointerDown={e=>{e.currentTarget.setPointerCapture(e.pointerId);drag.current={x:e.clientX,width};e.preventDefault()}}
    onPointerMove={e=>{if(drag.current)onChange(clampPanelWidth(drag.current.width+(e.clientX-drag.current.x)*(side==='left'?1:-1),min,max))}}
    onPointerUp={e=>{drag.current=null;e.currentTarget.releasePointerCapture(e.pointerId)}} onPointerCancel={()=>{drag.current=null}}
    onKeyDown={e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();onChange(clampPanelWidth(width+(e.key==='ArrowRight'?10:-10)*(side==='left'?1:-1),min,max))}}}/>;
}
