import {useConfirm} from './ConfirmAction';
import {useEffect,useRef,useState} from 'react';
import Editor from './LazyEditor';
import {useAppearance} from './Appearance';
import {Button,Input,NativeDialog} from './ui';
import ActionMenu from './ActionMenu';
import {MoreHorizontal,FileText,BookOpen,Code2,Check,Circle,RotateCw,Save} from 'lucide-react';
import {rpc,Task} from './api';
import {draftKey} from './workbench-types';
import {fileEditorOptions} from './editor-appearance';
import './editor-a.css';
import DocumentReader from './DocumentReader';
import LoadingState from './LoadingState';
import './document-workbench.css';
import {useWorkbenchPreferences} from './useWorkbenchPreferences';
import {shortcutMatches} from './workbench-preferences';
import {useExecutionTarget} from './ExecutionHostsPanel';
import {currentTarget,sameTarget} from './execution-target';

const assetExtensions=['png','jpg','jpeg','gif','webp','svg','pdf'];
const markdownExtensions=['md','markdown','mdown'];

export default function WorkbenchFile({workspaceId,path,task}:{workspaceId:string;path:string;task?:Task}) {
  const ask=useConfirm();
  const preferences=useWorkbenchPreferences(),target=useExecutionTarget(),saveLock=useRef(false);
  const {theme}=useAppearance();
  const extension=path.split('.').at(-1)?.toLowerCase()||'';
  const asset=assetExtensions.includes(extension),markdown=markdownExtensions.includes(extension);
  const [value,setValue]=useState<{content:string;previous:string}|null>(null),[assetUrl,setAssetUrl]=useState(''),[error,setError]=useState(''),[saving,setSaving]=useState(false),[preview,setPreview]=useState(asset||markdown);
  const key=draftKey(workspaceId,path);
  useEffect(()=>{let live=true;setValue(null);setAssetUrl('');setError('');setPreview(asset||markdown);
    async function load(){try{if(asset){const r=await rpc({cmd:'workbench.file.asset',workspace_id:workspaceId,path});if(live)setAssetUrl(`data:${r.mime};base64,${r.data}`);return}
      const r=await rpc(task?{cmd:'file.read',task_id:task.id,path}:{cmd:'workbench.file.read',workspace_id:workspaceId,path});if(!live)return;
      const raw=localStorage.getItem(key),draft=raw?JSON.parse(raw):null;
      if(draft && typeof draft.content==='string' && typeof draft.previous==='string'){setValue(draft);if(draft.previous!==r.content)setError('磁盘文件已变化，草稿已保留。保存会检查冲突，请先核对。');}
      else setValue({content:r.content,previous:r.content});
    }catch(e){if(live)setError(String(e));}}
    void load();return()=>{live=false};
  },[key,workspaceId,path,asset,markdown,task?.id]);
  const dirty=value!==null&&value.content!==value.previous;
  async function save(){if(!value||saveLock.current||!sameTarget(target,currentTarget()))return;saveLock.current=true;setSaving(true);try{await rpc(task?{cmd:'file.write',task_id:task.id,path,...value}:{cmd:'workbench.file.write',workspace_id:workspaceId,path,...value},target);localStorage.removeItem(key);setValue({content:value.content,previous:value.content});setError('')}catch(e){setError(String(e))}finally{saveLock.current=false;setSaving(false)}}
  useEffect(()=>{const keydown=(e:KeyboardEvent)=>{if(!e.isComposing&&shortcutMatches('saveFile',e,preferences)&&!document.querySelector('dialog[open],.app-settings-page')&&document.activeElement?.closest('.wb-editor')===document.querySelector(`[data-editor="${CSS.escape(key)}"]`)){e.preventDefault();if(dirty&&!saving)void save()}};window.addEventListener('keydown',keydown);return()=>window.removeEventListener('keydown',keydown)},[value,saving,preferences,target]);
  useEffect(()=>{if(!preferences.editor.autoSave||!dirty||saving||error||asset)return;const timer=setTimeout(()=>void save(),preferences.editor.autoSaveDelay);return()=>clearTimeout(timer)},[value,saving,error,key,target,preferences.editor.autoSave,preferences.editor.autoSaveDelay]);
  async function reload(){if(saveLock.current)return;if(dirty&&!(await ask('重新载入磁盘版本并放弃此文件的草稿？')))return;try{const r=await rpc(task?{cmd:'file.read',task_id:task.id,path}:{cmd:'workbench.file.read',workspace_id:workspaceId,path},target);localStorage.removeItem(key);setValue({content:r.content,previous:r.content});setError('')}catch(e){setError(String(e))}}
  return <div className={'wb-editor '+(markdown?'document-workbench '+(preview?'is-reading':'is-source'):'')} data-editor={key}>
    {markdown?<div className="document-toolbar"><div className="document-mode" role="group" aria-label="文档显示模式"><Button aria-pressed={preview} disabled={!value} onClick={()=>setPreview(true)}><BookOpen size={14}/>阅读</Button><Button aria-pressed={!preview} disabled={!value} onClick={()=>setPreview(false)}><Code2 size={14}/>源码</Button></div><div className="document-actions"><span className={'document-save-state '+(dirty?'is-dirty':'')} role="status">{dirty?<Circle size={7}/>:<Check size={13}/>}<span>{saving?'正在保存':!value?'正在读取':dirty?'本机草稿':'已保存'}</span></span>{(!preview||dirty)&&<Button className="file-save" disabled={!dirty||saving} onClick={save}><Save size={14}/>保存</Button>}<Button aria-label="重新载入" title="重新载入" onClick={reload}><RotateCw size={14}/></Button>{!task&&<FileActions workspaceId={workspaceId} path={path}/>}</div></div>:<div className="panel-toolbar file-editor-toolbar"><div className="file-editor-heading"><FileText size={18}/><div><strong title={path}>{path}</strong><span role="status">{asset?'文件预览':'代码与文本'}{!asset&&' · '+(saving?'正在保存':!value?'正在读取':dirty?'未保存 · 本机草稿':'已保存')}</span></div></div><div className="file-editor-actions">{!asset&&<><Button aria-label="重新载入" title="重新载入" onClick={reload}><RotateCw size={14}/></Button><Button className="file-save" disabled={!dirty||saving} onClick={save}><Save size={14}/>保存</Button></>}{!task&&<FileActions workspaceId={workspaceId} path={path}/>}</div></div>}
    {error&&<p role="alert" className="ide-error">{error}</p>}
    {asset?(assetUrl?(extension==='pdf'?<iframe className="wb-asset-pdf" title={`${path} PDF 预览`} src={assetUrl}/>:<img className="wb-asset-image" alt={path} src={assetUrl}/>):!error&&<LoadingState label="正在打开预览" className="file-editor-loading"/>):(value?(preview&&markdown?<DocumentReader source={value.content} path={path}/>:<div className="file-source-frame"><Editor path={`${workspaceId}/${path}`} value={value.content} theme={`velella-${theme}`} loading={<LoadingState label="正在打开文件" className="file-editor-loading"/>} onChange={content=>{if(saving)return;const next={...value,content:content??''};setValue(next);try{if(next.content===next.previous)localStorage.removeItem(key);else localStorage.setItem(key,JSON.stringify(next))}catch(e){setError(`草稿未能持久保存：${String(e)}，请保存文件后再关闭`)}}} options={{...fileEditorOptions(markdown,preferences.editor),readOnly:saving}}/></div>):!error&&<LoadingState label="正在打开文档" className="file-editor-loading"/>)}
  </div>;
}

