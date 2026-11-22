export type Geometry = {offset:number;rows:number;cols:number;reset?:boolean;sequence?:number};
export type ReplayCursor = {sequence?:number;rawGeometry?:boolean;tinyGeometryCount?:number;clearDisplayAt?:number;displayCleared?:boolean;beforeResize?:(offset:number)=>void;reset?:(offset:number)=>Promise<void>};
type ReplayTerminal={resize:(cols:number,rows:number)=>void;reset:()=>void;options:{scrollback?:number}};

// Trim only rendered scrollback through xterm's public options API. Unlike
// injecting ED3, this preserves a UTF-8/ANSI sequence split across output pages.
// Restoring capacity keeps all subsequent output scrollable. Raw PTY bytes and
// the current screen/parser/cursor are unchanged.
export function clearTerminalDisplayHistory(terminal:Pick<ReplayTerminal,'options'>){
  const capacity=terminal.options.scrollback??1000;
  terminal.options.scrollback=0;
  terminal.options.scrollback=capacity;
}
export async function writeTerminalPage(
  bytes:Uint8Array, from:number, replayEnd:number, geometry:Geometry[],
  terminal:ReplayTerminal,
  write:(bytes:Uint8Array,history:boolean)=>Promise<void>,
  state:ReplayCursor={},
){
  const end=from+bytes.length;
  let cursor=from;
  const writeRange=async(to:number)=>{
    const cut=Math.max(cursor,Math.min(to,replayEnd));
    if(cut>cursor)await write(bytes.subarray(cursor-from,cut-from),true);
    if(to>cut)await write(bytes.subarray(cut-from,to-from),false);
    cursor=to;
  };
  const segment=async(to:number)=>{
    const boundary=state.clearDisplayAt;
    if(!state.displayCleared&&boundary!==undefined&&boundary>=cursor&&boundary<=to){
      await writeRange(boundary);
      clearTerminalDisplayHistory(terminal);state.displayCleared=true;
    }
    await writeRange(to);
  };
  // Follow the host's serialized output/geometry order, including resizes
  // from other viewers. Sequence prevents repeated boundary resets on polls.
  for(const event of geometry){
    if(event.offset>end)break;
    if(event.sequence!==undefined&&state.sequence!==undefined&&event.sequence<=state.sequence)continue;
    if(!Number.isInteger(event.offset)||event.offset<0||![event.rows,event.cols].every(n=>Number.isInteger(n)&&n>=1&&n<=1000))throw new Error('终端尺寸记录无效');
    const at=Math.max(from,event.offset);
    if(at<cursor)throw new Error('终端尺寸记录顺序无效');
    await segment(at);
    // Older viewers recorded FitAddon's detached 2x1 grid. Replaying it can
    // reflow a whole screen into hundreds of blank or one-character rows.
    // Keep the preceding grid only for that historical artifact. Every byte
    // is still written; live geometry and process resets remain authoritative.
    const collapsedHistory=!event.reset&&event.offset<replayEnd&&event.rows===1&&event.cols<=2;
    if(collapsedHistory){
      state.tinyGeometryCount=(state.tinyGeometryCount??0)+1;
      if(!state.rawGeometry){state.sequence=event.sequence;continue;}
    }
    state.beforeResize?.(at);
    if(event.reset&&event.offset>=from){if(state.reset)await state.reset(at);else terminal.reset();}
    terminal.resize(event.cols,event.rows);
    state.sequence=event.sequence;
  }
  await segment(end);
}
