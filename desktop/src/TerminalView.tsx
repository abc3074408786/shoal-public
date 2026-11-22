import {useEffect,useRef,useState,type CSSProperties} from 'react';
import {createPortal} from 'react-dom';
import {Terminal} from '@xterm/xterm';
import {SearchAddon} from '@xterm/addon-search';
import {WebLinksAddon} from '@xterm/addon-web-links';
import {invoke} from './desktop-platform';
import {currentTarget} from './execution-target';
import '@xterm/xterm/css/xterm.css';
import './terminal-ime.css';
import './terminal-surface.css';
import * as ContextMenu from '@radix-ui/react-context-menu';
import {MoreHorizontal,Search,Settings2,X} from 'lucide-react';
import ActionMenu from './ActionMenu';
import {useAppearance} from './Appearance';
import {surfaceThemes} from './surface-themes';
import {Button,Input} from './ui';
import {rpc,isTauri,Task} from './api';
import {subscribeTerminalInput} from './terminal-input';
import {stabilizeTerminalComposition} from './terminal-ime';
import {writeTerminalPage,clearTerminalDisplayHistory, type ReplayCursor} from './terminal-replay';
import {createTerminalInputQueue} from './terminal-input-queue';
import {createTerminalOutputPump} from './terminal-output';
import {observeTerminalPerformance} from './terminal-performance';
import {createTerminalResize} from './terminal-resize';
import {measureTerminalViewport} from './terminal-fit';
import {manageTerminalScrollbar} from './terminal-scrollbar';
import {terminalKeyboardOptions} from './terminal-keyboard';
import {configureTerminalUnicode} from './terminal-unicode';
import {attachTerminalMouseWheelMultiplier} from './terminal-wheel';
import {installTerminalClipboard} from './terminal-clipboard';
import {manageTerminalFocus} from './terminal-focus';
import {manageTerminalRenderer,type RendererStatus} from './terminal-renderer';
import {readTerminalPreferences,saveTerminalPreferences,terminalAppearance,TERMINAL_PREFERENCES_EVENT,type TerminalPreferences} from './terminal-preferences';
import TerminalSettings from './TerminalSettings';
import LoadingState from './LoadingState';
import TerminalActivity from './TerminalActivity';
import TerminalFrames from './TerminalFrames';
import {captureTerminalClears,createTerminalFrameStore,resetTerminalWithHistory,type TerminalFrames as FrameCollection} from './terminal-frames';

