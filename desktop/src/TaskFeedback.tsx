import {useEffect,useRef,useState} from 'react';
import {rpc} from './api';
import {currentTarget,namespacedKey} from './execution-target';
import {Button,Textarea} from './ui';

type Pending={request_id:string;text:string};
type Props={taskId:string;enabled:boolean;onChanged:()=>Promise<void>};
export default function TaskFeedback(props:Props){
  return <TaskFeedbackForm key={props.taskId} {...props}/>;
}
function TaskFeedbackForm({taskId,enabled,onChanged}:Props){
  // The task belongs to the host that was selected when this form mounted; feedback must not be
  // re-routed if the user switches execution hosts before the receipt is confirmed.
  const target=useRef(currentTarget()).current;
  const key=namespacedKey(target,`velella:task-feedback:${taskId}`);
  const active=useRef(false),inFlight=useRef(false);
  const [text,setText]=useState(''),[pending,setPending]=useState<Pending|null>(null),[busy,setBusy]=useState(false),[message,setMessage]=useState('');
  useEffect(()=>{active.current=true;try{const saved=localStorage.getItem(key);if(saved){const value=JSON.parse(saved);setPending(value);setText(value.text);setMessage('上次发送尚未核对，请查询原回执。')}}catch{setMessage('无法读取上次发送记录，请检查原会话后再操作。')}return()=>{active.current=false}},[key]);
  async function run(query:boolean){
    if(inFlight.current||!active.current||(query&&!pending))return;
    inFlight.current=true;setBusy(true);
    try{
      const value=pending||{request_id:crypto.randomUUID(),text};
      localStorage.setItem(key,JSON.stringify(value));setPending(value);
      const result=await rpc(query?{cmd:'task.feedback.status',task_id:taskId,request_id:value.request_id}:{cmd:'task.feedback',task_id:taskId,...value},target);
      if(!active.current)return;
      const receipt=result.receipt;
      if(receipt?.state==='submitted'){localStorage.removeItem(key);setPending(null);setText('');setMessage('已提交到终端（PTY 输入已确认）；不代表 Agent 已接受或开始执行。')}
      else if(receipt?.state==='rejected'){localStorage.removeItem(key);setPending(null);setMessage(`未发送：${receipt.error||'会话尚未就绪'}`)}
      else setMessage(receipt?'送达结果尚未确认。请检查原终端；不会自动重发。':'服务端尚无回执。可用原请求重试；不会创建重复输入。');
      await onChanged();
    }catch(error){if(active.current)setMessage(`${String(error)}；保留原请求，可查询回执。`)}finally{inFlight.current=false;if(active.current)setBusy(false)}
  }
  return <form onSubmit={e=>{e.preventDefault();void run(false)}}>
    <label>审查意见<Textarea value={text} readOnly={!!pending} disabled={busy} onChange={e=>setText(e.target.value)} rows={3}/></label>
    <Button disabled={busy||!text.trim()||!enabled}>{pending?'用原请求重试':'发送给 Agent'}</Button>
    {pending&&<Button type="button" disabled={busy} onClick={()=>void run(true)}>查询原回执</Button>}
    {message&&<p role="status">{message}</p>}
  </form>;
}
