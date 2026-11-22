import {useEffect,useId,useRef,useState,type FormEvent} from 'react';
import {ArrowRight,Check,FileText,GitBranch,Plus,RefreshCw,Search,X} from 'lucide-react';
import {Button,Input,Textarea} from '../../src/ui';
import type {EntityDefinition,ExtensionInfo,ExtensionRequest,OntologyAction,OntologyDefinitions,OntologyEntity} from '../../src/extensions/types';
import SemanticCheckPanel from './semantic-check';

type Editor={kind:'project'}|{kind:'entity';row?:OntologyEntity}|{kind:'relation'};
type Props={info:ExtensionInfo;request:ExtensionRequest;onBusyChange:(busy:boolean)=>void;onDirtyChange:(dirty:boolean)=>void};
const label=(row:OntologyEntity)=>row.fields.name||row.fields.title||row.id;
function Facts({value}:{value:unknown}){return <pre className="ontology-facts">{typeof value==='string'?value:JSON.stringify(value,null,2)}</pre>}

export default function OntologyPanel({info,request,onBusyChange,onDirtyChange}:Props){
  const [status,setStatus]=useState<{initialized:boolean;engineReady:boolean;engineError?:string}>(),[definitions,setDefinitions]=useState<OntologyDefinitions>();
  const [projects,setProjects]=useState<OntologyEntity[]>([]),[project,setProject]=useState(''),[role,setRole]=useState(''),[tab,setTab]=useState<'queue'|'entities'>('queue');
  const [entities,setEntities]=useState<OntologyEntity[]>([]),[actions,setActions]=useState<OntologyAction[]>([]),[search,setSearch]=useState('');
  const [selected,setSelected]=useState<OntologyEntity>(),[context,setContext]=useState<any>(),[neighbors,setNeighbors]=useState<any>();
  const [editor,setEditor]=useState<Editor>(),[busy,setBusy]=useState(false),[loading,setLoading]=useState(true),[detailLoading,setDetailLoading]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('');
  const [generation,setGeneration]=useState(0),[catalogGeneration,setCatalogGeneration]=useState(0);
  const [checkingView,setCheckingView]=useState(false);
  const selection=useRef(0),alive=useRef(true);
  const viewId=useId();
  useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
  useEffect(()=>{onBusyChange(busy)},[busy,onBusyChange]);
  useEffect(()=>{onDirtyChange(!!editor);return()=>onDirtyChange(false)},[editor,onDirtyChange]);
  useEffect(()=>{let live=true;setLoading(true);setError('');setSelected(undefined);setEntities([]);setActions([]);setProject('');
    void request<any>('describe').then(async value=>{
      if(!live)return;setStatus(value);if(!value.engineReady)return;
      const schema=await request<any>('schema');if(live)setDefinitions(schema.definitions);
      if(value.initialized){const list=await request<any>('projects');if(live){setProjects(list.projects);setProject(list.projects[0]?.id||'')}}
    }).catch(error=>{if(live)setError(String(error))}).finally(()=>{if(live)setLoading(false)});
    return()=>{live=false};
  },[request,info.dataDir,catalogGeneration]);
  useEffect(()=>{
    let live=true;selection.current++;setSelected(undefined);setContext(undefined);setNeighbors(undefined);setEditor(undefined);setSearch('');setError('');
    if(!project||!status?.initialized){setEntities([]);setActions([]);return}
    setLoading(true);setEntities([]);setActions([]);
    void Promise.all([request<any>('entities',{project}),request<any>('queue',{project,role})]).then(([rows,queue])=>{if(live){setEntities(rows.entities);setActions(queue.actions)}}).catch(error=>{if(live)setError(String(error))}).finally(()=>{if(live)setLoading(false)});
    return()=>{live=false};
  },[project,role,generation,request,status?.initialized]);
  async function openEntity(id:string,type:string){
    const token=++selection.current;setSelected(undefined);setContext(undefined);setNeighbors(undefined);setDetailLoading(true);setError('');
    try{const [value,related]=await Promise.all([request<any>(type==='task'?'context':'entity',{project,id}),request<any>('neighbors',{project,id})]);
      if(alive.current&&token===selection.current){setSelected(type==='task'?value.task:value);setContext(type==='task'?value:undefined);setNeighbors(related)}
    }catch(error){if(alive.current&&token===selection.current)setError(String(error))}finally{if(alive.current&&token===selection.current)setDetailLoading(false)}
  }
  async function write(operation:string,values:Record<string,unknown>={}){
    setBusy(true);setError('');setNotice('');
    try{const value=await request<any>(operation,values);if(!alive.current)return;setEditor(undefined);setNotice('已保存到本体库');
      if(operation==='initialize'||operation==='createProject')setCatalogGeneration(n=>n+1);else setGeneration(n=>n+1);
      return value;
    }catch(error){if(alive.current)setError(String(error));throw error}finally{if(alive.current)setBusy(false)}
  }
  const selectedDefinition=definitions?.entity_types.find(item=>item.name===selected?.type);
  const filteredEntities=entities.filter(row=>(label(row)+' '+row.type+' '+row.id).toLowerCase().includes(search.toLowerCase()));
  const filteredActions=actions.filter(row=>(row.subject_name+' '+row.detail).toLowerCase().includes(search.toLowerCase()));
  return <section className="ontology-panel" aria-label="开发本体工作台">
    <div className="ontology-toolbar"><label>本体项目<select aria-label="本体项目" disabled={busy||!!editor||!projects.length} value={project} onChange={event=>setProject(event.target.value)}>{!projects.length&&<option value="">尚无项目</option>}{projects.map(row=><option key={row.id} value={row.id}>{label(row)}</option>)}</select></label>
      <label>责任角色<select aria-label="责任角色" disabled={busy||!!editor||!status?.initialized} value={role} onChange={event=>setRole(event.target.value)}><option value="">全部角色</option>{definitions?.roles.map(item=><option key={item.id} value={item.id}>{item.name_cn}</option>)}</select></label>
      <div className="ontology-toolbar-actions"><Button disabled={busy||!!editor||!project||!status?.initialized} aria-pressed={checkingView} onClick={()=>setCheckingView(value=>!value)}>语义检查</Button><Button aria-label="刷新本体" disabled={busy||!!editor} onClick={()=>{setNotice('');setGeneration(n=>n+1)}}><RefreshCw size={15}/></Button><Button disabled={busy||!!editor||!status?.initialized} onClick={()=>{setCheckingView(false);setEditor({kind:'project'})}}><Plus size={15}/>新建项目</Button></div>
    </div>
    {error&&<div role="alert" className="extension-error">{error}{error.includes('revision_conflict')&&<span> 当前版本已经变化，请取消编辑并刷新后重新核对。</span>}</div>}
    {notice&&<p role="status" className="ontology-notice"><Check size={14}/>{notice}</p>}
    {!status?.engineReady&&!loading?<div className="ontology-empty"><h2>本体引擎暂不可用</h2><p>{status?.engineError||error}</p></div>:!status?.initialized&&!loading?<div className="ontology-empty"><GitBranch size={30}/><h2>开始记录项目的协作事实</h2><p>首次使用需要初始化独立本体库。不会修改现有项目数据库或启动 Agent。</p><code>{info.dataDir}</code><Button disabled={busy} onClick={()=>void write('initialize').catch(()=>{})}>初始化本体库</Button></div>:<>
    {checkingView&&project?<SemanticCheckPanel key={info.dataDir+':'+project+':'+generation} project={project} request={request} definitions={definitions} onBack={()=>setCheckingView(false)} onOpenEntity={row=>{setCheckingView(false);void openEntity(row.id,row.type)}}/>:<div className="ontology-body">
      <aside className="ontology-list"><div className="ontology-list-heading"><div className="ontology-tabs" role="tablist" aria-label="本体视图" onKeyDown={event=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;event.preventDefault();const next=event.key==='Home'?'queue':event.key==='End'?'entities':tab==='queue'?'entities':'queue';setTab(next);document.getElementById(viewId+'-'+next)?.focus()}}><Button id={viewId+'-queue'} role="tab" aria-controls={viewId+'-list'} tabIndex={tab==='queue'?0:-1} aria-selected={tab==='queue'} onClick={()=>setTab('queue')}>角色待办 <small>{actions.length}</small></Button><Button id={viewId+'-entities'} role="tab" aria-controls={viewId+'-list'} tabIndex={tab==='entities'?0:-1} aria-selected={tab==='entities'} onClick={()=>setTab('entities')}>全部实体 <small>{entities.length}</small></Button></div><label className="ontology-search"><Search size={15}/><Input aria-label="搜索本体" placeholder="搜索名称或任务" value={search} onChange={event=>setSearch(event.target.value)}/></label></div>
        <div id={viewId+'-list'} role="tabpanel" aria-labelledby={viewId+'-'+tab} className="ontology-list-items">{loading?<p role="status" className="ontology-inline-empty">正在读取项目…</p>:!project?<p className="ontology-inline-empty">先新建一个本体项目。</p>:tab==='queue'?filteredActions.length?filteredActions.map((action,index)=><Button className={'ontology-row '+(selected?.id===action.subject?'selected':'')} disabled={busy||!!editor} key={action.subject+action.kind+index} onClick={()=>void openEntity(action.subject,action.subject_snapshot.type)}><span className="ontology-row-title">{action.subject_name}<ArrowRight size={14}/></span><span>{action.detail}</span><small>{definitions?.roles.find(item=>item.id===action.role)?.name_cn||action.role} · {action.subject_snapshot.state} · v{action.subject_snapshot.revision}</small></Button>):<p className="ontology-inline-empty">当前角色没有待办。可切换角色或查看全部实体。</p>:filteredEntities.length?filteredEntities.map(row=><Button className={'ontology-row '+(selected?.id===row.id?'selected':'')} disabled={busy||!!editor} key={row.id} onClick={()=>void openEntity(row.id,row.type)}><span className="ontology-row-title">{label(row)}<ArrowRight size={14}/></span><small>{definitions?.entity_types.find(item=>item.name===row.type)?.name_cn||row.type} · {row.state} · v{row.revision}</small></Button>):<p className="ontology-inline-empty">没有匹配的实体。</p>}</div>
        <div className="ontology-list-footer"><Button disabled={busy||!!editor||!project} onClick={()=>setEditor({kind:'entity'})}><Plus size={15}/>新建实体</Button><Button disabled={busy||!!editor||!project||entities.length<2} onClick={()=>setEditor({kind:'relation'})}><GitBranch size={15}/>关联</Button></div>
      </aside>
      <div className="ontology-detail" aria-label="本体详情">
        {editor&&definitions?<EditorForm key={editor.kind+(editor.kind==='entity'?editor.row?.id||'':'')} editor={editor} definitions={definitions} entities={entities} project={project} busy={busy} onCancel={()=>{setEditor(undefined);setError('')}} onSave={(operation,values)=>write(operation,values)}/>:detailLoading?<p role="status">正在读取完整上下文…</p>:selected?<>
          <header className="ontology-detail-heading"><div><span className="ontology-eyebrow">{selectedDefinition?.name_cn||selected.type} · {selected.state} · 版本 {selected.revision}</span><h2>{label(selected)}</h2><code>{selected.id}</code></div><Button disabled={busy} onClick={()=>setEditor({kind:'entity',row:selected})}>编辑实体</Button></header>
          {context?.requirement_detail&&<section className="ontology-requirement"><div className="ontology-section-label"><FileText size={16}/><h3>关联需求与验收标准</h3><small>版本 {context.requirement_detail.revision} · {context.requirement_detail.state}</small></div><h4>{label(context.requirement_detail)}</h4><p>{context.requirement_detail.fields.description||'未填写需求说明'}</p><Facts value={context.requirement_detail.fields.acceptance_criteria||'未填写验收标准'}/><small>来源：{context.requirement_detail.source||'未记录'} · 证据：{context.requirement_detail.evidence||'未记录'}</small></section>}
          {context&&!context.requirement_detail&&<p className="ontology-note">该任务尚未关联需求。</p>}
          <dl className="ontology-field-list">{Object.entries(selected.fields).map(([name,value])=><div key={name}><dt>{selectedDefinition?.fields.find(field=>field.name===name)?.description||name}</dt><dd>{value||'—'}</dd></div>)}<div><dt>来源</dt><dd>{selected.source||'未记录'}</dd></div><div><dt>证据</dt><dd>{selected.evidence||'未记录'}</dd></div></dl>
          {context&&<section className="ontology-related"><h3>交付产物与测试证据</h3>{[...new Map<string,{id:string;name:string;type:string;state:string}>((([...(context.artifacts||[]),...(context.evidence||[])]) as {id:string;name:string;type:string;state:string}[]).map(item=>[item.id,item])).values()].map(item=><Button key={item.id} disabled={busy} className="ontology-evidence-row" onClick={()=>void openEntity(item.id,item.type)}><FileText size={15}/><span>{item.name}</span><small>{item.state}</small><ArrowRight size={14}/></Button>)}{!context.artifacts?.length&&!context.evidence?.length&&<p>尚未登记产物或测试证据。</p>}</section>}
          <section className="ontology-related"><h3>关联关系</h3>{[...(neighbors?.outgoing||[]),...(neighbors?.incoming||[])].map((edge:any)=><div className="ontology-edge" key={edge.id}><span>{definitions?.relations.find(item=>item.kind===edge.kind)?.name_cn||edge.kind}</span><small>{entities.find(item=>item.id===(edge.from===selected.id?edge.to:edge.from))?label(entities.find(item=>item.id===(edge.from===selected.id?edge.to:edge.from))!):edge.from===selected.id?edge.to:edge.from}</small></div>)}{!neighbors?.outgoing?.length&&!neighbors?.incoming?.length&&<p>尚无关联关系。</p>}</section>
          {context?.next?.actions?.length>0&&<section className="ontology-related"><h3>交接建议</h3>{context.next.actions.map((action:OntologyAction,index:number)=><p key={index}>{action.detail}</p>)}</section>}
        </>:<div className="ontology-empty"><FileText size={28}/><h2>选择一项协作事实</h2><p>从角色待办开始，查看关联需求、验收依据和交付证据。</p></div>}
      </div>
    </div>}</>}
    <footer className="ontology-footer"><span title={info.dataDir}>{info.dataDir}</span><span>本机独立库 · {info.engine?.version} · 建议由你确认后执行</span></footer>
  </section>;
}

