import {useEffect,useRef,useState,type ReactNode} from 'react';
import {createPortal} from 'react-dom';
import {Columns2,Maximize2,Minimize2,MoreHorizontal,X} from 'lucide-react';
import {Button,Tip} from './ui';
import ActionMenu from './ActionMenu';
import SplitSurface from './SplitSurface';
import TerminalView from './TerminalSessions';
import {rpc} from './api';
import type {Session,SplitAxis,SplitNode,Tab,Workspace} from './workbench-types';
import {dropEdge,leafIds,moveLeaf,resizeSplit,terminalTree,type SplitEdge,MAX_SPLIT_LEAVES} from './split-layout';
import {terminalSessionTitle} from './terminal-execution';
import {useNativeSplitDrop,useNativeSplitFocus} from './useNativeSplitDrop';
import NativeSessionRestore from './NativeSessionRestore';

type Props={workspace:Workspace;tab:Tab;sessions:Session[];actionsTarget?:HTMLElement|null;focusSession?:{session:string;request:number};disabled:boolean;onSplit:(session:string,axis:SplitAxis)=>void;onTree:(tree:SplitNode)=>void;onClose:(session:string)=>void;onStop:(session:Session)=>void;onRecover:(session:Session)=>void;onReconnected?:()=>Promise<void>};
function TerminalPane({session,workspace,grouped,active,actionsTarget,controls,onActive,onDrag,children}:{session:Session|undefined;workspace:Workspace;grouped:boolean;active:boolean;actionsTarget?:HTMLElement|null;controls:ReactNode;onActive:()=>void;onDrag:(event:React.DragEvent)=>void;children:(target:HTMLElement|null)=>ReactNode}){
  const [target,setTarget]=useState<HTMLDivElement|null>(null);
  return <section className={`terminal-pane ${grouped?'is-grouped':''} ${active?'is-active':''}`} data-terminal-pane={session?.id} aria-label={`终端分屏 ${session?terminalSessionTitle(session):'载入中'}`} onFocusCapture={onActive} onPointerDownCapture={onActive}>
    {grouped&&<header className="terminal-pane-header"><span className="terminal-pane-title" draggable onDragStart={onDrag} title="拖到另一终端边缘移动分屏">{session?terminalSessionTitle(session):workspace.name}</span><div className="terminal-pane-actions" ref={setTarget}/></header>}
    {(grouped?target:actionsTarget)&&createPortal(controls,(grouped?target:actionsTarget)!)}
    {children(grouped?target:actionsTarget||null)}
  </section>;
}