export default function TerminalView({task,target='agent',workspace,onStop,onRecover,onReconnect,onOpenReconnect,openRequest,visible=true,renderVisible=visible,actionsTarget,historyOnly=false}:{historyOnly?:boolean;actionsTarget?:HTMLElement|null;visible?:boolean;renderVisible?:boolean;onStop?:()=>void;onRecover?:()=>void;onReconnect?:()=>Promise<void>;onOpenReconnect?:()=>Promise<void>;openRequest?:number;task?:Task;target?:string;workspace?:{projectId:string;workspaceId?:string;id:string;program?:string;agent?:boolean;status?:{pid?:number;state?:string;exit:number|null;human_control?:boolean;input_protocol?:number}|null}}) {
  const {theme}=useAppearance();
  const [searchOpen,setSearchOpen]=useState(false);
  const [loading,setLoading]=useState(true);
  const [settingsOpen,setSettingsOpen]=useState(false),[preferences,setPreferences]=useState(readTerminalPreferences);
  const preferencesRef=useRef(preferences);preferencesRef.current=preferences;
  const [rendererStatus,setRendererStatus]=useState<RendererStatus>({kind:'dom',reason:'正在初始化'});
  const applyAppearance=useRef<()=>void>(()=>{}),searchAddon=useRef<SearchAddon|null>(null);
  const copySelection=useRef<()=>Promise<boolean>>(async()=>false);
  const findInput=useRef<HTMLInputElement>(null),resizeTerminal=useRef<()=>void>(()=>{});
  const container=useRef<HTMLDivElement>(null);
  const terminal=useRef<Terminal|null>(null);
  const controlTerminal=useRef<(human:boolean)=>Promise<void>>(async()=>{});
  const pasteIntoSession=useRef<()=>Promise<void>>(async()=>{});
  const [inputPaused,setInputPaused]=useState(false),resumeInput=useRef<()=>void>(()=>{});
  const [inputSupported,setInputSupported]=useState(true),[controlBusy,setControlBusy]=useState(false);
  const [reconnecting,setReconnecting]=useState(false),[recoveryNotice,setRecoveryNotice]=useState('');
  const recoveryAttempt=useRef<string|null>(null);
  const opened=useRef<string|null>(null);
  const recoveryPending=useRef(false);
  const [geometryMissing,setGeometryMissing]=useState(false),[replaySize,setReplaySize]=useState<{cols:number;rows:number}|null>(null);
  const [rawGeometry,setRawGeometry]=useState(false),[tinyGeometryCount,setTinyGeometryCount]=useState(0);
  const [error,setError]=useState(''),[find,setFind]=useState('');
  const [displayTrimmed,setDisplayTrimmed]=useState(false),[displayRevision,setDisplayRevision]=useState(0);
  const [historyOpen,setHistoryOpen]=useState(false),[historyFrames,setHistoryFrames]=useState<FrameCollection>({frames:[],dropped:0});
  const captureHistory=useRef<()=>FrameCollection>(()=>({frames:[],dropped:0}));
  const clearDisplayHistory=useRef<()=>void>(()=>{});
  const performanceSnapshot=useRef<()=>unknown>(()=>null);
  const [exportBusy,setExportBusy]=useState(false),[exportResult,setExportResult]=useState('');
  const session=workspace||(target==='command'?task?.command:target==='shell'?task?.shell:task?.agent);
  const displayKey=`velella:terminal-display-boundary:v1:${JSON.stringify([workspace?.projectId,workspace?.workspaceId,task?.id,target,session?.id,workspace?.status?.pid])}`;
  const status=workspace?workspace.status:task?.status;
  const exited=status?.exit!==null&&status?.exit!==undefined;
  const hostLost=status?.state==='host_lost'||(!status&&!!onRecover);
  const runtimeSupported=status?.input_protocol===1;
  const readOnly=historyOnly||exited||!runtimeSupported;
  const live=useRef({readOnly,visible,renderVisible});live.current={readOnly,visible,renderVisible};
  const visibility=useRef<(value:boolean)=>void>(()=>{});
  const guarded=workspace?workspace.agent===true||['codex','claude','claude-code'].includes(workspace.program||''):!!task&&target==='agent';
  const inputLocked=readOnly||!inputSupported||inputPaused||loading;
  const openKey=JSON.stringify([workspace?.workspaceId,session?.id,openRequest]);
  const automaticallyRestoring=!historyOnly&&(reconnecting||(!!onReconnect||opened.current!==openKey&&!!onOpenReconnect)&&!recoveryNotice);
  useEffect(()=>{
    const key=JSON.stringify([workspace?.workspaceId,session?.id,workspace?.status?.pid]);
    if(historyOnly||!visible){recoveryAttempt.current=null;opened.current=null;return;}
    const opening=opened.current!==openKey;
    // Consume the opening even for a live host. A later stop observed by a
    // poll must stay stopped until another navigation/open request occurs.
    if(status&&(status.exit==null&&status.state!=='host_lost'||onOpenReconnect||onReconnect))opened.current=openKey;
    const reconnect=opening&&onOpenReconnect?onOpenReconnect:onReconnect;
    if(!reconnect||recoveryPending.current||recoveryAttempt.current===key&&!opening)return;
    recoveryPending.current=true;recoveryAttempt.current=key;setReconnecting(true);setRecoveryNotice('');
    void reconnect().then(()=>setRecoveryNotice(''))
      .catch(e=>setRecoveryNotice(`自动恢复未完成：${String(e)}`)).finally(()=>{recoveryPending.current=false;setReconnecting(false)});
  },[historyOnly,visible,openKey,workspace?.status?.pid,!!onReconnect,!!onOpenReconnect]);
  useEffect(()=>{const sync=()=>setPreferences(readTerminalPreferences());window.addEventListener(TERMINAL_PREFERENCES_EVENT,sync);window.addEventListener('storage',sync);return()=>{window.removeEventListener(TERMINAL_PREFERENCES_EVENT,sync);window.removeEventListener('storage',sync)}},[]);
  useEffect(()=>{
    if(!container.current || !session)return;
    let disposed=false,offset=0,replayEnd:number|undefined,caughtUp=false;
    let pump:ReturnType<typeof createTerminalOutputPump>|undefined;
    const wake=()=>pump?.wake();
    setLoading(true);setGeometryMissing(false);setInputPaused(false);setTinyGeometryCount(0);setError('');
    let clearDisplayRequested=false;
    let savedBoundary:number|undefined;
    try{const saved=localStorage.getItem(displayKey);if(saved!==null){const value=Number(saved);if(Number.isSafeInteger(value)&&value>=0)savedBoundary=value}}catch{/* Display recovery remains optional when storage is unavailable. */}
    setDisplayTrimmed(savedBoundary!==undefined);
    const replayCursor:ReplayCursor={rawGeometry,clearDisplayAt:savedBoundary};
    const ptyEnvironment={platform:navigator.platform,local:currentTarget().kind==='local'};
    const term=new Terminal({...terminalAppearance(preferencesRef.current,ptyEnvironment),...terminalKeyboardOptions(ptyEnvironment),disableStdin:true,scrollback:10000,minimumContrastRatio:theme==='light'?4.5:1,theme:surfaceThemes[theme],allowProposedApi:true,allowTransparency:false,drawBoldTextInBrightColors:true,scrollbar:{width:6}});
    const frameStore=createTerminalFrameStore();setHistoryOpen(false);setHistoryFrames({frames:[],dropped:0});
    const historyCapture=captureTerminalClears(term,()=>frameStore.capture(term,offset));
    replayCursor.beforeResize=at=>frameStore.capture(term,at);
    captureHistory.current=()=>{frameStore.capture(term,offset);return frameStore.snapshot()};
    terminal.current=term;
    term.open(container.current);
    const scrollbar=manageTerminalScrollbar(term);
    const timing=observeTerminalPerformance(term,session.id);performanceSnapshot.current=timing.snapshot;
    // Local diagnostics can subscribe before opening a fixture. No terminal
    // content or input is retained by the application for these lifecycle events.
    window.dispatchEvent(new CustomEvent('velella:terminal-created',{detail:{terminal:term,sessionId:session.id}}));
    configureTerminalUnicode(term);
    const search=new SearchAddon();term.loadAddon(search);searchAddon.current=search;
    const wheel=attachTerminalMouseWheelMultiplier(term);
    const composition=stabilizeTerminalComposition(term,ptyEnvironment);
    const focus=manageTerminalFocus(term);
    // ConPTY queries belong to the persistent host even without a viewer.
    // Old hosts only own DSR; newer Windows hosts also own primary DA.
    const dsr=term.parser.registerCsiHandler({final:'n'},params=>params[0]===6);
    const hostAnswersPrimary=(status as {terminal_query_protocol?:number}|null|undefined)?.terminal_query_protocol===1;
    const primaryDA=term.parser.registerCsiHandler({final:'c'},params=>hostAnswersPrimary&&params[0]===0);
    // Bind mutations to this viewer's host, including requests already queued
    // when recovery starts but before the new PID reaches the UI.
    const expectedHostPid=workspace?.status?.pid;
    const request=async(cmd:string,extra:Record<string,unknown>={})=>{
      const started=performance.now();let response:any;
      const hostGuard=expectedHostPid!==undefined&&['terminal.input','terminal.resize','terminal.control'].includes(cmd)?{expected_host_pid:expectedHostPid}:{};
      try{return response=await rpc(workspace?{cmd:cmd.replace('terminal.',workspace.workspaceId?'workbench.terminal.':'workspace.'),workspace_id:workspace.workspaceId,project_id:workspace.projectId,session_id:workspace.id,...extra,...hostGuard}:{cmd,task_id:task?.id,target,...extra,...hostGuard})}
      finally{const elapsedMs=performance.now()-started;timing.transport(cmd,elapsedMs,response,typeof extra.text==='string'?extra.text.length:0);window.dispatchEvent(new CustomEvent('velella:terminal-transport',{detail:{sessionId:session.id,command:cmd,elapsedMs,failed:!response,since:extra.since,from:response?.from,end:response?.end,total:response?.total}}))}
    };
    const report=(error:unknown)=>{if(!disposed){setLoading(false);setError(String(error))}};
    clearDisplayHistory.current=()=>{clearDisplayRequested=true;wake()};
    const inputQueue=createTerminalInputQueue(error=>{if(!disposed){setInputPaused(true);report(error)}});
    resumeInput.current=()=>{inputQueue.resume();setInputPaused(false);setError('');wake()};
    term.loadAddon(new WebLinksAddon((event,url)=>{
      if(!event.ctrlKey||disposed)return;
      if(isTauri())void invoke('terminal_open_link',{url}).catch(report);
      else window.open(url,'_blank','noopener,noreferrer');
    }));
    const negotiate=createTerminalResize(()=>measureTerminalViewport(term),size=>term.resize(size.cols,size.rows),()=>replayCursor.sequence!==undefined,size=>request('terminal.resize',size),report);
    const resize=()=>{if(disposed||!live.current.visible)return;
      if(!caughtUp){if(replayEnd===undefined){if(live.current.readOnly&&replaySize)term.resize(replaySize.cols,replaySize.rows);else{const size=measureTerminalViewport(term);if(size)term.resize(size.cols,size.rows)}}return}
      if(live.current.readOnly)return; // Preserve historical cursor geometry; scroll to view overflow.
      if(!document.hasFocus()||document.visibilityState!=='visible')return;
      negotiate.request();
    };
    const observer=new ResizeObserver(resize);observer.observe(container.current);resizeTerminal.current=resize;resize();
    let pasteCapture:Array<{text:string;input_kind:'user'|'protocol'|'pointer'}>|undefined;
    const sendInput=async(text:string,input_kind:'user'|'protocol'|'pointer'='user',queueMs?:number)=>{if(disposed)return;timing.record('input_send',{kind:input_kind,units:text.length,queueMs});await request('terminal.input',{text,enter:false,input_kind});wake()};
    const input=subscribeTerminalInput(term,(text,input_kind)=>{if(live.current.readOnly||!live.current.visible||(!caughtUp&&input_kind!=='protocol'))return;if(pasteCapture){pasteCapture.push({text,input_kind});return}const queued=performance.now();timing.record('input_queued',{kind:input_kind,units:text.length});wake();void inputQueue.send(()=>sendInput(text,input_kind,performance.now()-queued),text.length*2).catch(()=>{});});
    setInputSupported(input.supported);
    if(!task&&['powershell','sh'].includes(workspace?.program||'')){
      let resetAt:number|undefined;
      replayCursor.reset=async at=>{if(resetAt===at)return;resetAt=at;if(at>0)await resetTerminalWithHistory(term,input.write);else term.reset()};
    }
    const clipboard=installTerminalClipboard(term,{
      environment:ptyEnvironment,
      current:()=>!live.current.readOnly&&live.current.visible&&input.supported&&!disposed&&terminal.current===term&&!term.options.disableStdin,
      preferences:()=>preferencesRef.current,report,
      schedule:action=>inputQueue.send(action,0),
      pasteText:async value=>{
        // Let xterm retain its paste normalization and bracketed-mode behavior.
        // Capture only its synchronous emission, then send inside the already
        // reserved queue action instead of enqueueing behind subsequent keys.
        const captured:NonNullable<typeof pasteCapture>=[];pasteCapture=captured;
        try{term.paste(value)}finally{pasteCapture=undefined}
        if(captured.reduce((size,item)=>size+item.text.length*2,0)>1024*1024)throw new Error('粘贴内容过大，未发送。');
        for(const item of captured)await sendInput(item.text,item.input_kind);
      },
      send:async value=>{if(value.length*2>1024*1024)throw new Error('粘贴内容过大，未发送。');if(!disposed&&live.current.visible)await sendInput(value)},
    });
    pasteIntoSession.current=clipboard.paste;copySelection.current=clipboard.copy;
    const renderer=manageTerminalRenderer(term,resize,value=>{if(!disposed)setRendererStatus(value)},term.options.theme?.background===surfaceThemes.light.background);
    let rendererMode:TerminalPreferences['renderer']|undefined;
    applyAppearance.current=()=>{
      if(disposed)return;
      const value=preferencesRef.current;
      const {cursorBlink,...appearance}=terminalAppearance(value,ptyEnvironment);
      term.options={...appearance};focus.setBlink(cursorBlink===true);
      const white=term.options.theme?.background===surfaceThemes.light.background;
      if(rendererMode!==value.renderer){rendererMode=value.renderer;renderer.update(value.renderer)}
      renderer.setWhiteSurface(white);
      resize();
    };
    applyAppearance.current();
    const fonts=()=>{if(!disposed){term.refresh(0,term.rows-1);resize()}};
    document.fonts.addEventListener('loadingdone',fonts);window.addEventListener('resize',resize);window.addEventListener('focus',resize);
    const viewport=container.current;
    viewport.addEventListener('focusin',resize);document.addEventListener('pointerdown',resize);
    void document.fonts.load('14px "Velella Nerd Font Symbols"').then(fonts).catch(report);
    controlTerminal.current=async human=>{await inputQueue.drain();if(!disposed)await request('terminal.control',{human});};
    pump=createTerminalOutputPump(async()=>{
        const data=await request('terminal.output',{since:offset});
        if(disposed)return {backlog:false,changed:false};
        const previous=offset;
        if(replayEnd===undefined){replayEnd=data.total;setGeometryMissing(data.total>0&&(!data.geometry?.length||data.geometry[0].offset>0||!!data.geometry_error));if(savedBoundary!==undefined&&savedBoundary>data.total){replayCursor.clearDisplayAt=undefined;setDisplayTrimmed(false);localStorage.removeItem(displayKey)}}
        {
          const bytes=Uint8Array.from(atob(data.data||''),c=>c.charCodeAt(0));
          const parseStarted=performance.now();
          const previousGeometry=replayCursor.sequence;
          await writeTerminalPage(bytes,data.from??offset,replayEnd??data.end,data.geometry??[],term,input.write,replayCursor);
          if(bytes.length)timing.record('output_parse',{ms:performance.now()-parseStarted,bytes:bytes.length,from:data.from??offset,end:data.end});
          if(disposed)return {backlog:false,changed:false};offset=data.end;
          if(replayCursor.sequence!==previousGeometry){negotiate.observe({cols:term.cols,rows:term.rows});if(caughtUp&&offset>=data.total)resize()}
          setTinyGeometryCount(replayCursor.tinyGeometryCount??0);
          if(container.current){container.current.dataset.terminalOffset=String(offset);container.current.dataset.terminalTotal=String(data.total)}
        }
        if(clearDisplayRequested&&offset>=data.total){
          clearDisplayRequested=false;
          // Save the consumed byte boundary before trimming. A later viewer
          // replays every original byte and trims at this same boundary.
          localStorage.setItem(displayKey,String(offset));
          clearTerminalDisplayHistory(term);
          replayCursor.clearDisplayAt=offset;replayCursor.displayCleared=true;
          setDisplayTrimmed(true);setError('');
        }
        if(!caughtUp&&offset>=(replayEnd??0)){caughtUp=true;resize();term.scrollToBottom();setLoading(false)}
        if(!disposed&&data.geometry_error)setError(`尺寸记录不可用：${data.geometry_error}`);
        return {backlog:offset<data.total,changed:offset>previous};
    },report);
    visibility.current=value=>{renderer.setVisible(value&&live.current.renderVisible);pump?.setVisible(value);if(value){resize();term.refresh(0,term.rows-1)}else term.blur()};
    visibility.current(live.current.visible);
    // Opening a viewer never steals focus from another input or IME composition.
    return()=>{disposed=true;captureHistory.current=()=>({frames:[],dropped:0});historyCapture.dispose();inputQueue.dispose();pump?.dispose();resumeInput.current=()=>{};clearDisplayHistory.current=()=>{};visibility.current=()=>{};document.fonts.removeEventListener('loadingdone',fonts);window.removeEventListener('resize',resize);window.removeEventListener('focus',resize);viewport.removeEventListener('focusin',resize);document.removeEventListener('pointerdown',resize);clipboard.dispose();focus.dispose();renderer.dispose();negotiate.dispose();composition.dispose();wheel.dispose();timing.dispose();input.dispose();observer.disconnect();dsr.dispose();primaryDA.dispose();scrollbar.dispose();if(terminal.current===term){terminal.current=null;searchAddon.current=null;copySelection.current=async()=>false;applyAppearance.current=()=>{};controlTerminal.current=async()=>{};pasteIntoSession.current=async()=>{};resizeTerminal.current=()=>{}}term.dispose();};
  // A retained session ID can own a new host after recovery. Dispose old input
  // on confirmed exit, then replay and renegotiate geometry for the new PID.
  },[task?.id,workspace?.projectId,workspace?.workspaceId,session?.id,workspace?.status?.pid,exited,target,replaySize,rawGeometry,displayKey,displayRevision]);
  useEffect(()=>{if(terminal.current)terminal.current.options.disableStdin=readOnly||!inputSupported||inputPaused||!visible||loading;visibility.current(visible)},[visible,renderVisible,readOnly,inputSupported,inputPaused,loading]);
  useEffect(()=>{
    if(loading)return;
    // Reveal first, then synchronize the DOM scrollbar. Scrolling a hidden
    // xterm before React commits the restored screen can leave it at old rows.
    const term=terminal.current,frame=requestAnimationFrame(()=>term?.scrollToBottom());
    return()=>cancelAnimationFrame(frame);
  },[loading]);
  useEffect(()=>{
    const term=terminal.current;if(!term)return;
    const palette=surfaceThemes[theme],configured=term.options.theme as Record<string,unknown>|undefined;
    if(Object.entries(palette).some(([key,value])=>configured?.[key]!==value))term.options.theme=palette;
    const contrast=theme==='light'?4.5:1;if(term.options.minimumContrastRatio!==contrast)term.options.minimumContrastRatio=contrast;
    applyAppearance.current();
  },[theme,preferences,session?.id]);
  useEffect(()=>{if(searchOpen)findInput.current?.focus()},[searchOpen]);
  if(!session)return <div className="empty small"><p>终端将在任务启动后出现在这里。</p><span>排队中的任务不会提前启动 Agent。</span></div>;
  function search(previous=false){if(!find)return;const found=previous?searchAddon.current?.findPrevious(find):searchAddon.current?.findNext(find);setError(found?'':'未找到终端文本。')}
  async function copy(){try{setError(await copySelection.current()?'':'请先选择要复制的终端文本。')}catch(e){setError(`复制失败：${String(e)}`)}}
  async function paste(){if(!inputLocked)await pasteIntoSession.current()}
  async function changeControl(human:boolean){if(historyOnly||controlBusy)return;try{setControlBusy(true);await controlTerminal.current(human);setError('')}catch(e){setError(String(e))}finally{setControlBusy(false)}}
  function changePreferences(value:TerminalPreferences){try{setPreferences(saveTerminalPreferences(value));setError('')}catch(e){setError(`终端设置保存失败：${String(e)}`)}}
  function changeFont(value:number){changePreferences({...preferences,fontSize:value})}
  function restoreDisplayHistory(){try{localStorage.removeItem(displayKey);setDisplayRevision(value=>value+1)}catch(e){setError(`恢复显示历史失败：${String(e)}`)}}
  function openHistory(){setHistoryFrames(captureHistory.current());setHistoryOpen(true)}
  function changeHistoryOpen(open:boolean){setHistoryOpen(open);if(!open)setHistoryFrames({frames:[],dropped:0})}
  async function exportPerformance(){
    if(exportBusy)return;setExportBusy(true);setExportResult('');
    try{
      const snapshot=performanceSnapshot.current();if(!snapshot)throw new Error('终端记录尚未就绪');
      if(window.velellaDesktop){
        const result=await invoke<{path:string}>('terminal_performance_export',{snapshot,target:currentTarget()});
        setExportResult(`已导出到：${result.path}`);
      }else{
        const url=URL.createObjectURL(new Blob([JSON.stringify(snapshot,null,2)],{type:'application/json'}));
        const link=document.createElement('a');link.href=url;link.download=`velella-terminal-performance-${Date.now()}.json`;
        document.body.append(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),1000);
        setExportResult('已请求浏览器下载，请在下载列表中查看。');
      }
    }catch(error){setExportResult(`导出失败：${String(error)}`)}finally{setExportBusy(false)}
  }
  const options=<>{!historyOnly&&guarded&&status?.human_control&&<Button disabled={controlBusy} onClick={()=>void changeControl(false)}>继续后台检查</Button>}<Button onClick={()=>setSearchOpen(true)}>查找终端文本</Button><Button onClick={()=>void copy()}>复制所选</Button><Button disabled={inputLocked} onClick={()=>void paste()}>粘贴文字或图片</Button><Button onClick={()=>setSettingsOpen(true)}>终端设置</Button><Button onClick={openHistory}>查看历史画面</Button><Button onClick={exportPerformance}>导出终端性能记录</Button><Button title="保留当前屏幕和原始记录，只清理显示中的旧历史" onClick={()=>clearDisplayHistory.current()}>清理显示历史</Button>{displayTrimmed&&<Button onClick={restoreDisplayHistory}>恢复完整显示历史</Button>}<div className="wb-menu-divider"/>{[12,14,16,18].map(size=><Button key={size} onClick={()=>changeFont(size)}>字号 {size}{preferences.fontSize===size?' · 当前':''}</Button>)}{onRecover&&<Button onClick={onRecover}>重新启动终端</Button>}{onStop&&<><div className="wb-menu-divider"/><Button className="danger" onClick={onStop}>停止进程</Button></>}</>;
  return <ContextMenu.Root>{visible&&actionsTarget&&createPortal(<div className="terminal-toolbar-actions"><Button aria-label="搜索终端" title="查找终端文本 · Ctrl+Shift+F" aria-expanded={searchOpen} onClick={()=>setSearchOpen(!searchOpen)}><Search size={14}/></Button><Button aria-label="终端设置" title="终端设置" onClick={()=>setSettingsOpen(true)}><Settings2 size={14}/></Button><ActionMenu label="终端操作" icon={<MoreHorizontal size={16}/>}>{options}</ActionMenu></div>,actionsTarget)}<ContextMenu.Trigger asChild><div className="terminal-wrap terminal-workbench" style={{'--terminal-bg':surfaceThemes[theme].background,'--terminal-fg':surfaceThemes[theme].foreground,'--terminal-selection':surfaceThemes[theme].selectionBackground} as CSSProperties} onKeyDownCapture={e=>{if(e.ctrlKey&&e.shiftKey&&e.key.toLowerCase()==='f'){e.preventDefault();e.stopPropagation();setSearchOpen(true)}}}>
    {searchOpen&&<form className="terminal-find" data-terminal-search-root onSubmit={e=>{e.preventDefault();search()}} onKeyDown={e=>{if(e.key==='Escape'){e.stopPropagation();setSearchOpen(false);terminal.current?.focus()}}}><Search size={14}/><Input ref={findInput} aria-label="查找终端文本" value={find} onChange={e=>setFind(e.target.value)}/><Button disabled={!find}>下一个</Button><Button type="button" disabled={!find} onClick={()=>search(true)}>上一个</Button><Button type="button" aria-label="关闭终端搜索" onClick={()=>{setSearchOpen(false);terminal.current?.focus()}}><X size={14}/></Button></form>}
    {!automaticallyRestoring&&(historyOnly||exited||hostLost||!status||!runtimeSupported||!inputSupported||recoveryNotice)&&<div className="terminal-status" role="status">
      {historyOnly?'历史记录 · 只读查看，不会启动或重启终端。':recoveryNotice|| (hostLost?'终端连接已断开，运行状态无法确认。历史记录已保留。':exited?'进程已结束 · 历史回放':!status?'正在重新连接终端…':!runtimeSupported?'此终端使用旧版输入协议，请重新启动终端。':!inputSupported?'终端输入组件不可用，请重新打开桌面。':'')}
      {!historyOnly&&!reconnecting&&(hostLost||exited)&&onRecover&&<Button onClick={onRecover}>重新启动终端</Button>}
      {!reconnecting&&(historyOnly||hostLost||exited)&&<Button disabled={loading} onClick={openHistory}>查看对话历史</Button>}
      {!reconnecting&&hostLost&&recoveryNotice&&<span>{recoveryNotice}</span>}
      {!hostLost&&!exited&&!reconnecting&&recoveryNotice&&<Button onClick={()=>setRecoveryNotice('')}>知道了</Button>}
    </div>}

    {tinyGeometryCount>0&&<details className="terminal-history-options"><summary>{rawGeometry?'正在按异常窗口尺寸原样回放':'已修正旧记录的异常窗口尺寸'}</summary><p>发现 {tinyGeometryCount} 次异常尺寸。原始历史未改动；异常期间的画面可能不完整。</p>{readOnly&&<Button onClick={()=>setRawGeometry(!rawGeometry)}>{rawGeometry?'修正异常尺寸':'查看原始回放'}</Button>}</details>}
    {geometryMissing&&<details className="terminal-history-options"><summary>旧记录缺少窗口尺寸，历史画面可能错位</summary>{readOnly&&<form onSubmit={event=>{event.preventDefault();const values=new FormData(event.currentTarget);setReplaySize({cols:Number(values.get('cols')),rows:Number(values.get('rows'))})}}><label>列 <Input name="cols" type="number" min={2} max={1000} required defaultValue={replaySize?.cols??120}/></label><label>行 <Input name="rows" type="number" min={1} max={1000} required defaultValue={replaySize?.rows??30}/></label><Button>按此尺寸重新回放</Button></form>}<p>原始记录已保留；调整回放尺寸不会启动进程。</p></details>}
    {workspace?.workspaceId&&['claude','codex'].includes(workspace.program||'')&&<TerminalActivity key={`${workspace.workspaceId}:${workspace.id}`} workspaceId={workspace.workspaceId} sessionId={workspace.id} visible={visible} exited={exited||hostLost}/>}
    {(loading||reconnecting)&&<div className="terminal-loading"><LoadingState label="正在恢复会话"/></div>}
    <div ref={container} aria-busy={loading} data-terminal-loading={loading?'true':undefined} data-terminal-renderer={rendererStatus.kind} className={`terminal-canvas${readOnly?' terminal-history':''}`}/>
    <TerminalFrames open={historyOpen} onOpenChange={changeHistoryOpen} value={historyFrames} preferences={preferences}/>
    {(exportBusy||exportResult)&&<div className="terminal-status" role="status">{exportBusy?'正在导出性能记录…':exportResult}</div>}
    <TerminalSettings open={settingsOpen} onOpenChange={setSettingsOpen} value={preferences} onChange={changePreferences} renderer={rendererStatus}/>
    {error&&<div className="terminal-error" role="status">{inputPaused?'输入已暂停，未确认和排队输入不会自动补发。 ':''}{error}{inputPaused&&<Button disabled={readOnly} onClick={()=>resumeInput.current()}>核对后恢复输入</Button>}</div>}
  </div></ContextMenu.Trigger><ContextMenu.Portal><ContextMenu.Content className="wb-menu-panel" collisionPadding={8}>
    <ContextMenu.Item className="ui-button" onSelect={()=>setSearchOpen(true)}>查找终端文本</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" onSelect={()=>void copy()}>复制所选</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" disabled={inputLocked} onSelect={()=>void paste()}>粘贴文字或图片</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" onSelect={()=>setSettingsOpen(true)}>终端设置</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" onSelect={openHistory}>查看历史画面</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" onSelect={exportPerformance}>导出终端性能记录</ContextMenu.Item>
    <ContextMenu.Item className="ui-button" onSelect={()=>clearDisplayHistory.current()}>清理显示历史</ContextMenu.Item>
    {displayTrimmed&&<ContextMenu.Item className="ui-button" onSelect={restoreDisplayHistory}>恢复完整显示历史</ContextMenu.Item>}
    <ContextMenu.Separator className="wb-menu-divider"/>
    {[12,14,16,18].map(size=><ContextMenu.Item className="ui-button" key={size} onSelect={()=>changeFont(size)}>字号 {size}{preferences.fontSize===size?' · 当前':''}</ContextMenu.Item>)}
    {onRecover&&<ContextMenu.Item className="ui-button" onSelect={onRecover}>重新启动终端</ContextMenu.Item>}
    {onStop&&<ContextMenu.Item className="ui-button danger" onSelect={onStop}>停止进程</ContextMenu.Item>}
  </ContextMenu.Content></ContextMenu.Portal></ContextMenu.Root>;
}
