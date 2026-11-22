import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import type {Terminal} from '@xterm/xterm';
import TerminalView from '../src/TerminalView';
import {AppearanceProvider} from '../src/Appearance';
import {TooltipProvider} from '../src/ui';
import '../src/style.css';import '../src/theme.css';import '../src/orca-shell.css';

// Isolated transport: no model, daemon, production session or OS clipboard.
Object.defineProperty(navigator,'platform',{configurable:true,value:new URLSearchParams(location.search).get('platform')||'Win32'});
localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false}));
let term:Terminal|undefined,inputs:string[]=[],setInputs:(values:string[])=>void=()=>{};
let output=new TextEncoder().encode('\x1b[2J\x1b[H\x1b[?2004h输入兼容性隔离终端\r\n> ');
const append=(text:string)=>{const more=new TextEncoder().encode(text),next=new Uint8Array(output.length+more.length);next.set(output);next.set(more,output.length);output=next};
const nativeFetch=window.fetch.bind(window);
window.fetch=async(input,init)=>{
 if(!String(input).endsWith('/_velella'))return nativeFetch(input,init);
 const request=JSON.parse(String(init?.body));
 if(request.cmd.endsWith('.output')){
  const from=Math.min(request.since??0,output.length),bytes=output.slice(from);
  return Response.json({ok:true,from,end:output.length,total:output.length,data:btoa(String.fromCharCode(...bytes)),geometry:from===0?[{offset:0,cols:80,rows:24,reset:true,sequence:1}]:[]});
 }
 if(request.cmd.endsWith('.input')){inputs=[...inputs,request.text];setInputs(inputs)}
 return Response.json({ok:true});
};
window.addEventListener('velella:terminal-created',event=>{term=(event as CustomEvent).detail.terminal});
function compose(modifier:'shift'|'ctrl'|'plain'|'cancel'|'select-again'){
 const textarea=term!.textarea!;term!.focus();
 textarea.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));
 textarea.value='ni';textarea.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:'ni'}));
 textarea.dispatchEvent(new KeyboardEvent('keydown',{key:modifier==='cancel'?'Escape':'Enter',code:modifier==='cancel'?'Escape':'Enter',keyCode:modifier==='cancel'?27:13,shiftKey:modifier==='shift'||modifier==='select-again',ctrlKey:modifier==='ctrl',bubbles:true,cancelable:true}));
 if(modifier==='select-again')textarea.dispatchEvent(new KeyboardEvent('keydown',{key:' ',code:'Space',keyCode:32,bubbles:true,cancelable:true}));
 const value=modifier==='cancel'?'':'你';textarea.value=value;
 textarea.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:value}));
 textarea.dispatchEvent(new InputEvent('input',{bubbles:true,data:value,inputType:'insertFromComposition'}));
 textarea.dispatchEvent(new KeyboardEvent('keyup',{key:modifier==='cancel'?'Escape':'Enter',code:modifier==='cancel'?'Escape':'Enter',keyCode:modifier==='cancel'?27:13,shiftKey:modifier==='shift',ctrlKey:modifier==='ctrl',bubbles:true,cancelable:true}));
}
function App(){const [sent,update]=useState<string[]>([]);setInputs=update;return <>
 <div style={{padding:12,display:'flex',gap:8,flexWrap:'wrap'}}>
  <span>隔离终端，不连接真实会话</span>
  <button onClick={()=>{inputs=[];update([]);term?.focus()}}>清空计数并聚焦</button>
  <button onClick={()=>compose('shift')}>中文确认并换行</button>
  <button onClick={()=>compose('plain')}>仅确认中文</button>
  <button onClick={()=>compose('ctrl')}>中文确认并提交</button>
  <button onClick={()=>compose('cancel')}>取消中文输入</button>
  <button onClick={()=>compose('select-again')}>换行后继续选词</button>
  <button onClick={()=>append('\x1b[?u')}>请求键盘协议</button>
  <button onClick={()=>{term?.focus();const data=new DataTransfer();data.setData('text/plain','第一行\n第二行');term?.textarea?.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:data}))}}>粘贴多行样例</button>
 </div>
 <output aria-label="输入字节" style={{display:'block',padding:12,whiteSpace:'pre-wrap'}}>{JSON.stringify(sent)}</output>
 <div style={{height:'calc(100dvh - 135px)',display:'flex'}}><TerminalView workspace={{projectId:'input-parity-fixture',workspaceId:'input-parity-fixture',id:'input-parity-fixture',program:'powershell',status:{pid:1,exit:null,input_protocol:1}}}/></div>
 </>}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><App/></TooltipProvider></AppearanceProvider>);