export default function TerminalSplitView(props:Props){
  const {workspace,tab,sessions,actionsTarget,disabled,onSplit,onTree,onClose,onStop,onRecover}=props;
  const tree=terminalTree(tab),ids=leafIds(tree),grouped=ids.length>1;
  const [active,setActive]=useState(tab.target),[maximized,setMaximized]=useState<string>(),[drop,setDrop]=useState<{id:string;edge:SplitEdge}>();
  const [selectionRequest,setSelectionRequest]=useState<Record<string,number>>({});
  const root=useRef<HTMLDivElement>(null),previous=useRef(ids);
  function targetPane(event:DragEvent){const element=(event.target as Element)?.closest<HTMLElement>('[data-terminal-pane]');if(!element||!root.current?.contains(element))return;const box=element.getBoundingClientRect();return {id:element.dataset.terminalPane!,edge:dropEdge(event.clientX-box.left,event.clientY-box.top,box.width,box.height)||'right' as SplitEdge};}
  useNativeSplitDrop(root,'text/velella-terminal-pane',{over:event=>{if(!disabled)setDrop(targetPane(event))},drop:event=>{if(disabled)return;try{const source=JSON.parse(event.dataTransfer!.getData('text/velella-terminal-pane')),target=targetPane(event);if(source.tab===tab.id&&target)onTree(moveLeaf(tree,source.session,target.id,target.edge));}catch{/* External payloads are ignored. */}},leave:()=>setDrop(undefined)});
  useEffect(()=>{const id=props.focusSession?.session;if(!id||!ids.includes(id))return;setActive(id);setMaximized(undefined);const frame=requestAnimationFrame(()=>{if(!document.querySelector('dialog[open],[role=menu]'))root.current?.querySelector<HTMLElement>(`[data-terminal-pane="${CSS.escape(id)}"] textarea`)?.focus()});return()=>cancelAnimationFrame(frame)},[props.focusSession?.request]);
  useEffect(()=>{
    const added=ids.find(id=>!previous.current.includes(id));previous.current=ids;
    if(!ids.includes(active))setActive(ids[0]);
    if(maximized&&!ids.includes(maximized))setMaximized(undefined);
    if(added){setActive(added);setMaximized(undefined);const frame=requestAnimationFrame(()=>{if(!document.querySelector('dialog[open],[role=menu]'))root.current?.querySelector<HTMLElement>(`[data-terminal-pane="${CSS.escape(added)}"] textarea`)?.focus()});return()=>cancelAnimationFrame(frame);}
  },[ids.join('|')]);
  const activeId=ids.includes(active)?active:ids[0];
  useNativeSplitFocus(root,event=>{const pane=(event.target as Element)?.closest<HTMLElement>('[data-terminal-pane]');if(pane)setActive(pane.dataset.terminalPane!);},e=>{
    if(disabled||e.isComposing||document.querySelector('dialog[open],[role=menu]'))return;
    if(e.shiftKey&&e.code==='KeyD'&&(e.ctrlKey||e.altKey)){e.preventDefault();e.stopPropagation();if(ids.length<MAX_SPLIT_LEAVES)onSplit(activeId,e.altKey?'vertical':'horizontal');}
    if(e.ctrlKey&&!e.shiftKey&&!e.altKey&&['BracketLeft','BracketRight'].includes(e.code)){e.preventDefault();e.stopPropagation();const id=ids[(ids.indexOf(activeId)+(e.code==='BracketRight'?1:-1)+ids.length)%ids.length];setActive(id);if(maximized)setMaximized(id);requestAnimationFrame(()=>root.current?.querySelector<HTMLElement>(`[data-terminal-pane="${CSS.escape(id)}"] textarea`)?.focus());}
  });
  return <div className="terminal-group" ref={root}>
    <SplitSurface tree={tree} label="终端" disabled={disabled} maximized={maximized} onResize={(id,ratio)=>onTree(resizeSplit(tree,id,ratio))} renderLeaf={sid=>{
      const session=sessions.find(s=>s.id===sid);
      const splitDisabled=disabled||ids.length>=MAX_SPLIT_LEAVES;
      const controls=<div className="terminal-split-actions"><Tip label="向右拆分终端"><Button aria-label="向右拆分终端" disabled={splitDisabled} onClick={()=>onSplit(sid,'horizontal')}><Columns2 size={14}/></Button></Tip><ActionMenu label="终端分屏操作" disabled={disabled} icon={<MoreHorizontal size={14}/>}>
        <Button disabled={splitDisabled} onClick={()=>onSplit(sid,'horizontal')}>向右拆分终端</Button><Button disabled={splitDisabled} onClick={()=>onSplit(sid,'vertical')}>向下拆分终端</Button>
        {grouped&&<><Button onClick={()=>setMaximized(maximized?undefined:sid)}>{maximized?'还原终端分屏':'展开当前终端'}</Button><Button onClick={()=>onClose(sid)}>关闭当前分屏（停止进程）</Button></>}
      </ActionMenu>{grouped&&<><Tip label={maximized?'还原终端分屏':'展开当前终端'}><Button aria-label={maximized?'还原终端分屏':'展开当前终端'} onClick={()=>setMaximized(maximized?undefined:sid)}>{maximized?<Minimize2 size={13}/>:<Maximize2 size={13}/>}</Button></Tip><Button aria-label="关闭终端分屏" title="关闭分屏并停止对应进程，历史记录保留" disabled={disabled} onClick={()=>onClose(sid)}><X size={13}/></Button></>}</div>;
      return <div className="terminal-group">
        {session?.restore_selection_required&&<NativeSessionRestore workspaceId={workspace.id} session={session} onSelected={async()=>{await props.onReconnected?.();setSelectionRequest(value=>({...value,[sid]:(value[sid]||0)+1}))}}/>}
        <TerminalPane session={session} workspace={workspace} grouped={grouped} active={activeId===sid} actionsTarget={actionsTarget} controls={controls} onActive={()=>setActive(sid)} onDrag={e=>{if(disabled){e.preventDefault();return;}e.stopPropagation();e.dataTransfer.setData('text/velella-terminal-pane',JSON.stringify({tab:tab.id,session:sid}));e.dataTransfer.effectAllowed='move'}}>{target=><TerminalView renderVisible={!maximized||!ids.includes(maximized)||maximized===sid} actionsTarget={target} openRequest={(props.focusSession?.session===sid?props.focusSession.request:0)+(selectionRequest[sid]||0)} onStop={session?()=>onStop(session):undefined} onRecover={session?.recoverable?()=>onRecover(session):undefined} onReconnect={session?.auto_recoverable?async()=>{await rpc({cmd:'workbench.terminal.recover',workspace_id:workspace.id,session_id:sid,automatic:true});await props.onReconnected?.()}:undefined} onOpenReconnect={session?.open_recoverable?async()=>{await rpc({cmd:'workbench.terminal.recover',workspace_id:workspace.id,session_id:sid,automatic:true,on_open:true});await props.onReconnected?.()}:undefined} workspace={{projectId:workspace.project_id,workspaceId:workspace.id,id:sid,program:session?.program,agent:session?.kind==='agent',status:session?.status}}/>}</TerminalPane>
        {drop?.id===sid&&<div className="split-drop-hint" data-edge={drop.edge}>移动终端</div>}
      </div>;
    }}/>
  </div>;
}
