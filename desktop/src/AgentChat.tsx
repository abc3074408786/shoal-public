import {useRef,useState} from 'react';
import {Button,Textarea} from './ui';
import {rpc} from './api';
import TerminalView from './TerminalSessions';
import {Session} from './workbench-types';
import AnnotationCards from './AnnotationCards';
import {useAnnotations} from './useAnnotations';
import {acknowledgeAnnotationSubmission,beginAnnotationSubmission,editAnnotation,emptyChat,setChatText,settleAnnotationSubmission} from './annotation-drafts';
import './annotation-chat.css';

export default function AgentChat({workspaceId,session,onTerminal,actionsTarget}:{workspaceId:string;session?:Session;onTerminal?:()=>void;actionsTarget?:HTMLElement|null}){
  const {state,error:storageError}=useAnnotations(workspaceId);
  const [error,setError]=useState(''),[composer,setComposer]=useState(true),[busy,setBusy]=useState(false),[message,setMessage]=useState('');
  const inFlight=useRef(false),composing=useRef(false);
  if(!session)return <div className="ide-empty">会话记录不可用；请刷新工作区。</div>;
  if(session.native_chat)return <div className="ide-empty">独立 Codex 聊天已停用，历史数据保留在原会话目录。请新建 Codex 终端继续工作。</div>;
  const activeSession=session,chat=state.chats[session.id]??emptyChat(),items=state.items.filter(i=>i.sessionId===session.id);
  const locked=!!chat.pending,available=!!session.status&&session.status.exit===null;
  async function run(action:()=>Promise<void>){if(inFlight.current)return;inFlight.current=true;setBusy(true);setError('');try{await action()}catch(e){setError(String(e))}finally{inFlight.current=false;setBusy(false)}}
  function local(action:()=>unknown){try{action();setError('')}catch(e){setError(String(e))}}
  async function send(e:React.FormEvent){e.preventDefault();if(!available||locked||busy||storageError)return;await run(async()=>{
    const pending=beginAnnotationSubmission(workspaceId,activeSession.id);
    try{
      const r=await rpc<any>({cmd:'workbench.feedback.send',workspace_id:workspaceId,session_id:activeSession.id,request_id:pending.id,text:pending.prompt,source:pending.source});
      settleAnnotationSubmission(workspaceId,activeSession.id,pending.id,r.receipt?.state);
      setMessage(r.receipt?.state==='submitted'?'已提交到当前会话。Agent 的执行与权限提示显示在上方。':'发送结果待核对，请查看当前会话并查询回执。');
    }catch(e){settleAnnotationSubmission(workspaceId,activeSession.id,pending.id,'uncertain');throw e;}
  })}
  async function inspect(){if(!chat.pending)return;const id=chat.pending.id;await run(async()=>{
    const r=await rpc<any>({cmd:'workbench.feedback.status',workspace_id:workspaceId,request_id:id});
    settleAnnotationSubmission(workspaceId,activeSession.id,id,r.receipt?.state);
    setMessage(r.receipt?.state==='submitted'?'回执确认已提交。':'尚未确认提交，请检查上方会话；不会自动重发。');
  })}
  async function returnControl(){await run(async()=>{await rpc({cmd:'workbench.terminal.control',workspace_id:workspaceId,session_id:activeSession.id,human:false})})}
  return <div className="agent-chat">
    <div className="panel-toolbar"><strong>{session.title}</strong><span>当前会话</span>{onTerminal&&<Button onClick={onTerminal}>打开终端</Button>}{session.task_id&&<Button disabled={busy} onClick={()=>void returnControl()}>交还自动执行</Button>}<Button aria-expanded={composer} onClick={()=>setComposer(!composer)}>{composer?'收起聊天输入':'展开聊天输入'}{items.length?` · ${items.length} 条批注`:''}</Button></div>
    <div className="agent-chat-output"><TerminalView actionsTarget={actionsTarget} workspace={{projectId:'',workspaceId,id:session.id,agent:session.kind==='agent',status:session.status}}/></div>
    {composer&&<form className="agent-chat-compose annotation-compose" onSubmit={send}>
      {chat.history.length>0&&<details className="annotation-sent"><summary>发送记录与批注 · {chat.history.length}</summary>{chat.history.map(entry=><article key={entry.id}><p>{entry.text||'请处理以下网页批注'}</p><AnnotationCards items={entry.items} workspaceId={workspaceId}/></article>)}</details>}
      {items.length>0&&<><strong>网页批注 · {items.length}</strong><AnnotationCards items={items} workspaceId={workspaceId} disabled={busy||locked} onEdit={(id,text)=>local(()=>editAnnotation(workspaceId,id,text))} onRemove={id=>local(()=>editAnnotation(workspaceId,id,null))}/></>}
      <Textarea onCompositionStart={()=>{composing.current=true}} onCompositionEnd={()=>{composing.current=false}} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.ctrlKey&&!e.altKey&&!e.metaKey&&!composing.current&&!e.nativeEvent.isComposing&&e.nativeEvent.keyCode!==229){e.preventDefault();if(available&&!locked&&!busy)e.currentTarget.form?.requestSubmit()}}} aria-label="发送给 Agent" value={chat.text} disabled={busy||locked||!!storageError} maxLength={4000} onChange={e=>local(()=>setChatText(workspaceId,session.id,e.target.value))} placeholder={items.length?'补充总体要求，和这些批注一起发送…':'继续当前会话…'} rows={2}/>
      <div className="annotation-compose-actions"><small>{locked?'发送结果待核对，草稿已保留。':items.length?'保存时的页面上下文会随批注一起发送。':'输入与原会话同步，关闭标签不会停止 Agent。'}</small><Button disabled={busy||locked||!available||!!storageError||(!chat.text.trim()&&!items.length)}>{items.length?`发送消息与 ${items.length} 条批注`:'发送'}</Button></div>
      {locked&&<div className="annotation-compose-actions"><Button type="button" disabled={busy} onClick={()=>void inspect()}>查询发送回执</Button>{chat.pending?.state==='uncertain'&&<><Button type="button" disabled={busy} onClick={()=>local(()=>acknowledgeAnnotationSubmission(workspaceId,session.id,'delivered'))}>确认已送达，归档草稿</Button><Button type="button" disabled={busy} onClick={()=>local(()=>acknowledgeAnnotationSubmission(workspaceId,session.id,'not-delivered'))}>确认未送达，允许重新发送</Button></>}</div>}
    </form>}
    {message&&<p role="status" className="browser-annotation-note">{message}</p>}
    {(error||storageError)&&<p className="ide-error" role="alert">{error||storageError}</p>}
  </div>;
}
