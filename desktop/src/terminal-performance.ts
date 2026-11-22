import type {Terminal} from '@xterm/xterm';

type Stage='native_input'|'composition_start'|'composition_end'|'input_queued'|'input_send'|'input_ack'|'output_read'|'output_parse'|'render_callback'|'paint_opportunity';
type Kind='key'|'enter'|'ime_key'|'paste'|'user'|'protocol'|'pointer';
type Fields={kind?:Kind;ms?:number;queueMs?:number;units?:number;bytes?:number;from?:number;end?:number;failed?:boolean;documentHidden?:boolean};
type Entry=Fields & {sequence:number;atMs:number;stage:Stage};
const kinds=new Set(['key','enter','ime_key','paste','user','protocol','pointer']);
const stages=new Set(['native_input','composition_start','composition_end','input_queued','input_send','input_ack','output_read','output_parse','render_callback','paint_opportunity']);

/** Bounded, memory-only metadata. Input/output/clipboard contents never enter this recorder. */
export function createTerminalPerformance(sessionId:string,now=()=>performance.timeOrigin+performance.now()){
  const entries:Entry[]=[];let sequence=0;
  return {
    record(stage:Stage,fields:Fields={}){
      if(!stages.has(stage))return;
      const entry:Entry={sequence:++sequence,atMs:Math.round(now()*100)/100,stage};
      if(fields.kind&&kinds.has(fields.kind))entry.kind=fields.kind;
      for(const key of ['ms','queueMs','units','bytes','from','end'] as const){
        const value=fields[key];if(typeof value==='number'&&Number.isFinite(value)&&value>=0)entry[key]=Math.round(value*100)/100;
      }
      for(const key of ['failed','documentHidden'] as const)if(typeof fields[key]==='boolean')entry[key]=fields[key];
      entries.push(entry);if(entries.length>512)entries.shift();
    },
    snapshot(){return {version:1,sessionId,exportedAt:new Date(now()).toISOString(),retained:entries.length,dropped:sequence-entries.length,privacy:'Timing and counts only; no command, output or clipboard content.',paintMeaning:'paint_opportunity is a browser frame opportunity, not proof of physical screen presentation.',entries:entries.map(entry=>({...entry}))}},
  };
}

export function observeTerminalPerformance(term:Terminal,sessionId:string){
  const trace=createTerminalPerformance(sessionId),element=term.element,textarea=term.textarea;
  const frames=new Set<number>();let pendingRender:number|undefined,framePending=false,disposed=false;
  const record=(stage:Stage,fields:Fields={})=>{if(!disposed)trace.record(stage,{...fields,documentHidden:document.visibilityState==='hidden'})};
  const native=(event:Event)=>{
    if(event.target!==textarea)return;
    if(event.type==='keydown'){
      const key=event as KeyboardEvent;
      record('native_input',{kind:key.isComposing||key.keyCode===229?'ime_key':key.key==='Enter'?'enter':'key'});
    }else if(event.type==='paste')record('native_input',{kind:'paste'});
    else record(event.type==='compositionstart'?'composition_start':'composition_end');
  };
  const events=['keydown','paste','compositionstart','compositionend'];
  for(const event of events)element?.addEventListener(event,native,true);
  const frame=(action:()=>void)=>{const id=requestAnimationFrame(()=>{frames.delete(id);if(!disposed)action()});frames.add(id)};
  const render=term.onRender(()=>{
    if(pendingRender===undefined)return;
    const received=pendingRender;pendingRender=undefined;
    record('render_callback',{ms:performance.now()-received});
    if(!framePending){framePending=true;frame(()=>frame(()=>{framePending=false;record('paint_opportunity',{ms:performance.now()-received})}))}
  });
  return {
    record,snapshot:trace.snapshot,
    transport(command:string,elapsedMs:number,response:any,units=0){
      if(command==='terminal.input')record('input_ack',{ms:elapsedMs,units,failed:!response||response.ok===false});
      if(command==='terminal.output'){
        const bytes=Math.max(0,(response?.end??0)-(response?.from??response?.end??0));
        if(bytes||elapsedMs>=100)record('output_read',{ms:elapsedMs,bytes,from:response?.from,end:response?.end,failed:!response||response.ok===false});
        if(bytes&&pendingRender===undefined)pendingRender=performance.now();
      }
    },
    dispose(){disposed=true;render.dispose();for(const event of events)element?.removeEventListener(event,native,true);for(const id of frames)cancelAnimationFrame(id);frames.clear()},
  };
}
