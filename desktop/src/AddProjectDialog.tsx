import {useEffect,useRef,useState} from 'react';
import {ArrowLeft,ChevronRight,FolderOpen,GitFork,Monitor,Plus,Server,X} from 'lucide-react';
import {invoke,open} from './desktop-platform';
import {currentTarget} from './execution-target';
import {isTauri,rpc} from './api';
import {Button,Input,NativeDialog} from './ui';
import './creation-dialog.css';

export default function AddProjectDialog({remote=false,hostName='',onClose,onCreate,onAdded}:{remote?:boolean;hostName?:string;onClose:()=>void;onCreate:()=>void;onAdded:(id:string)=>Promise<void>}){
  const dialog=useRef<HTMLDialogElement>(null),inflight=useRef(false),added=useRef<string|null>(null),requestId=useRef(crypto.randomUUID());
  const [mode,setMode]=useState<'choose'|'browse'|'clone'>(remote?'browse':'choose'),[busy,setBusy]=useState(false),[cancelling,setCancelling]=useState(false),[error,setError]=useState('');
  const [folder,setFolder]=useState(''),[url,setUrl]=useState(''),[parent,setParent]=useState(''),[name,setName]=useState(''),[customName,setCustomName]=useState(false),[retained,setRetained]=useState(''),[registeredRetry,setRegisteredRetry]=useState(false);
  const nativeClone=!!window.velellaDesktop,locked=!!added.current||registeredRetry;
  useEffect(()=>{dialog.current?.showModal()},[]);
  function changed(){requestId.current=crypto.randomUUID();setError('');setRetained('')}
  async function finish(id:string){added.current=id;await onAdded(id);onClose()}
  async function addFolder(path:string){if(inflight.current)return;inflight.current=true;setBusy(true);setError('');try{if(added.current)await finish(added.current);else{const r=await rpc({cmd:'project.add',path});await finish(r.project.id)}}catch(e){setError(String(e));setMode('browse')}finally{inflight.current=false;setBusy(false)}}
  async function browse(){if(remote){setMode('browse');return}if(isTauri()){try{const path=await open({directory:true,multiple:false,title:'选择项目文件夹'});if(typeof path==='string'){setFolder(path);await addFolder(path)}}catch(e){setError(String(e))}}else setMode('browse')}
  async function chooseParent(){try{const path=await open({directory:true,multiple:false,title:'选择仓库保存位置'});if(typeof path==='string'){changed();setParent(path)}}catch(e){setError(String(e))}}
  async function clone(){if(inflight.current||!nativeClone)return;inflight.current=true;setBusy(true);setError('');setCancelling(false);try{
    if(added.current){await finish(added.current);return}
    const r=await invoke<any>('project_clone',{request:{request_id:requestId.current,url:url.trim(),parent:parent.trim(),name:name.trim()},target:currentTarget()});
    if(!r.ok){setRetained(r.retained_path||'');setRegisteredRetry(!!r.retry_registration);throw Error(r.error||'克隆未完成')}
    setRegisteredRetry(true);setRetained(r.path||'');await finish(r.project.id);
  }catch(e){setError(String(e))}finally{inflight.current=false;setBusy(false);setCancelling(false)}}
  async function cancelClone(){setCancelling(true);try{await invoke('project_clone_cancel',{request_id:requestId.current})}catch(e){setError(String(e));setCancelling(false)}}
  const title=mode==='clone'?'从 URL 克隆':mode==='browse'?'浏览文件夹':'添加项目';
  return <NativeDialog ref={dialog} className="creation-dialog add-project-dialog" aria-labelledby="add-project-title" onClose={onClose} onCancel={e=>{if(busy)e.preventDefault()}}><form onSubmit={e=>{e.preventDefault();if(mode==='browse')void addFolder(folder.trim());else if(mode==='clone')void clone()}}>
    <div className="dialog-heading"><div className="creation-heading-copy"><h2 id="add-project-title">{title}</h2><p>{mode==='clone'?'将远程 Git 仓库克隆到本机并加入项目列表。':'把已有文件夹或新项目加入鱼群。'}</p></div><Button type="button" aria-label="关闭添加项目" disabled={busy} onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body">
      <div className="add-project-host"><span>主机</span><span>{remote?<Server size={14}/>:<Monitor size={14}/>}{remote?'远端执行主机 · '+hostName:navigator.platform.startsWith('Win')?'本机 Windows':'本机'}</span></div>
      {remote&&<p className="creation-hint">路径由远端主机按自己的规则校验；不会调用本机目录选择器，也不会把远端路径交给本机文件系统。Git 克隆请先在远端终端完成，再添加已有文件夹。</p>}
      {mode==='choose'?<div className="add-project-options"><Button autoFocus type="button" className="add-project-browse" disabled={busy} onClick={()=>void browse()}><FolderOpen size={21}/><span><strong>浏览文件夹</strong><small>添加本地文件夹或 Git 仓库</small></span><ChevronRight size={16}/></Button><span className="add-project-caption">其他添加方式</span><div className="add-project-alternatives">{!remote&&<Button type="button" onClick={()=>{setMode('clone');setError('')}}><GitFork size={19}/><span><strong>从 URL 克隆</strong><small>克隆远程 Git 仓库到本机</small></span><ChevronRight size={16}/></Button>}<Button type="button" onClick={onCreate}><Plus size={19}/><span><strong>创建新项目</strong><small>创建文件夹并初始化 Git</small></span><ChevronRight size={16}/></Button></div></div>:<fieldset className="creation-fields" disabled={busy||locked}>
        {mode==='browse'?<label>{remote?'远端项目文件夹':'项目文件夹'}<Input autoFocus required value={folder} onChange={e=>setFolder(e.target.value)} placeholder={remote?'/srv/projects/项目 或 D:\\Projects\\项目':'输入已有文件夹的完整路径'}/></label>:<>
          <label>Git 仓库地址<Input autoFocus required value={url} onChange={e=>{changed();setUrl(e.target.value);if(!customName)setName(e.target.value.trim().split(/[/:]/).pop()?.replace(/\.git$/,'')||'')}} placeholder="https://github.com/团队/项目.git"/><small className="creation-hint">支持 HTTPS 和 SSH，使用本机已有的 Git 登录配置。</small></label>
          <div className="creation-field"><label htmlFor="clone-project-parent">保存位置</label><div className="creation-control creation-location-input"><Input id="clone-project-parent" required value={parent} onChange={e=>{changed();setParent(e.target.value)}} placeholder="选择父文件夹"/>{isTauri()&&<Button type="button" aria-label="选择克隆保存位置" onClick={()=>void chooseParent()}><FolderOpen size={16}/></Button>}</div></div>
          <label>项目文件夹名称<Input required maxLength={80} value={name} onChange={e=>{changed();setCustomName(true);setName(e.target.value)}}/></label>
          {parent&&name&&<p className="creation-path">{parent.replace(/[\\/]$/,'')}/{name.trim()}</p>}
        </>}
      </fieldset>}
      {mode==='clone'&&!nativeClone&&<p className="creation-hint">请在鱼群桌面版中使用 Git 克隆。</p>}
      {busy&&<p className="creation-hint" role="status">{mode==='clone'?(cancelling?'正在取消克隆…':'正在克隆并添加项目，较大仓库可能需要几分钟…'):'正在添加项目…'}</p>}
      {error&&<p className="creation-error" role="alert">{error}</p>}
      {retained&&<p className="creation-hint">已保留目录：{retained}{registeredRetry?'。可以继续添加，无需再次克隆。':'。重新克隆请使用新的文件夹名称。'}</p>}
    </div>
    {mode!=='choose'&&<div className="dialog-footer">{remote?<Button type="button" disabled={busy} onClick={onClose}>取消</Button>:<Button type="button" disabled={busy||locked} onClick={()=>{setMode('choose');setError('')}}><ArrowLeft size={14}/>返回</Button>}{busy&&mode==='clone'?<Button type="button" disabled={cancelling} onClick={()=>void cancelClone()}>取消克隆</Button>:<Button className="primary" disabled={busy||mode==='clone'&&(!nativeClone||!url.trim()||!parent.trim()||!name.trim())||mode==='browse'&&!folder.trim()}>{locked?'继续添加项目':mode==='clone'?'克隆并添加':'添加项目'}</Button>}</div>}
  </form></NativeDialog>;
}
