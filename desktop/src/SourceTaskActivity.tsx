import {useEffect,useRef,useState} from 'react';
import {rpc} from './api';
import {Button,Input,Textarea} from './ui';
import {SourceItem} from './task-sources';
import SourceTaskBody from './SourceTaskBody';
import {sourceDate} from './SourceTaskInfo';

export default function SourceTaskActivity({item,onChanged}:{item:SourceItem;onChanged:()=>void}){
  const [states,setStates]=useState<{id:string;name:string}[]>([]),[comments,setComments]=useState<{id:string;body:string;author:string;created:string}[]>([]),[next,setNext]=useState<string|null>(null);
  const [state,setState]=useState(''),[comment,setComment]=useState(''),[editing,setEditing]=useState(false),[title,setTitle]=useState(item.title),[description,setDescription]=useState(item.description),[busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('');
  const pending=useRef<{id:string;fields:Record<string,string>}|null>(null),inFlight=useRef(false);
  const [commentsBusy,setCommentsBusy]=useState(true),[commentsError,setCommentsError]=useState('');
  const commentGeneration=useRef(0),failedCursor=useRef('');
  async function loadComments(cursor=''){
    const serial=++commentGeneration.current;setCommentsBusy(true);setCommentsError('');failedCursor.current=cursor;
    try{
      const r=await rpc({cmd:'sources.comments',source_id:item.source_id,key:item.key,cursor});
      if(serial!==commentGeneration.current)return;
      setComments(old=>cursor?[...old,...(r.comments||[]).filter((c:{id:string})=>!old.some(previous=>previous.id===c.id))]:r.comments||[]);setNext(r.next||null);
    }catch(e){if(serial===commentGeneration.current)setCommentsError(e instanceof Error?e.message:String(e))}
    finally{if(serial===commentGeneration.current)setCommentsBusy(false)}
  }
  useEffect(()=>{void loadComments();return()=>{commentGeneration.current++}},[item.key]);
  useEffect(()=>{let live=true;void rpc({cmd:'sources.states',source_id:item.source_id,key:item.key}).then(r=>{if(live)setStates(r.states||[])}).catch(e=>{if(live)setError(String(e))});return()=>{live=false}},[item.key]);
  async function mutate(fields:Record<string,string>){
    if(inFlight.current)return;inFlight.current=true;setBusy(true);setError('');setNotice('');
    const request=pending.current||{id:crypto.randomUUID(),fields};pending.current=request;
    try{await rpc({cmd:'sources.mutate',source_id:item.source_id,key:item.key,request_id:request.id,fields:request.fields});pending.current=null;setNotice('已保存到来源平台');setComment('');setEditing(false);onChanged();if(fields.action==='comment')await loadComments()}
    catch(e){setError(String(e))}finally{inFlight.current=false;setBusy(false)}
  }
  return <section className="source-activity" aria-label="任务操作与讨论">
    <div className="source-detail-actions"><label>来源状态<select aria-label="来源状态" value={state} disabled={busy||!!pending.current} onChange={e=>setState(e.target.value)}><option value="">选择状态</option>{states.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select></label><Button disabled={!state||busy||!!pending.current} onClick={()=>void mutate({action:'state',state})}>更新状态</Button><Button disabled={busy||!!pending.current} onClick={()=>{if(!editing){setTitle(item.title);setDescription(item.description)}setEditing(!editing)}}>编辑任务</Button></div>
    {editing&&<form onSubmit={e=>{e.preventDefault();void mutate({action:'edit',title,description})}}><label>任务标题<Input required disabled={busy||!!pending.current} value={title} onChange={e=>setTitle(e.target.value)}/></label><label>任务描述<Textarea rows={6} disabled={busy||!!pending.current} value={description} onChange={e=>setDescription(e.target.value)}/></label><Button disabled={busy||!!pending.current}>保存任务</Button></form>}
    <section className="source-comments" aria-label="任务评论" aria-busy={commentsBusy}>
      <div className="source-comments-heading"><h3>评论与讨论{comments.length>0?`（已加载 ${comments.length} 条）`:''}</h3><Button disabled={commentsBusy} onClick={()=>void loadComments()}>刷新评论</Button></div>
      {commentsBusy&&<p role="status">正在加载评论…</p>}
      {commentsError&&<div role="alert"><p>{commentsError}</p><Button disabled={commentsBusy} onClick={()=>void loadComments(failedCursor.current)}>重试加载评论</Button></div>}
      {!commentsBusy&&!commentsError&&!comments.length&&<p className="source-muted">暂无评论，可以补充问题、进展或验收结果。</p>}
      {comments.map(c=><article key={c.id}><small>{c.author||'未知作者'} · {sourceDate(c.created)||'时间未提供'}</small><SourceTaskBody source={c.body}/></article>)}
      {next&&!commentsError&&<Button disabled={commentsBusy} onClick={()=>void loadComments(next)}>更多评论</Button>}
      <form onSubmit={e=>{e.preventDefault();void mutate({action:'comment',body:comment})}}><label>评论<Textarea rows={3} required placeholder="补充问题、进展或验收结果…" disabled={busy||!!pending.current} value={comment} onChange={e=>setComment(e.target.value)}/></label><Button disabled={busy||!comment.trim()||!!pending.current}>发布评论</Button></form>
    </section>
    {error&&<p className="source-error" role="alert">{error}</p>}{pending.current&&!busy&&<p>修改回执已保留。请在原任务核对结果，避免重复发布。</p>}{notice&&<p role="status">{notice}</p>}
  </section>;
}
