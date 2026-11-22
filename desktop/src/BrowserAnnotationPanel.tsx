import {useEffect,useRef,useState} from 'react';
import {invoke} from './desktop-platform';
import {currentTarget} from './execution-target';
import {Crosshair,RotateCcw,Send,X} from 'lucide-react';
import {Button,Input,Textarea} from './ui';
import {rpc} from './api';
import {scheduleBrowserSurfaces} from './browser-surfaces';
import {addAnnotation,assignAnnotations,editAnnotation,AnnotationSelection as Selection} from './annotation-drafts';
import {useAnnotations} from './useAnnotations';
import AnnotationCards from './AnnotationCards';
import './annotation-chat.css';

type Target={id:string;title:string;program:string;native_chat?:boolean;status:{exit:number|null;state:string}|null};
type Receipt={request_id:string;session_id:string;state:string;error?:string};
const numericFields=[['font-size','字号',8],['padding','内边距',0],['margin','外边距',0],['border-radius','圆角',0]] as const;
const uniformPixels=(value:string='')=>/^\d+(\.\d+)?px$/.test(value)?value.slice(0,-2):'';
const hexColor=(value:string='')=>{const rgb=value.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)/);return rgb?'#'+rgb.slice(1,4).map(n=>Number(n).toString(16).padStart(2,'0')).join(''):'#000000';};

