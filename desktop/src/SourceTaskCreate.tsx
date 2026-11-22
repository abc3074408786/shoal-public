import {useEffect,useRef,useState} from 'react';
import {X} from 'lucide-react';
import {rpc} from './api';
import {Button,Input,Textarea,NativeDialog} from './ui';
import {TaskSource,SourceItem,providers,openSourceUrl,sourceHome} from './task-sources';

type FieldOption=string|{id?:string;value?:string;name?:string;children?:unknown[]};
type Field={key?:string;fieldId?:string;name:string;required:boolean;hasDefaultValue?:boolean;schema?:{type:string;items?:string};allowedValues?:FieldOption[]};
export default function SourceTaskCreate({source,onClose,onCreated}:{source:TaskSource;onClose:()=>void;onCreated:(item:SourceItem)=>void}){
  const dialog=useRef<HTMLDialogElement>(null),requestId=useRef(crypto.randomUUID()),inFlight=useRef(false),generation=useRef(0);
  const [title,setTitle]=useState(''),[description,setDescription]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState(''),[submitted,setSubmitted]=useState(false);
  const [scope,setScope]=useState(source.scope),[scopes,setScopes]=useState<{id:string;name:string}[]>([]);
  const [types,setTypes]=useState<{id:string;name:string;subtask?:boolean}[]>([]),[typeId,setTypeId]=useState(''),[fields,setFields]=useState<Field[]>([]),[extra,setExtra]=useState<Record<string,unknown>>({}),[loading,setLoading]=useState(source.provider==='jira'),[fieldsReady,setFieldsReady]=useState(false);
  useEffect(()=>{dialog.current?.showModal();let live=true;if(['linear','jira'].includes(source.provider))void rpc({cmd:'sources.scopes',source_id:source.id}).then(r=>{if(live)setScopes(r.choices||[])}).catch(e=>{if(live)setError(String(e))});return()=>{live=false}},[source.id]);
  useEffect(()=>{let live=true;generation.current++;setTypes([]);setTypeId('');setFields([]);setFieldsReady(false);if(source.provider==='jira'&&scope){setLoading(true);void rpc({cmd:'sources.create_options',source_id:source.id,scope}).then(r=>{if(live)setTypes(r.options.types)}).catch(e=>{if(live)setError(String(e))}).finally(()=>{if(live)setLoading(false)})}else setLoading(false);return()=>{live=false}},[scope,source.id]);
  async function chooseType(id:string){setTypeId(id);setFields([]);setExtra({});setFieldsReady(false);setLoading(true);setError('');const serial=++generation.current;try{const r=await rpc({cmd:'sources.create_options',source_id:source.id,scope,type_id:id});if(serial===generation.current){setFields(r.options.fields);setFieldsReady(!!id)}}catch(e){if(serial===generation.current)setError(String(e))}finally{if(serial===generation.current)setLoading(false)}}
  const required=fields.filter(f=>f.required&&!f.hasDefaultValue);
  const unsupported=required.filter(f=>!(f.allowedValues?.length&&!f.allowedValues.some(v=>typeof v!=='string'&&v.children?.length))&&!(['string','number','date','datetime'].includes(f.schema?.type||'')||(f.schema?.type==='array'&&f.schema.items==='string')));
  async function submit(e:React.FormEvent){e.preventDefault();if(inFlight.current)return;inFlight.current=true;setBusy(true);setError('');try{
    const querying=submitted;setSubmitted(true);
    const r=await rpc(querying?{cmd:'sources.creation_status',source_id:source.id,request_id:requestId.current}:{cmd:'sources.create',source_id:source.id,request_id:requestId.current,fields:{title,description,type_id:typeId,extra,scope}});
    onCreated(r.item);onClose();
  }catch(e){setError(String(e))}finally{inFlight.current=false;setBusy(false)}}
  return <NativeDialog ref={dialog} className="source-dialog" aria-labelledby="new-source-task" onClose={onClose} onCancel={e=>{if(busy)e.preventDefault()}}><form onSubmit={submit}>
    <div className="dialog-heading"><h2 id="new-source-task">在 {providers[source.provider]} 新建任务</h2><Button type="button" aria-label="关闭新建任务" disabled={busy} onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body"><p className="source-muted">{source.name} · {source.scope}</p>
      {['linear','jira'].includes(source.provider)&&<label>{source.provider==='linear'?'所属团队':'所属项目'}<select aria-label={source.provider==='linear'?'所属团队':'所属项目'} required disabled={busy||submitted} value={scope} onChange={e=>setScope(e.target.value)}><option value="">请选择</option>{source.scope&&!scopes.some(c=>c.id===source.scope)&&<option value={source.scope}>{source.scope}</option>}{scopes.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label>}
      <label>任务标题<Input autoFocus required maxLength={250} disabled={busy||submitted} value={title} onChange={e=>setTitle(e.target.value)}/></label>
      <label>需求与验收条件<Textarea rows={7} disabled={busy||submitted} value={description} onChange={e=>setDescription(e.target.value)} placeholder="要解决什么问题？完成后应当能做什么？"/></label>
      {source.provider==='jira'&&<label>事项类型<select aria-label="事项类型" required disabled={busy||submitted||loading} value={typeId} onChange={e=>void chooseType(e.target.value)}><option value="">请选择</option>{types.filter(t=>!t.subtask).map(t=><option key={t.id} value={t.id}>{t.name}</option>)}</select></label>}
      {required.filter(f=>!unsupported.includes(f)).map(f=>{
        const key=f.fieldId||f.key!,kind=f.schema?.type;
        const options=(f.allowedValues||[]).map(v=>typeof v==='string'?v:kind==='string'||kind==='array'&&f.schema?.items==='string'?v.value||v.name||'':v.id?{id:v.id}:{value:v.value||v.name});
        const selected=options.map((option,index)=>({index:String(index),chosen:(kind==='array'?Array.isArray(extra[key])?extra[key] as unknown[]:[]:[extra[key]]).some(value=>JSON.stringify(value)===JSON.stringify(option))})).filter(v=>v.chosen).map(v=>v.index);
        return <label key={key}>{f.name}{options.length?<select aria-label={f.name} required multiple={kind==='array'} disabled={busy||submitted} value={kind==='array'?selected:selected[0]??''} onChange={e=>{const values=Array.from(e.target.selectedOptions).filter(option=>option.value!=='').map(option=>options[Number(option.value)]);setExtra({...extra,[key]:kind==='array'?values:values[0]})}}>{kind!=='array'&&<option value="">请选择</option>}{f.allowedValues!.map((v,index)=><option key={index} value={index}>{typeof v==='string'?v:v.name||v.value}</option>)}</select>:<Input required disabled={busy||submitted} type={kind==='number'?'number':kind==='date'?'date':kind==='datetime'?'datetime-local':'text'} onChange={e=>setExtra({...extra,[key]:kind==='number'?Number(e.target.value):kind==='array'?e.target.value.split(',').map(s=>s.trim()).filter(Boolean):kind==='datetime'&&e.target.value?new Date(e.target.value).toISOString():e.target.value})}/>}</label>
      })}
      {unsupported.length>0&&<p role="alert">此 Jira 项目要求额外字段：{unsupported.map(f=>f.name).join('、')}。请在来源平台创建后刷新列表。</p>}
      {loading&&<p role="status">正在读取 Jira 必填字段…</p>}{error&&<p role="alert" className="source-error">{error}</p>}
      <p className="source-muted">提交会在来源平台创建真实任务。创建任务本身不会启动 Agent。</p>
    </div><div className="dialog-footer"><Button type="button" disabled={busy} onClick={()=>void openSourceUrl(sourceHome(source)).catch(e=>setError(String(e)))}>打开来源</Button><Button type="button" disabled={busy} onClick={onClose}>取消</Button><Button className="primary" disabled={busy||loading||!!unsupported.length||(source.provider==='jira'&&(!typeId||!fieldsReady))}>{busy?'正在提交…':submitted?'查询创建结果':'创建任务'}</Button></div>
  </form></NativeDialog>;
}