function FileActions({workspaceId,path}:{workspaceId:string;path:string}){
  const dialog=useRef<HTMLDialogElement>(null);
  const [mode,setMode]=useState<'rename'|'delete'>('rename'),[next,setNext]=useState(path),[busy,setBusy]=useState(false),[error,setError]=useState('');
  function open(nextMode:'rename'|'delete'){setMode(nextMode);setNext(path);setError('');dialog.current?.showModal()}
  async function submit(e:React.FormEvent){e.preventDefault();try{setBusy(true);if(mode==='rename'){const value=next.trim();await rpc({cmd:'workbench.file.rename',workspace_id:workspaceId,path,new_path:value});window.dispatchEvent(new CustomEvent('velella:file-changed',{detail:{kind:'rename',path,newPath:value}}))}else{await rpc({cmd:'workbench.file.delete',workspace_id:workspaceId,path,confirmed:true});window.dispatchEvent(new CustomEvent('velella:file-changed',{detail:{kind:'delete',path}}))}dialog.current?.close()}catch(e){setError(String(e))}finally{setBusy(false)}}
  return <><ActionMenu label="文件操作" icon={<MoreHorizontal size={16}/>}><Button disabled={busy} onClick={()=>open('rename')}>重命名</Button><Button disabled={busy} className="danger" onClick={()=>open('delete')}>删除</Button></ActionMenu><NativeDialog ref={dialog} aria-labelledby="file-action-title"><form onSubmit={submit}><div className="dialog-heading"><h2 id="file-action-title">{mode==='rename'?'重命名文件':'删除文件'}</h2></div><div className="dialog-body">{mode==='rename'?<label>新路径<Input aria-label="新文件路径" autoFocus required value={next} onChange={e=>setNext(e.target.value)}/></label>:<><p>删除 <strong>{path}</strong>？此操作不可恢复。</p><p>未保存的本机草稿不会被写入文件。</p></>}{error&&<p role="alert" className="ide-error">{error}</p>}</div><div className="dialog-footer"><Button type="button" onClick={()=>dialog.current?.close()}>取消</Button><Button className={mode==='delete'?'danger':''} disabled={busy} type="submit">{mode==='rename'?'确认重命名':'确认删除'}</Button></div></form></NativeDialog></>
}

