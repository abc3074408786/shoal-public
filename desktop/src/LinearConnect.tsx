import {useEffect,useRef,useState} from 'react';
import {ExternalLink,QrCode,X} from 'lucide-react';
import {rpc} from './api';
import {Button,NativeDialog} from './ui';
import {openSourceUrl,TaskSource} from './task-sources';

type Attempt={request_id:string;state:string;authorization_url?:string;qr_svg?:string;message?:string;source?:TaskSource};
export default function LinearConnect({onClose,onSaved,onManual}:{onClose:()=>void;onSaved:(source:TaskSource)=>void;onManual:()=>void}) {
  const dialog=useRef<HTMLDialogElement>(null),alive=useRef(false),requestId=useRef<string|null>(null),starting=useRef(false),cancelling=useRef(false),accepted=useRef(new Set<string>());
  const [configured,setConfigured]=useState<boolean|null>(null),[message,setMessage]=useState(''),[error,setError]=useState(''),[attempt,setAttempt]=useState<Attempt|null>(null);
  const [stopping,setStopping]=useState(false),[mode,setMode]=useState<'browser'|'qr'>('browser');
  const opened=useRef(new Set<string>());
  useEffect(()=>{
    alive.current=true;dialog.current?.showModal();void check();
    return()=>{alive.current=false;const id=requestId.current;if(id&&!accepted.current.has(id))void rpc({cmd:'sources.linear.auth.cancel',request_id:id}).catch(()=>{});};
  },[]);
  async function check(){
    setError('');setConfigured(null);
    try{const r=await rpc({cmd:'sources.linear.account'});if(alive.current){setConfigured(r.configured);setMode(r.mode==='qr'?'qr':'browser');setMessage(r.message||'');}}
    catch{if(alive.current){setConfigured(false);setError('无法检查 Linear 授权服务，请重新检查。');}}
  }
  function accept(value:Attempt){
    if(!alive.current||requestId.current!==value.request_id||accepted.current.has(value.request_id))return;
    if(['complete','cancelled','denied','expired','failed'].includes(value.state))accepted.current.add(value.request_id);
    setAttempt(value);
    if(mode==='browser'&&value.state==='waiting'&&value.authorization_url&&!cancelling.current&&!opened.current.has(value.request_id)){
      opened.current.add(value.request_id);void openSourceUrl(value.authorization_url).catch(()=>{if(alive.current)setError('无法自动打开浏览器，请点击“重新打开浏览器”。');});
    }
    if(value.state==='complete'&&value.source){onSaved(value.source);onClose();}
  }
  async function poll(id:string){
    try{const r=await rpc({cmd:'sources.linear.auth.status',request_id:id});accept(r.attempt);}
    catch{if(alive.current&&requestId.current===id&&!accepted.current.has(id))setAttempt({request_id:id,state:'uncertain',message:'暂时无法确认授权结果，请检查结果后再操作。'});}
  }
  useEffect(()=>{
    if(!attempt||!['starting','waiting'].includes(attempt.state))return;
    let live=true;let timer:ReturnType<typeof setTimeout>;const id=attempt.request_id;
    const tick=async()=>{await poll(id);if(live)timer=setTimeout(tick,1200);};
    timer=setTimeout(tick,1200);return()=>{live=false;clearTimeout(timer);};
  },[attempt?.request_id,attempt?.state]);
  const pending=!!attempt&&['requesting','starting','waiting','uncertain'].includes(attempt.state);
  async function begin(){
    if(starting.current||pending)return;
    const id=crypto.randomUUID();requestId.current=id;starting.current=true;setError('');setAttempt({request_id:id,state:'requesting'});
    try{const r=await rpc({cmd:'sources.linear.auth.start',request_id:id});accept(r.attempt);}
    catch{if(alive.current&&requestId.current===id)await poll(id);}
    finally{starting.current=false;if(!alive.current)void rpc({cmd:'sources.linear.auth.cancel',request_id:id}).catch(()=>{});}
  }
  async function stop(closeAfter=false){
    const id=requestId.current;if(!id||cancelling.current)return;cancelling.current=true;setStopping(true);setError('');
    try{const r=await rpc({cmd:'sources.linear.auth.cancel',request_id:id});accept(r.attempt);if(closeAfter&&alive.current&&r.attempt.state!=='complete')onClose();}
    catch{if(alive.current)setError('停止等待的结果未确认，请检查授权结果。');}
    finally{cancelling.current=false;if(alive.current)setStopping(false);}
  }
  function close(){if(requestId.current&&!accepted.current.has(requestId.current))void stop(true);else onClose();}
  const qrReady=attempt?.state==='waiting'&&attempt.qr_svg&&attempt.authorization_url;
  return <NativeDialog ref={dialog} className="source-dialog github-connect" aria-labelledby="linear-connect-title" onClose={onClose} onCancel={e=>{e.preventDefault();close();}}>
    <div className="dialog-heading"><h2 id="linear-connect-title">连接 Linear</h2><Button disabled={stopping} aria-label="关闭 Linear 连接" onClick={close}><X size={16}/></Button></div>
    <div className="dialog-body">
      <div className="github-auth-intro">{mode==='qr'?<QrCode size={30}/>:<ExternalLink size={30}/>}<h3>{mode==='qr'?'扫码连接 Linear':'在浏览器连接 Linear'}</h3><p>{mode==='qr'?'用手机扫码，或在本机浏览器授权。':'在这台电脑的浏览器中登录并授权。'}完成后将自动连接你的 Linear 工作区。</p></div>
      {configured===null?<p className="source-muted" role="status">正在检查授权服务…</p>:!configured?<><p className="source-muted" role="status">{message}</p><Button onClick={()=>void check()}>重新检查</Button></>:<>
        {mode==='browser'&&attempt?.state==='waiting'&&attempt.authorization_url?<div className="github-qr-card">
          <p>请在浏览器中确认授权给 Velella，并选择要连接的工作区。</p>
          <Button onClick={()=>void openSourceUrl(attempt.authorization_url!).catch(()=>setError('无法打开浏览器，请重试。'))}><ExternalLink size={15}/>重新打开浏览器</Button>
          <p className="source-muted" role="status">等待浏览器授权，10 分钟内有效…</p>
        </div>:qrReady?<div className="github-qr-card">
          <img className="github-qr" src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(attempt.qr_svg!)}`} alt="Linear 授权二维码"/>
          <p>在 Linear 确认授权给 Velella，并选择要连接的工作区。</p>
          <Button onClick={()=>void openSourceUrl(attempt.authorization_url!).catch(()=>setError('无法打开浏览器，请扫码继续。'))}><ExternalLink size={15}/>在浏览器打开</Button>
          <p className="source-muted" role="status">等待授权，二维码有效期为 10 分钟…</p>
        </div>:pending?<p className="source-muted" role="status">{attempt.state==='uncertain'?attempt.message:'正在准备授权…'}</p>:<>
          {attempt?.message&&<p className="source-muted" role="status">{attempt.message}</p>}
          <Button className="primary github-generate" onClick={()=>void begin()}>{mode==='qr'?<QrCode size={18}/>:<ExternalLink size={18}/ >}{mode==='qr'?(attempt?'重新生成二维码':'生成授权二维码'):(attempt?'重新授权':'在浏览器授权')}</Button>
        </>}
        {pending&&<div className="github-actions">{attempt?.state==='uncertain'&&<Button onClick={()=>void poll(attempt.request_id)}>检查授权结果</Button>}<Button disabled={stopping} onClick={()=>void stop()}>{stopping?'正在停止…':'停止等待'}</Button></div>}
      </>}
      {error&&<p className="source-error" role="alert">{error}</p>}
    </div>
    <div className="dialog-footer github-connect-footer"><Button disabled={pending||stopping} onClick={onManual}>使用 API Key 手动连接</Button><Button disabled={stopping} onClick={close}>{stopping?'正在停止…':'取消'}</Button></div>
  </NativeDialog>;
}
