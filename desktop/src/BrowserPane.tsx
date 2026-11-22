import {useEffect,useRef,useState} from 'react';
import {invoke} from './desktop-platform';
import {ArrowLeft,ArrowRight,Crosshair,Globe,MoreHorizontal,RotateCw,X} from 'lucide-react';
import {isTauri} from './api';
import {Button,Input,NativeDialog,Tip} from './ui';
import ActionMenu from './ActionMenu';
import BrowserAutomationPanel from './BrowserAutomationPanel';
import BrowserAnnotationPanel from './BrowserAnnotationPanel';
import {ensureBrowserPage,registerBrowserSurface,scheduleBrowserSurfaces} from './browser-surfaces';
import {currentTarget} from './execution-target';
import './browser-pane.css';

export default function BrowserPane({workspaceId,pageId,onCreated}:{workspaceId:string;pageId:string;onCreated:(page:{id:string;url:string})=>Promise<boolean>}){
  const [id,setId]=useState(pageId),[address,setAddress]=useState(''),[page,setPage]=useState<any>(null),[busy,setBusy]=useState(false),[error,setError]=useState(''),[tools,setTools]=useState(false),[phone,setPhone]=useState(false);
  const slot=useRef<HTMLDivElement>(null),dialog=useRef<HTMLDialogElement>(null),editing=useRef(false);
  const [history,setHistory]=useState(false);
  const [annotating,setAnnotating]=useState(false);
  const draft=id.startsWith('browser-draft-');
  const browser=(op:string,extra:Record<string,unknown>={})=>invoke<any>('browser_command',{request:{op,workspace_id:workspaceId,id,...extra},target:currentTarget()});
  useEffect(()=>{setId(pageId)},[pageId]);
  useEffect(()=>{if(tools||history)dialog.current?.showModal();else dialog.current?.close();scheduleBrowserSurfaces()},[tools,history]);
  useEffect(()=>{
    const handler=(event:Event)=>setError((event as CustomEvent<string>).detail);
    window.addEventListener('velella:browser-layout-error',handler);return()=>window.removeEventListener('velella:browser-layout-error',handler);
  },[]);
  useEffect(()=>{
    if(draft||!isTauri())return;
    let live=true,unregister=()=>{},timer:ReturnType<typeof setTimeout>;
    async function poll(){
      try{const r=await browser('list');const p=r.pages.find((p:any)=>p.id===id);if(live){setPage(p);if(p&&!editing.current)setAddress(p.url)}}catch{/* A user action owns the error display; polling may meet a busy controller. */}
      if(live)timer=setTimeout(poll,1000);
    }
    void ensureBrowserPage(`${workspaceId}:${id}`,async()=>{const r=await browser('list');if(!r.pages.some((p:any)=>p.id===id&&p.open))await browser('open',{reveal:false})}).then(()=>{if(live&&slot.current){unregister=registerBrowserSurface(id,workspaceId,slot.current);void poll()}}).catch(e=>{if(live)setError(String(e))});
    return()=>{live=false;clearTimeout(timer);unregister()};
  },[id,workspaceId]);
  async function act(op:string,extra:Record<string,unknown>={}){if(busy)return;setBusy(true);setError('');try{await browser(op,extra);scheduleBrowserSurfaces()}catch(e){setError(String(e))}finally{setBusy(false)}}
  async function navigate(){
    if(busy||!address.trim())return;
    let url=address.trim();if(!/^[a-z][a-z\d+.-]*:/i.test(url))url=(/^(localhost|127\.0\.0\.1|\[::1\])([:/]|$)/i.test(url)?'http://':'https://')+url;
    if(!draft){await act('navigate',{url});return}
    setBusy(true);setError('');
    try{const r=await browser('create',{url,reveal:false});setId(r.page.id);if(!(await onCreated(r.page)))setError('页面已保留，但标签保存失败。可从工作区工具重新打开。')}catch(e){setError(String(e))}finally{setBusy(false)}
  }
  return <div className="browser-pane" data-browser-page={id}>
    <div className="browser-toolbar">
      <Tip label="后退"><Button aria-label="网页后退" disabled={busy||draft} onClick={()=>void act('back')}><ArrowLeft size={14}/></Button></Tip>
      <Tip label="前进"><Button aria-label="网页前进" disabled={busy||draft} onClick={()=>void act('forward')}><ArrowRight size={14}/></Button></Tip>
      <Tip label="刷新网页"><Button aria-label="刷新网页" disabled={busy||draft} onClick={()=>void act('reload')}><RotateCw size={14}/></Button></Tip>
      <form className="browser-address" onSubmit={e=>{e.preventDefault();void navigate()}}><Globe size={14}/><Input aria-label="网页地址" placeholder="输入网址或 localhost:3000" value={address} onFocus={()=>editing.current=true} onBlur={()=>editing.current=false} onChange={e=>setAddress(e.target.value)}/><Button disabled={busy||!address.trim()||!isTauri()}>前往</Button></form>
      <Tip label="点选网页元素并交给 Agent 修改"><Button aria-label="点选修改网页" aria-pressed={annotating} disabled={draft||!page?.ready} onClick={()=>{setAnnotating(v=>!v);scheduleBrowserSurfaces()}}><Crosshair size={15}/></Button></Tip>
      <ActionMenu label="浏览器工具" icon={<MoreHorizontal size={15}/>} disabled={draft}>
        <Button onClick={()=>setTools(true)}>页面快照与 Agent</Button>
        <Button onClick={()=>void act('devtools')}>开发工具</Button>
        <Button onClick={()=>setHistory(true)}>访问历史</Button>
        <Button onClick={()=>{setPhone(!phone);requestAnimationFrame(scheduleBrowserSurfaces)}}>{phone?'恢复完整宽度':'手机宽度'}</Button>
      </ActionMenu>
    </div>
    {error&&<p role="alert" className="ide-error">{error}<Button aria-label="关闭浏览器提示" onClick={()=>setError('')}><X size={12}/></Button></p>}
    {annotating&&!draft&&<BrowserAnnotationPanel workspaceId={workspaceId} pageId={id} onClose={()=>setAnnotating(false)}/>}
    <div className="browser-content"><div ref={slot} className={`browser-native-slot${phone?' phone':''}`}>
      {draft?<p className="browser-placeholder">在地址栏输入网址，页面会显示在当前工作区。</p>:!isTauri()?<p className="browser-placeholder">请在鱼群桌面版打开此标签。</p>:<span className="browser-placeholder">{page?.ready?'':page?.open===false?'页面已关闭，可切换标签后重新打开。':'正在加载网页…'}</span>}
    </div></div>
    <NativeDialog ref={dialog} className="capabilities-dialog" aria-label={history?'访问历史':'页面快照与 Agent'} onClose={()=>{setTools(false);setHistory(false)}}><div className="dialog-heading"><h2>{history?'访问历史':'页面快照与 Agent'}</h2><Button aria-label="关闭页面工具" onClick={()=>{setTools(false);setHistory(false)}}><X size={16}/></Button></div><div className="dialog-body">{history?<ul className="capability-history">{[...(page?.history||[])].reverse().map((url:string,i:number)=><li key={i}><Button onClick={()=>{setHistory(false);void act('navigate',{url})}}>{url}</Button></li>)}</ul>:!draft&&<BrowserAutomationPanel workspaceId={workspaceId} pageId={id} open={!!page?.ready}/>}</div></NativeDialog>
  </div>;
}
