export type TerminalSize={rows:number;cols:number};

// Keep the viewport grid on the host's recorded timeline. Measuring available
// space must not resize xterm before older output has been consumed.
export function createTerminalResize(
  measure:()=>TerminalSize|undefined,
  applyLegacy:(size:TerminalSize)=>void,
  recorded:()=>boolean,
  send:(size:TerminalSize)=>Promise<unknown>,
  report:(error:unknown)=>void,
){
  let disposed=false,running=false,pending:TerminalSize|undefined,lastSent='';
  const drain=async()=>{
    running=true;
    try{
      while(!disposed&&pending){
        const size=pending;pending=undefined;
        const key=`${size.rows}:${size.cols}`;
        if(key===lastSent)continue;
        try{await send(size);lastSent=key}catch(error){if(!disposed)report(error)}
      }
    }finally{running=false}
  };
  return {
    // A shared host can be resized by another viewer after our last request.
    // Use its consumed geometry as the deduplication baseline; observing alone
    // must never compete with the foreground viewer for the PTY size.
    observe(size:TerminalSize){if(!disposed)lastSent=`${size.rows}:${size.cols}`},
    request(){
      if(disposed)return;
      const size=measure();
      if(!size||![size.rows,size.cols].every(n=>Number.isInteger(n)&&n>=1&&n<=1000))return;
      // FitAddon clamps a detached/collapsed viewport to 2x1. That is not a
      // user's requested geometry and must never corrupt the live ConPTY TUI.
      if(size.cols<=2||size.rows<=1)return;
      if(!recorded())applyLegacy(size);
      pending=size;if(!running)void drain();
    },
    dispose(){disposed=true;pending=undefined},
  };
}
