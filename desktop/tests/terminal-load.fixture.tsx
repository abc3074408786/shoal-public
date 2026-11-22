import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import TerminalView from '../src/TerminalView';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
let release:()=>void=()=>{};
const gate=new Promise<void>(resolve=>{release=resolve});
const nativeFetch=window.fetch.bind(window);
const tracePath=new URLSearchParams(location.search).get('trace');
const history=new URLSearchParams(location.search).has('history')?Array.from({length:50},(_,i)=>`HISTORY_${i}`).join('\r\n')+'\r\n':'';
const fixtureBytes=new TextEncoder().encode('\x1b[2J\x1b[H'+history+'RESTORED_TERMINAL\r\n> ');
const trace=tracePath?await nativeFetch(tracePath).then(r=>r.json()):{ok:true,from:0,end:fixtureBytes.length,total:fixtureBytes.length,data:btoa(String.fromCharCode(...fixtureBytes)),geometry:[{offset:0,cols:80,rows:24,reset:true,sequence:0}]};
let inputs=0,reads=0;
window.addEventListener('velella:terminal-created',event=>{
 const term=(event as CustomEvent).detail.terminal;
 const sync=()=>{const element=term.element?.parentElement;if(element){element.dataset.bufferBase=String(term.buffer.active.baseY);element.dataset.bufferViewport=String(term.buffer.active.viewportY)}};
 term.onScroll(sync);term.onWriteParsed(sync);
});
window.fetch=async(input,init)=>{
 if(!String(input).endsWith('/_velella'))return nativeFetch(input,init);
 const request=JSON.parse(String(init?.body));
 if(request.cmd.endsWith('.output')){
  reads++;await gate;
  if(request.since>=trace.total)return Response.json({...trace,from:trace.total,end:trace.total,data:''});
  return Response.json(trace);
 }
 if(request.cmd.endsWith('.input'))inputs++;
 return Response.json({ok:true});
};
function App(){const [released,setReleased]=useState(false);return <>
 <div style={{height:40,display:'flex',alignItems:'center',paddingInline:16,gap:12}}><button onClick={()=>{release();setReleased(true)}} disabled={released}>完成记录加载</button><span>隔离回放，不连接真实终端</span><button onClick={()=>alert(JSON.stringify({inputs,reads}))}>检查输入计数</button></div>
 <div style={{height:'calc(100dvh - 40px)',display:'flex'}}><TerminalView workspace={{projectId:'replay-fixture',workspaceId:'replay-fixture',id:'replay-fixture',program:'powershell',status:{exit:null,input_protocol:1,pid:1}}}/></div>
 </>}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><App/></TooltipProvider></AppearanceProvider>);
