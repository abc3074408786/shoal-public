import type {Terminal} from '@xterm/xterm';

export type TerminalInputKind='user'|'protocol'|'pointer';
export function isProtocolReply(text:string){
  if(text.length>512)return false;
  return /^\x1b\[\?(?:[0-9]|[12][0-9]|3[01])u$/.test(text)||/^(?:\x1b\[(?:[IO]|0n|[?>][\d;]+c|\??[\d;]+R|\??[\d;]+\$y|[468];[\d;]+t)|\x1b\](?:1[012];|4;\d+;)rgb:[\da-fA-F]{1,4}\/[\da-fA-F]{1,4}\/[\da-fA-F]{1,4}(?:\x07|\x1b\\))$/.test(text);
}

// xterm 6 exposes provenance only on its core service. Keep this seam bounded
// and contract-tested; do not fall back to treating every onData as a keystroke.
export function subscribeTerminalInput(term:Terminal,send:(text:string,kind:TerminalInputKind)=>void){
  const core=(term as unknown as {_core?:{coreService?:{onUserInput?:(listener:()=>void)=>{dispose:()=>void}}}})._core?.coreService;
  let disposed=false,pendingUser=false,replaying=false;
  const pendingWrites=new Set<()=>void>();
  let user:{dispose:()=>void}|undefined;
  try{user=core?.onUserInput?.(()=>{pendingUser=true})}catch{/* Unsupported internals stay read-only. */}
  const supported=typeof user?.dispose==='function';
  if(!supported)term.options.disableStdin=true;
  const data=term.onData(text=>{
    const fromUser=pendingUser;pendingUser=false;
    if(disposed||!supported)return;
    if(fromUser){
      if(replaying&&(/^\x1b\[(?:<|M)/.test(text)||(term.buffer.active.type==='alternate'&&/^\x1b(?:\[|O)[AB]$/.test(text))))return;
      send(text,/^\x1b\[<\d+;\d+;\d+[Mm]$/.test(text)?'pointer':'user');
    }else if(!replaying&&text!=='\x1b[I'&&text!=='\x1b[O'&&isProtocolReply(text))send(text,'protocol');
  });
  return {supported,write:(bytes:Uint8Array|string,history:boolean)=>new Promise<void>(resolve=>{
    if(disposed){resolve();return}replaying=history;
    const done=()=>{pendingWrites.delete(done);replaying=false;resolve()};pendingWrites.add(done);term.write(bytes,done);
  }),dispose(){disposed=true;pendingUser=false;data.dispose();user?.dispose();for(const done of pendingWrites)done()}};
}
