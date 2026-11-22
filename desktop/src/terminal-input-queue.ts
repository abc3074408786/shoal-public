// Ordered user input has a different failure policy from output reads: an
// ambiguous write stops the queue. Neither that write nor queued keys replay.
export function createTerminalInputQueue(report:(error:unknown)=>void){
  let chain=Promise.resolve(),generation=0,paused=false,disposed=false,pending=0;
  return {
    send(action:()=>Promise<void>,size:number){
      if(disposed||paused)return Promise.reject(new Error('输入已暂停，请核对终端后恢复输入。'));
      if(pending+size>1024*1024){paused=true;generation++;const error=new Error('待发送输入过多，输入已暂停；未发送内容不会自动补发。');report(error);return Promise.reject(error)}
      pending+=size;const version=generation;
      const next=chain.then(async()=>{if(disposed||version!==generation)throw new Error('未发送的输入已取消');await action()});
      chain=next.catch(error=>{if(!disposed&&version===generation){paused=true;generation++;report(error)}}).finally(()=>{pending-=size});
      return next;
    },
    drain:()=>chain,
    resume(){if(!disposed){generation++;paused=false}},
    dispose(){disposed=true;generation++;paused=true},
  };
}
