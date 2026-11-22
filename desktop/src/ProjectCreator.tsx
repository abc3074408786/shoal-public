import {useEffect,useRef,useState} from 'react';
import {open} from './desktop-platform';
import {isTauri,rpc} from './api';
import {Button,Input,NativeDialog} from './ui';
import {Folder,FolderOpen,X} from 'lucide-react';
import './creation-dialog.css';

export default function ProjectCreator({remote=false,hostName='',onClose,onCreated}:{remote?:boolean;hostName?:string;onClose:()=>void;onCreated:(id:string)=>Promise<void>}){
  const dialog=useRef<HTMLDialogElement>(null),created=useRef<string|null>(null),submitting=useRef(false);
  const [name,setName]=useState(''),[parent,setParent]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState('');
  useEffect(()=>{dialog.current?.showModal()},[]);
  async function browse(){if(remote)return;try{const path=await open({directory:true,multiple:false,title:'选择项目保存位置'});if(typeof path==='string')setParent(path)}catch(e){setError(String(e))}}
  async function submit(){if(submitting.current)return;submitting.current=true;setBusy(true);setError('');try{
    if(!created.current){const r=await rpc({cmd:'project.create',parent,name});created.current=r.project.id}
    await onCreated(created.current!);onClose();
  }catch(e){setError(String(e))}finally{submitting.current=false;setBusy(false)}}
  return <NativeDialog ref={dialog} className="workspace-composer creation-dialog" aria-labelledby="create-project-title" onClose={onClose} onCancel={e=>{if(busy)e.preventDefault()}}><form onSubmit={e=>{e.preventDefault();void submit()}} onKeyDown={e=>{if((e.ctrlKey||e.metaKey)&&e.key==='Enter'&&!e.nativeEvent.isComposing){e.preventDefault();e.currentTarget.requestSubmit()}}}>
    <div className="dialog-heading"><div className="creation-heading-copy"><h2 id="create-project-title">创建项目文件夹</h2><p>{remote?'在远端执行主机 '+hostName+' 上创建文件夹并初始化 Git；路径由该主机校验。':'创建文件夹并初始化 Git，之后可添加独立工作树。'}</p></div><Button type="button" aria-label="关闭创建项目" disabled={busy} onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body"><fieldset className="creation-fields" disabled={busy||!!created.current}><label className="creation-field"><span className="creation-label">项目名称</span><span className="creation-search"><Folder size={15}/><Input autoFocus required maxLength={80} value={name} onChange={e=>setName(e.target.value)} placeholder="我的项目"/></span></label>
    <label className="creation-field"><span className="creation-label">保存位置</span><span className="creation-control creation-location-input"><Input required value={parent} onChange={e=>setParent(e.target.value)} placeholder={remote?'/srv/projects 或 D:\\Projects':'选择父文件夹'}/>{isTauri()&&!remote&&<Button type="button" aria-label="选择位置" onClick={browse}><FolderOpen size={16}/></Button>}</span></label>
    {parent&&name&&<p className="creation-path">{parent.replace(/[\\/]$/,'')}/{name.trim()}</p>}
    </fieldset>
    {error&&<p role="alert" className="creation-error">{error}</p>}</div>
    <div className="dialog-footer"><Button type="button" disabled={busy} onClick={onClose}>取消</Button><Button className="primary" disabled={busy||!name.trim()||!parent.trim()}>{busy?'正在创建…':created.current?'继续打开':'创建项目'}<kbd aria-hidden="true">Ctrl ↵</kbd></Button></div>
  </form></NativeDialog>;
}
