import {useConfirm} from './ConfirmAction';
import {useEffect,useRef,useState} from 'react';
import Editor,{DiffEditor} from './LazyEditor';
import ResizeHandle from './ResizeHandle';
import {useAppearance} from './Appearance';
import {Button} from './ui';
import {rpc,Task} from './api';
import {fileEditorOptions} from './editor-appearance';
import './editor-a.css';
import {useWorkbenchPreferences} from './useWorkbenchPreferences';
import {useExecutionTarget} from './ExecutionHostsPanel';
import {currentTarget,sameTarget} from './execution-target';
import {shortcutMatches} from './workbench-preferences';
export default function FilesView({task,rightOpen=true,rightWidth=240,rightMax=480,onResize=()=>{}}:{task:Task;rightOpen?:boolean;rightWidth?:number;rightMax?:number;onResize?:(width:number)=>void}) {
  const ask=useConfirm();
  const preferences=useWorkbenchPreferences(),target=useExecutionTarget(),saveLock=useRef(false);
  const [saving,setSaving]=useState(false);
  const panel=useRef<HTMLDivElement>(null);
  const {theme}=useAppearance();
  const [files,setFiles]=useState<string[]>([]),[path,setPath]=useState(''),[content,setContent]=useState(''),[previous,setPrevious]=useState(''),[original,setOriginal]=useState(''),[diff,setDiff]=useState(false),[error,setError]=useState('');
  useEffect(()=>{let active=true;rpc({cmd:'task.files',task_id:task.id}).then(r=>{if(active)setFiles(r.files)}).catch(e=>setError(e.message));return()=>{active=false}},[task.id]);
  async function load(p:string){if(content!==previous&&!(await ask('当前文件有未保存修改，放弃并切换文件？')))return;try{const r=await rpc({cmd:'file.read',task_id:task.id,path:p});setPath(p);setContent(r.content);setPrevious(r.content);setOriginal(r.original);setError('')}catch(e){setError(String(e))}}
  async function save(){if(saveLock.current||!sameTarget(target,currentTarget()))return;saveLock.current=true;setSaving(true);try{await rpc({cmd:'file.write',task_id:task.id,path,content,previous},target);setPrevious(content);setError('')}catch(e){setError(String(e))}finally{saveLock.current=false;setSaving(false)}}
  useEffect(()=>{if(!preferences.editor.autoSave||!path||content===previous||saving||error||diff)return;const timer=setTimeout(()=>void save(),preferences.editor.autoSaveDelay);return()=>clearTimeout(timer)},[content,previous,path,saving,error,diff,target,preferences.editor.autoSave,preferences.editor.autoSaveDelay]);
  useEffect(()=>{const keydown=(e:KeyboardEvent)=>{if(!e.isComposing&&shortcutMatches('saveFile',e,preferences)&&!document.querySelector('dialog[open],.app-settings-page')&&panel.current?.contains(document.activeElement)){e.preventDefault();if(path&&content!==previous&&!saving)void save()}};window.addEventListener('keydown',keydown);return()=>window.removeEventListener('keydown',keydown)},[path,content,previous,saving,preferences,target]);
  return <div ref={panel} className="files-layout" style={{gridTemplateColumns:rightOpen?`minmax(0,1fr) ${rightWidth}px`:'minmax(0,1fr)'}}>{rightOpen&&<div className="file-list" aria-label="任务文件"><ResizeHandle side="right" width={rightWidth} min={220} max={rightMax} onChange={onResize}/>{files.map(f=><Button key={f} disabled={saving} className={f===path?'selected':''} onClick={()=>load(f)} title={f}>{f}</Button>)}</div>}<div className="editor-panel"><div className="panel-toolbar"><span className="mono truncate">{path||'选择文件'}{content!==previous?' · 未保存':''}</span><Button onClick={()=>setDiff(!diff)} disabled={!path||task.direct||saving} title={task.direct?'普通文件夹没有 Git 基线':undefined}>{diff?'编辑':'对比基线'}</Button><Button onClick={save} disabled={!path||content===previous||saving}>保存</Button></div>{error&&<p role="alert" className="error">{error}</p>}{path?(diff?<DiffEditor theme={`velella-${theme}`} original={original} modified={content} options={{...fileEditorOptions(false,preferences.editor),readOnly:true}}/>:<Editor theme={`velella-${theme}`} path={path} value={content} onChange={v=>{if(!saving)setContent(v||'')}} options={{...fileEditorOptions(/\.(md|markdown|mdown)$/i.test(path),preferences.editor),readOnly:saving}}/>):<div className="empty small">从右侧选择文件，查看或编辑任务中的改动。</div>}</div></div>;
}
