// One page in flight, including xterm consumption. Yield between pages so a
// backlog cannot starve input; idle viewers do not busy-poll the runtime.
export function createTerminalOutputPump(
  page:()=>Promise<{backlog:boolean;changed:boolean}>,
  report:(error:unknown)=>void,
){
  let disposed=false,running=false,woken=false,visible=true,idle=8,responsiveUntil=0;
  let timer:ReturnType<typeof setTimeout>|undefined;
  const schedule=(delay:number)=>{clearTimeout(timer);if(!disposed)timer=setTimeout(tick,delay)};
  const tick=async()=>{
    if(disposed||running)return;
    running=true;woken=false;let delay=100;
    try{
      const result=await page();
      // Hidden, quiet sessions need no interactive polling cadence. Activity
      // resets the delay; reveal/input wakes immediately, including in flight.
      idle=result.changed?8:Math.min(idle*2,visible?100:2000);
      // Output is interactive activity too: streamed CLI responses have empty
      // reads between chunks. Renew the window so those gaps do not repeatedly
      // push the next chunk behind a 64/100 ms idle timer.
      if(result.changed)responsiveUntil=performance.now()+150;
      // A key can reach the PTY before its echo is ready. Keep a short interactive
      // window instead of backing off to 32/64/100 ms after those empty reads.
      // A hidden, caught-up stream needs batching, not interactive polling.
      // Backlogs still drain immediately; reveal/input bypass this wait via wake.
      delay=result.backlog?0:!visible?Math.max(idle,250):performance.now()<responsiveUntil?Math.min(idle,8):idle;
    }catch(error){report(error);idle=Math.min(Math.max(idle*2,100),2000);delay=idle}
    finally{running=false;if(!disposed)schedule(woken?0:delay)}
  };
  const wake=()=>{if(disposed)return;idle=8;responsiveUntil=performance.now()+150;if(running)woken=true;else schedule(0)};
  wake();
  return {wake,setVisible(value:boolean){visible=value;if(value)wake()},dispose(){disposed=true;clearTimeout(timer)}};
}