function EditorForm({editor,definitions,entities,project,busy,onCancel,onSave}:{editor:Editor;definitions:OntologyDefinitions;entities:OntologyEntity[];project:string;busy:boolean;onCancel:()=>void;onSave:(operation:string,values:Record<string,unknown>)=>Promise<unknown>}){
  const row=editor.kind==='entity'?editor.row:undefined;
  const [type,setType]=useState(row?.type||'requirement'),[fields,setFields]=useState<Record<string,string>>(row?.fields||{}),[state,setState]=useState(row?.state||''),[source,setSource]=useState(row?.source||'desktop:ontology'),[evidence,setEvidence]=useState(row?.evidence||'');
  const [id,setId]=useState(''),[name,setName]=useState(''),[description,setDescription]=useState(''),[relation,setRelation]=useState('task_for_requirement'),[from,setFrom]=useState(''),[to,setTo]=useState('');
  const definition=definitions.entity_types.find(item=>item.name===type)!,relationDefinition=definitions.relations.find(item=>item.kind===relation)!;
  const states=row?[row.state,...definition.transitions.filter(([previous])=>previous===row.state).map(([,next])=>next)]:[definition.initial_state];
  async function submit(event:FormEvent){event.preventDefault();try{
    if(editor.kind==='project'){await onSave('createProject',{id,name,description});return}
    if(editor.kind==='relation'){await onSave('link',{project,input:{kind:relation,from,to,source,evidence}});return}
    const input={...(row?{expected_revision:row.revision}:{type}),fields,state:state||definition.initial_state,source,evidence};
    await onSave('saveEntity',{project,...(row?{id:row.id}:{}),input});
  }catch{/* Parent preserves the draft and displays the exact engine error. */}}
  return <form className="ontology-editor" onSubmit={event=>void submit(event)}><header><div><span className="ontology-eyebrow">{row?`当前版本 ${row.revision} · 保存时校验版本`:'明确记录，不自动执行'}</span><h2>{editor.kind==='project'?'新建本体项目':editor.kind==='relation'?'建立关联':row?'编辑实体':'新建实体'}</h2></div><Button type="button" aria-label="取消本体编辑" disabled={busy} onClick={onCancel}><X size={17}/></Button></header><fieldset disabled={busy}>
    {editor.kind==='project'?<><label>项目ID<Input autoFocus required pattern="[a-zA-Z][a-zA-Z0-9._\-]*" maxLength={64} value={id} onChange={event=>setId(event.target.value)} placeholder="例如 orcas"/></label><label>项目名称<Input required value={name} onChange={event=>setName(event.target.value)}/></label><label>项目说明<Textarea rows={3} value={description} onChange={event=>setDescription(event.target.value)}/></label></>:editor.kind==='relation'?<>
      <label>关联类型<select value={relation} onChange={event=>{setRelation(event.target.value);setFrom('');setTo('')}}>{definitions.relations.map(item=><option value={item.kind} key={item.kind}>{item.name_cn}</option>)}</select></label>
      <label>起点实体<select required value={from} onChange={event=>setFrom(event.target.value)}><option value="">请选择</option>{entities.filter(item=>relationDefinition.from_types.includes(item.type)).map(item=><option key={item.id} value={item.id}>{label(item)}</option>)}</select></label>
      <label>终点实体<select required value={to} onChange={event=>setTo(event.target.value)}><option value="">请选择</option>{entities.filter(item=>relationDefinition.to_types.includes(item.type)).map(item=><option key={item.id} value={item.id}>{label(item)}</option>)}</select></label>
    </>:<>
      <label>实体类型<select disabled={!!row} value={type} onChange={event=>{setType(event.target.value);setFields({});setState('')}}>{definitions.entity_types.filter(item=>item.name!=='project'||row?.type==='project').map(item=><option key={item.name} value={item.name}>{item.name_cn}</option>)}</select></label>
      {definition.fields.map(field=><label key={field.name}>{field.description}{field.required?' *':''}{field.kind.kind==='enum'?<select name={field.name} required={field.required} value={fields[field.name]||''} onChange={event=>setFields(current=>({...current,[field.name]:event.target.value}))}><option value="" disabled={!!row?.fields[field.name]}>未设置</option>{field.kind.values?.map(value=><option key={value} value={value}>{value}</option>)}</select>:['title','name','priority','origin','kind','path','run_at','version','channel','role'].includes(field.name)?<Input name={field.name} required={field.required} value={fields[field.name]||''} onChange={event=>setFields(current=>({...current,[field.name]:event.target.value}))}/>:<Textarea name={field.name} required={field.required} rows={field.name==='acceptance_criteria'?5:3} value={fields[field.name]||''} onChange={event=>setFields(current=>({...current,[field.name]:event.target.value}))}/>}</label>)}
      <label>状态<select value={state||definition.initial_state} disabled={!row} onChange={event=>setState(event.target.value)}>{states.map(value=><option key={value} value={value}>{value}</option>)}</select></label>
    </>}
    {editor.kind!=='project'&&<><label>来源<Input value={source} onChange={event=>setSource(event.target.value)}/></label><label>证据说明<Textarea rows={2} value={evidence} onChange={event=>setEvidence(event.target.value)}/></label></>}
    </fieldset><footer><Button type="button" disabled={busy} onClick={onCancel}>取消</Button><Button type="submit" disabled={busy}>{busy?'正在保存…':'保存'}</Button></footer></form>;
}
