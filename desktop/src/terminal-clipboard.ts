import type {Terminal} from '@xterm/xterm';
import {invoke} from './desktop-platform';
import {isTauri} from './api';
import {pasteClipboard,type ClipboardPayload} from './image-attachment';
import {currentTarget} from './execution-target';
import {stripTerminalSelectionGutter} from './terminal-selection';
import type {TerminalPreferences} from './terminal-preferences';
import {terminalEditingInput,terminalKeyboardFlags,type TerminalKeyboardEnvironment} from './terminal-keyboard';

export function sanitizeTerminalPaste(value:string){return value.replace(/\x1b/g,'␛')}

export function installTerminalClipboard(term:Terminal,options:{
  environment?:TerminalKeyboardEnvironment;
  current:()=>boolean;preferences:()=>TerminalPreferences;
  schedule:(action:()=>Promise<void>)=>Promise<void>;
  pasteText:(value:string)=>Promise<void>;send:(value:string)=>Promise<void>;report:(error:unknown)=>void;
}){
  const element=term.element!;
  let disposed=false,suppressNativePaste=false,interrupted=false;
  const claimed=new Set<string>();
  const current=()=>!disposed&&options.current();
  const write=async(text:string)=>{
    if(isTauri())await invoke('terminal_copy',{text});
    else await navigator.clipboard.writeText(text);
  };
  const copy=async()=>{
    const selection=term.getSelection();
    if(!selection)return false;
    await write(options.preferences().trimCopyGutter?stripTerminalSelectionGutter(selection):selection);
    return true;
  };
  const paste=(payload?:ClipboardPayload,owner:Element|null=null)=>{
    if(!current())return Promise.resolve();
    // A pasted image reaches the CLI as a file path on the machine that wrote
    // it, so a remote session must keep text-only paste rather than receive a
    // local path it cannot open.
    const localHost=currentTarget().kind==='local';
    // Read each deliberate gesture immediately, then deliver in gesture order.
    // Resolve errors into values so a later queued read cannot reject unhandled.
    const reading=Promise.resolve().then(()=>payload?payload:isTauri()&&localHost?invoke<ClipboardPayload>('terminal_clipboard',{target:currentTarget()}):navigator.clipboard.readText().then(value=>({kind:'text' as const,value})))
      .then(value=>({value}),error=>({error}));
    // Reserve the gesture in the same queue as keyboard input before awaiting
    // the clipboard. Later typing/Enter must never overtake this paste.
    return options.schedule(async()=>{
      const result=await reading;
      if('error' in result){options.report(result.error);return}
      await pasteClipboard(
        ()=>Promise.resolve(result.value),
        ()=>current()&&(!owner||document.activeElement===owner),
        async value=>{
          const text=sanitizeTerminalPaste(value);
          // After Ctrl+C a foreground process may leave bracket mode stale.
          // Keep multiline frames; a single line needs no framing in that state.
          const ignore=term.options.ignoreBracketedPasteMode;
          if(interrupted&&!/[\r\n]/.test(text))term.options.ignoreBracketedPasteMode=true;
          try{await options.pasteText(text)}finally{term.options.ignoreBracketedPasteMode=ignore}
        },options.send);
    }).catch(options.report);
  };
  // A new explicit bracketed-mode sequence, including one split across writes,
  // establishes the foreground application's current paste state.
  const mode=term.parser.registerCsiHandler({prefix:'?',final:'h'},params=>{if(params.includes(2004))interrupted=false;return false});
  const modeOff=term.parser.registerCsiHandler({prefix:'?',final:'l'},params=>{if(params.includes(2004))interrupted=false;return false});
  const key=(event:KeyboardEvent)=>{
    if(event.isComposing||event.keyCode===229)return;
    const key=event.key.toLowerCase(),code=event.code||event.key;
    if(event.type==='keyup'){
      if(claimed.delete(code)){event.preventDefault();event.stopImmediatePropagation()}
      return;
    }
    // A fresh editing gesture owns later native paste events. Keep a chord's
    // suppression until its event arrives, even when the OS clipboard is slow.
    if(!event.repeat&&!['control','shift','alt','meta'].includes(key))suppressNativePaste=false;
    const ctrl=event.ctrlKey&&!event.altKey&&!event.metaKey;
    const copyKey=ctrl&&key==='c'&&(event.shiftKey||term.hasSelection());
    const pasteKey=(ctrl&&key==='v')||(event.shiftKey&&key==='insert'&&!event.ctrlKey&&!event.altKey&&!event.metaKey);
    const editing=terminalEditingInput(event,terminalKeyboardFlags(term),options.environment??{platform:typeof navigator==='undefined'?'':navigator.platform,local:currentTarget().kind==='local'});
    if(copyKey||pasteKey||editing!==undefined){
      event.preventDefault();event.stopImmediatePropagation();claimed.add(code);
      if(pasteKey)suppressNativePaste=true;
      if(event.repeat&&(copyKey||pasteKey))return;
      if(copyKey)void copy().catch(options.report);
      if(pasteKey)void paste(undefined,document.activeElement);
      if(editing!==undefined&&current())term.input(editing,true);
    }else if(ctrl&&!event.shiftKey&&key==='c')interrupted=term.modes.bracketedPasteMode;
  };
  const onPaste=(event:ClipboardEvent)=>{
    event.preventDefault();event.stopImmediatePropagation();
    if(suppressNativePaste){suppressNativePaste=false;return}
    // Tauri reads native image formats; browser paste events already own text.
    const text=event.clipboardData?.getData('text/plain');
    void paste(!isTauri()&&text!==undefined?{kind:'text',value:text}:undefined,document.activeElement);
  };
  const context=(event:MouseEvent)=>{
    suppressNativePaste=false;
    if(event.ctrlKey||!options.preferences().rightClickPaste)return;
    event.preventDefault();event.stopImmediatePropagation();
    if(term.hasSelection())void copy().then(copied=>{if(copied&&!disposed)term.clearSelection()}).catch(options.report);
    else {if(current())term.focus();void paste()}
  };
  const resetGesture=()=>{suppressNativePaste=false};
  element.addEventListener('keydown',key,true);element.addEventListener('keyup',key,true);
  element.addEventListener('paste',onPaste,true);element.addEventListener('contextmenu',context,true);
  document.addEventListener('pointerdown',resetGesture,true);term.textarea?.addEventListener('blur',resetGesture);
  return {paste:async()=>{resetGesture();await paste()},copy,dispose(){disposed=true;claimed.clear();mode.dispose();modeOff.dispose();document.removeEventListener('pointerdown',resetGesture,true);term.textarea?.removeEventListener('blur',resetGesture);element.removeEventListener('keydown',key,true);element.removeEventListener('keyup',key,true);element.removeEventListener('paste',onPaste,true);element.removeEventListener('contextmenu',context,true)}};
}
