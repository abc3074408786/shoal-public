import type {Terminal} from '@xterm/xterm';
import {createTerminalTuiMouseWheelDistanceState,resolveTerminalTuiMouseWheelReportCount} from './terminal-wheel-distance';

/** Normalize distance at the DOM boundary; xterm remains responsible for mouse encoding. */
export function attachTerminalMouseWheelMultiplier(term:Terminal,options:{getTuiMouseWheelMultiplier?:()=>number}={}) {
  const state=createTerminalTuiMouseWheelDistanceState();
  const element=term.element;
  if(!element)return {dispose(){}};
  const forwarded=new WeakSet<Event>();
  const wheel=(event:WheelEvent)=>{
    if(forwarded.has(event)||event.shiftKey||event.ctrlKey||event.metaKey||term.modes.mouseTrackingMode==='none'||!event.deltaY)return;
    const screen=element.querySelector('.xterm-screen');
    const height=screen?.getBoundingClientRect().height;
    const count=resolveTerminalTuiMouseWheelReportCount(event,options.getTuiMouseWheelMultiplier?.()??1,state,{cellHeight:height?height/term.rows:undefined,rows:term.rows});
    event.preventDefault();event.stopImmediatePropagation();
    for(let index=0;index<count;index++){
      const next=new WheelEvent('wheel',{bubbles:true,cancelable:true,deltaMode:1,deltaY:Math.sign(event.deltaY),clientX:event.clientX,clientY:event.clientY,altKey:event.altKey});
      forwarded.add(next);
      (event.target??element).dispatchEvent(next);
    }
  };
  element.addEventListener('wheel',wheel,{capture:true,passive:false});
  return {dispose(){element.removeEventListener('wheel',wheel,true)}};
}
