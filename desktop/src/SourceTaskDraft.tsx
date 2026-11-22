import {useEffect,useRef,useState} from 'react';
import {rpc} from './api';
import {currentTarget,namespacedKey} from './execution-target';
import {Button} from './ui';

type Draft={session_id:string;request_id:string;text:string;source:string;state:'waiting'|'sending'|'drafted'|'submitted'|'uncertain'|'manual';expires_at:number;auto_submit?:boolean;initial?:boolean;creation_id?:string;error?:string};
const legacyKey=(workspace:string)=>namespacedKey(currentTarget(),`velella:source-draft:${workspace}`);
const key=(workspace:string,session:string)=>`${legacyKey(workspace)}:${session}`;
const eventName='velella:prompt-draft-changed';
function read(storageKey:string):Draft|null{try{return JSON.parse(localStorage.getItem(storageKey)||'null')}catch{return null}}
function entries(workspace:string){
  const legacy=legacyKey(workspace),prefix=legacy+':';
  return Object.keys(localStorage).filter(k=>k===legacy||k.startsWith(prefix)).map(storageKey=>({storageKey,draft:read(storageKey)})).filter((entry):entry is {storageKey:string;draft:Draft}=>!!entry.draft);
}
export function saveSourceDraft(workspace:string,session:string,text:string,source:string,autoSubmit=false,creationId?:string){
  const storageKey=key(workspace,session),previous=read(storageKey);
  if(previous){
    if(previous.source!==source||previous.text!==text||!!previous.auto_submit!==autoSubmit||previous.creation_id!==creationId)throw Error('此会话已保存不同的初始提示词，请检查原草稿。');
    return;
  }
  localStorage.setItem(storageKey,JSON.stringify({session_id:session,request_id:crypto.randomUUID(),text,source,state:'waiting',expires_at:Date.now()+120000,auto_submit:autoSubmit,initial:true,creation_id:creationId}));
  window.dispatchEvent(new Event(eventName));
}
function receiptState(receipt:any):Draft['state']{
  if(receipt?.state==='drafted')return 'drafted';
  if(['submitted','turn_started'].includes(receipt?.state))return 'submitted';
  return 'uncertain';
}
function PromptDraft({workspace,storageKey,initial}:{workspace:string;storageKey:string;initial:Draft}){
  const manualInFlight=useRef(false);
  const [draft,setDraft]=useState(initial),[copied,setCopied]=useState(false);
  function save(value:Draft){localStorage.setItem(storageKey,JSON.stringify(value));setDraft(value)}
  async function pasteManually(){
    if(draft.state!=='manual'||draft.auto_submit||manualInFlight.current)return;
    manualInFlight.current=true;
    save({...draft,state:'sending'});
    try{
      const r=await rpc({cmd:'workbench.feedback.send',workspace_id:workspace,session_id:draft.session_id,request_id:draft.request_id,text:draft.text,source:draft.source,draft:true,initial:!!draft.initial,...(draft.initial?{expected_input_sequence:0}:{})});
      save({...draft,state:receiptState(r.receipt),error:r.receipt?.error});
    }catch(e){save({...draft,state:'uncertain',error:String(e)})}
    finally{manualInFlight.current=false}
  }
  useEffect(()=>{
    if(manualInFlight.current||['drafted','submitted','uncertain','manual'].includes(draft.state))return;
    let live=true,timer:ReturnType<typeof setTimeout>;
    function update(value:Draft){localStorage.setItem(storageKey,JSON.stringify(value));if(live)setDraft(value)}
    async function poll(){
      if(draft.auto_submit&&draft.creation_id){
        try{
          const r=await rpc({cmd:'workbench.creation.status',request_id:draft.creation_id});
          if(!live)return;
          const prompt=r.creation?.steps.find((step:any)=>step.kind==='prompt');
          const launch=r.creation?.steps.find((step:any)=>step.kind==='launch');
          if(prompt?.state==='succeeded'){update({...draft,state:'submitted'});return}
          if(!r.creation||['failed','uncertain','skipped'].includes(prompt?.state)||['failed','uncertain'].includes(launch?.state)){
            update({...draft,state:'uncertain',error:prompt?.error||launch?.error||'请检查原创建记录和会话；不会从界面重新投递。'});return;
          }
        }catch(e){if(live)update({...draft,error:String(e)})}
        if(live)timer=setTimeout(poll,1000);
        return;
      }
      try{
        if(draft.state==='sending'){
          const r=await rpc({cmd:'workbench.feedback.status',workspace_id:workspace,request_id:draft.request_id});
          if(!live)return;
          update({...draft,state:receiptState(r.receipt),error:r.receipt?.error||(!r.receipt?'提示词投递结果待核对，请检查原会话。':undefined)});return;
        }
        if(Date.now()>=draft.expires_at){update({...draft,state:'manual',error:'等待输入就绪已超时。提示词已保留，未自动提交。'});return}
        const r=await rpc({cmd:'workbench.sessions',workspace_id:workspace});if(!live)return;
        const session=r.sessions.find((s:any)=>s.id===draft.session_id);
        if(session?.status&&session.status.input_sequence!==0){update({...draft,state:'manual',error:'终端已有输入或不支持输入检查，请复制提示词后手动处理。'});return}
        if(session?.status?.state==='waiting_input'&&session.status.exit==null){
          // Persist before the request; a lost response must never queue a second submission.
          localStorage.setItem(storageKey,JSON.stringify({...draft,state:'sending'}));
          try{
            const result=await rpc({cmd:'workbench.feedback.send',workspace_id:workspace,session_id:draft.session_id,request_id:draft.request_id,text:draft.text,source:draft.source,draft:!draft.auto_submit,initial:!!draft.initial,expected_input_sequence:0});
            update({...draft,state:receiptState(result.receipt),error:result.receipt?.error});
          }catch(e){update({...draft,state:'uncertain',error:String(e)})}
          return;
        }
      }catch(e){if(Date.now()>=draft.expires_at&&live){update({...draft,state:'manual',error:String(e)});return}}
      if(live)timer=setTimeout(poll,1000);
    }
    void poll();return()=>{live=false;clearTimeout(timer)};
  },[workspace,storageKey,draft.state]);
  if(draft.state==='drafted')return null;
  return <div className="source-workspace-links" role="status"><span>{draft.state==='submitted'?'初始提示词已被终端接收；是否开始执行以原会话为准。':draft.state==='uncertain'?'提示词投递待核对，未自动重发。':draft.state==='manual'?'提示词已保留。':`提示词等待输入就绪${draft.auto_submit?'后提交':'后填入草稿'}。请先在终端完成登录或授权。`}</span>{draft.state==='manual'&&!draft.auto_submit&&!draft.initial&&<Button title="粘贴到原 Agent 输入框，不自动提交" onClick={()=>void pasteManually()}>粘贴提示词草稿</Button>}<Button onClick={()=>void navigator.clipboard.writeText(draft.text).then(()=>setCopied(true))}>{copied?'已复制':'复制提示词'}</Button>{draft.error&&<span>{draft.error}</span>}</div>;
}
export default function SourceTaskDraft({workspace}:{workspace:string}){
  const [drafts,setDrafts]=useState(()=>entries(workspace));
  useEffect(()=>{
    const refresh=()=>setDrafts(entries(workspace));
    refresh();window.addEventListener(eventName,refresh);window.addEventListener('storage',refresh);
    return()=>{window.removeEventListener(eventName,refresh);window.removeEventListener('storage',refresh)};
  },[workspace]);
  useEffect(()=>{
    if(!/^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(workspace))return;
    let live=true,timer:ReturnType<typeof setTimeout>;
    async function recover(){
      try{
        const r=await rpc({cmd:'workbench.creation.status',request_id:workspace});
        if(!live)return;
        const creation=r.creation,prompt=creation?.request?.initial_prompt;
        if(!creation||creation.request_id!==workspace||!prompt)return;
        const launch=creation.steps.find((step:any)=>step.kind==='launch');
        if(launch?.state==='succeeded'&&launch.descriptor){
          if(!read(key(workspace,launch.descriptor.id)))saveSourceDraft(workspace,launch.descriptor.id,prompt.text,creation.request.source_key||'initial:'+workspace,prompt.mode==='submit',workspace);
          return;
        }
        if(['failed','uncertain'].includes(launch?.state))return;
      }catch{}
      if(live)timer=setTimeout(recover,1000);
    }
    void recover();return()=>{live=false;clearTimeout(timer)};
  },[workspace]);
  return <>{drafts.filter(({storageKey})=>storageKey===legacyKey(workspace)||storageKey.startsWith(legacyKey(workspace)+':')).map(({storageKey,draft})=><PromptDraft key={storageKey} workspace={workspace} storageKey={storageKey} initial={draft}/>)}</>;
}