export default function BrowserAnnotationPanel({workspaceId,pageId,onClose}:{workspaceId:string;pageId:string;onClose:()=>void}){
  const [selection,setSelection]=useState<Selection|null>(null),[patch,setPatch]=useState<Record<string,string>>({}),[comment,setComment]=useState('');
  const [targets,setTargets]=useState<Target[]>([]),[target,setTarget]=useState(''),[picking,setPicking]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState('');
  const pendingKey=`velella:annotation-pending:${workspaceId}:${pageId}`;
  const [receipt,setReceipt]=useState<Receipt|null>(null),[requestId,setRequestId]=useState(()=>localStorage.getItem(pendingKey)||'');
  const [numericDraft,setNumericDraft]=useState<Record<string,string>>({});
  const [receiptChecked,setReceiptChecked]=useState(false);
  const {state:annotations,error:draftError}=useAnnotations(workspaceId);
  const saved=annotations.items.filter(item=>!item.sessionId);
  useEffect(()=>{setRequestId(localStorage.getItem(pendingKey)||'');setReceipt(null);setReceiptChecked(false)},[pendingKey]);
  const knownSelection=useRef(''),inFlight=useRef(false);
  const browser=async(op:string,extra:Record<string,unknown>={})=>{
    const result=await invoke<any>('browser_command',{request:{op,workspace_id:workspaceId,id:pageId,...extra},target:currentTarget()});
    if(result.ok===false)throw Error(result.error||'操作未完成');return result;
  };
  function adopt(result:any){
    setPicking(!!result.picking);setSelection(result.selection??null);
    if(knownSelection.current!==(result.selection?.selection_id||'')){
      knownSelection.current=result.selection?.selection_id||'';setPatch({});setNumericDraft({});
    }
    if(result.notice)setMessage(result.notice);
  }
  useEffect(()=>{
    let live=true,timer:ReturnType<typeof setTimeout>;
    async function poll(){
      if(!inFlight.current){try{const r=await browser('annotation_status');if(live)adopt(r)}catch(e){if(live&&!String(e).includes('browser_busy'))setError(String(e));}}
      if(live)timer=setTimeout(poll,600);
    }
    void poll();scheduleBrowserSurfaces();
    return()=>{live=false;clearTimeout(timer);void browser('annotation_cancel').catch(()=>{});scheduleBrowserSurfaces();};
  },[pageId,workspaceId]);
  useEffect(()=>{
    let live=true;
    void rpc<any>({cmd:'workbench.sessions',workspace_id:workspaceId}).then(r=>{
      if(!live)return;const eligible=r.sessions.filter((s:Target)=>!s.native_chat&&['claude','codex'].includes(s.program)&&s.status&&s.status.exit===null);
      setTargets(eligible);if(eligible.length===1)setTarget(eligible[0].id);
    }).catch(e=>{if(live)setError(String(e))});
    return()=>{live=false};
  },[workspaceId,selection?.selection_id]);
  async function run(fn:()=>Promise<void>){if(inFlight.current)return;inFlight.current=true;setBusy(true);setError('');try{await fn()}catch(e){setError(String(e))}finally{inFlight.current=false;setBusy(false);scheduleBrowserSurfaces()}}
  const locked=!!requestId;
  function change(key:string,value:string){setPatch(p=>({...p,[key]:value}));setMessage('调整尚未预览');}
  const pending=locked&&receipt?.state!=='submitted';
  async function pick(mode='element'){if(pending)return;await run(async()=>{setRequestId('');setReceipt(null);setReceiptChecked(false);setMessage(mode==='region'?'在网页拖选一个区域；Esc 取消。':'点击网页中的元素；Esc 取消选取。');adopt(await browser('annotation_start',{mode}))})}
  function changeNumber(key:string,value:string){
    setNumericDraft(p=>({...p,[key]:value}));
    if(value===''){setPatch(p=>{const next={...p};delete next[key];return next});setMessage('该项保留原值；应用预览后生效。');}
    else change(key,value+'px');
  }
  async function preview(){if(!selection)return;await run(async()=>{adopt(await browser('annotation_preview',{selection_id:selection.selection_id,patch}));setMessage('正在预览，尚未修改项目文件。')})}
  async function reset(){if(!selection)return;await run(async()=>{adopt(await browser('annotation_reset',{selection_id:selection.selection_id}));setPatch({});setNumericDraft({});setMessage('已恢复网页原样。')})}
  function showReceipt(value:Receipt|null){
    setReceipt(value);
    if(value?.state==='submitted'){localStorage.removeItem(pendingKey);setMessage('已提交到所选 Agent。请在原会话查看接收结果、权限提示和代码改动。');setSelection(null);setPatch({});}
    else if(value)setMessage('发送结果尚不确定，请查看 Agent 原会话。此请求不会自动重发。');
    else setMessage('尚未找到回执；请先检查 Agent 原会话，避免重复发送。');
  }
  async function send(){
    if(!selection||!target||locked)return;
    await run(async()=>{
      // Apply the latest edited fields, then backend preparation restores the
      // page before it sends the source-change request to the selected Agent.
      await browser('annotation_preview',{selection_id:selection.selection_id,patch});
      const id=crypto.randomUUID();localStorage.setItem(pendingKey,id);setRequestId(id);
      try{const r=await browser('annotation_send',{selection_id:selection.selection_id,session_id:target,comment,request_id:id});showReceipt(r.receipt)}
      catch(e){setMessage('预览已撤销或发送状态未知。请查询回执，并检查原 Agent 会话。');throw e;}
    });
  }
  async function inspectReceipt(){await run(async()=>{const r=await rpc<any>({cmd:'workbench.feedback.status',workspace_id:workspaceId,request_id:requestId});setReceiptChecked(true);showReceipt(r.receipt)})}
  async function saveComment(){if(!selection||locked)return;await run(async()=>{
    if(!comment.trim()&&!Object.keys(patch).length)throw Error('请填写批注或调整一个属性。');
    await browser('annotation_preview',{selection_id:selection.selection_id,patch});
    const prepared=await browser('annotation_prepare',{selection_id:selection.selection_id});
    addAnnotation(workspaceId,pageId,prepared.selection,comment);
    adopt(await browser('annotation_cancel'));setComment('');setMessage('批注已保存，预览已撤销。可以继续选取，再一起带入聊天。');
  })}
  async function openChat(){if(!target||!saved.length)return;await run(async()=>{
    assignAnnotations(workspaceId,saved.map(item=>item.id),target);
    await browser('annotation_cancel');
    window.dispatchEvent(new CustomEvent('velella:annotation-chat',{detail:{workspaceId,sessionId:target}}));
    setMessage('批注已加入该会话的输入框，尚未发送。');
  })}
  async function close(){await run(async()=>{try{await browser('annotation_cancel')}finally{onClose()}})}
  return <section className="browser-annotation-panel" aria-label="点选修改">
    <div className="browser-annotation-heading"><strong>点选修改</strong><span>预览后交给 Agent 落实到代码</span><Button type="button" aria-label="关闭点选修改" disabled={busy} onClick={()=>void close()}><X size={15}/></Button></div>
    <div className="browser-annotation-actions"><Button type="button" disabled={busy||pending} aria-pressed={picking} onClick={()=>void pick()}><Crosshair size={14}/>{picking?'正在选取…':selection?'重新选取':'选取网页元素'}</Button><Button type="button" disabled={busy||pending} onClick={()=>void pick('region')}>拖选区域</Button>{selection&&<code title={selection.selector}>{selection.tag} · {selection.text.slice(0,60)||selection.selector}</code>}</div>
    {!selection&&!receipt&&<p className="browser-annotation-note">选取主页面中的文字、按钮或区域。表单输入和 iframe 暂不参与修改。</p>}
    {selection&&<>
      {selection.kind!=='region'&&<><div className="browser-annotation-fields">
        <label className="browser-annotation-text">文字<Input aria-label="预览文字" disabled={busy||locked||!selection.editable_text} value={patch.text??selection.text} maxLength={2000} onChange={e=>change('text',e.target.value)}/>{!selection.editable_text&&<small>区域内含子元素，请选取具体文字。</small>}</label>
        <label>字体<select aria-label="预览字体" disabled={busy||locked} value={patch['font-family']||''} onChange={e=>change('font-family',e.target.value)}><option value="" disabled>保留原字体</option><option value="system-ui">系统字体</option><option value="sans-serif">无衬线</option><option value="serif">衬线</option><option value="monospace">等宽</option></select></label>
        {numericFields.map(([key,label,min])=><label key={key}>{label}（px）<Input aria-label={`预览${label}`} type="number" min={min} max={128} step="1" disabled={busy||locked} placeholder="保留原值" value={numericDraft[key]??uniformPixels(patch[key]??selection.styles[key])} onChange={e=>changeNumber(key,e.target.value)}/></label>)}
        {(['color','background-color'] as const).map((key,i)=><label key={key}>{i?'背景颜色':'文字颜色'}<Input aria-label={i?'预览背景颜色':'预览文字颜色'} type="color" disabled={busy||locked} value={patch[key]??hexColor(selection.styles[key])} onChange={e=>change(key,e.target.value)}/></label>)}
      </div>
      <div className="browser-annotation-actions"><Button type="button" disabled={busy||locked||!Object.keys(patch).length} onClick={()=>void preview()}>应用预览</Button><Button type="button" disabled={busy||locked} onClick={()=>void reset()}><RotateCcw size={14}/>撤销预览</Button></div>
      </>}
      {selection.kind==='region'&&<p className="browser-annotation-note">已选区域 {Math.round(selection.bounds.width)} × {Math.round(selection.bounds.height)}，包含 {selection.elements?.length||0} 个定位元素。样式调整请点选具体元素。</p>}
      <label>修改要求<Textarea aria-label="网页修改要求" rows={2} maxLength={4000} value={comment} disabled={busy||locked} onChange={e=>setComment(e.target.value)} placeholder="例如：按钮文字保持单行，增大字号并保留原来的点击行为。"/></label>
      <div className="browser-annotation-actions"><Button type="button" disabled={busy||locked||!!draftError||(!comment.trim()&&!Object.keys(patch).length)} onClick={()=>void saveComment()}>保存批注</Button><Button type="button" disabled={busy||locked} onClick={()=>void run(async()=>{adopt(await browser('annotation_cancel'));setComment('')})}>取消本次批注</Button></div>
      <div className="browser-annotation-delivery"><label>发送到<select aria-label="接收修改要求的 Agent" disabled={busy||locked} value={target} onChange={e=>setTarget(e.target.value)}><option value="">{targets.length?'选择当前工作区的 Agent':'请先在此工作区打开 Claude 或 Codex'}</option>{targets.map(s=><option value={s.id} key={s.id}>{s.title} · {s.id.slice(-6)}</option>)}</select></label><Button type="button" disabled={busy||locked||!target||(!comment.trim()&&!Object.keys(patch).length)} onClick={()=>void send()}><Send size={14}/>发送给 Agent</Button></div>
      <p className="browser-annotation-note">请先完成 Agent 的登录与信任确认。发送前撤销网页预览；项目文件由 Agent 修改，权限提示仍在原会话确认。</p>
    </>}
    {saved.length>0&&<section aria-label="已保存的网页批注"><h3>待加入聊天的批注 · {saved.length}</h3><AnnotationCards items={saved} workspaceId={workspaceId} disabled={busy} onEdit={(id,text)=>void run(async()=>{editAnnotation(workspaceId,id,text)})} onRemove={id=>void run(async()=>{editAnnotation(workspaceId,id,null)})}/>
      <div className="browser-annotation-delivery"><label>加入会话<select aria-label="接收批注的聊天" value={target} disabled={busy} onChange={e=>setTarget(e.target.value)}><option value="">选择当前工作区的 Agent</option>{targets.map(s=><option key={s.id} value={s.id}>{s.title} · {s.id.slice(-6)}</option>)}</select></label><Button type="button" disabled={busy||!target||!!selection||pending||!!draftError} onClick={()=>void openChat()}>带入聊天（{saved.length}）</Button></div>
      {selection&&<p className="browser-annotation-note">先保存或取消当前批注，再一起带入聊天。</p>}
    </section>}
    {requestId&&receipt?.state!=='submitted'&&<Button type="button" disabled={busy} onClick={()=>void inspectReceipt()}>查询发送回执</Button>}
    {pending&&<p className="browser-annotation-note">有一条发送结果待核对。请先查询回执并检查原 Agent 会话，确认后再开始新修改。</p>}
    {pending&&(receipt?.state==='uncertain'||receiptChecked)&&<Button type="button" disabled={busy} onClick={()=>{localStorage.removeItem(pendingKey);setRequestId('');setReceipt(null);setReceiptChecked(false);setMessage('已确认原会话状态，可以重新选取元素。')}}>已核对原会话，开始新修改</Button>}
    {message&&<p className="browser-annotation-note" role="status">{message}</p>}
    {error&&<p role="alert" className="ide-error">{error}</p>}
    {draftError&&<p role="alert" className="ide-error">{draftError}</p>}
  </section>;
}
