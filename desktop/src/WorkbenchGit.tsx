import {useEffect,useRef,useState} from 'react';
import {GitBranch,RefreshCw} from 'lucide-react';
import {Button,Input,NativeDialog,Tip} from './ui';
import {rpc} from './api';
import {parseGitStatus} from './git-status';
export default function WorkbenchGit({workspaceId,variant='full',onOpenDiff}:{workspaceId:string;variant?:'full'|'sidebar'|'diff';onOpenDiff?:()=>void}){
  const confirmation=useRef<HTMLDialogElement>(null);
  const [state,setState]=useState<any>(null),[error,setError]=useState(''),[path,setPath]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{let live=true;const refresh=async()=>{try{const r=await rpc({cmd:'workbench.git',workspace_id:workspaceId});if(live){setState(r);setError('')}}catch(e){if(live)setError(String(e))}};void refresh();window.addEventListener('velella:git-changed',refresh);return()=>{live=false;window.removeEventListener('velella:git-changed',refresh)}},[workspaceId]);
  async function refresh(){try{setState(await rpc({cmd:'workbench.git',workspace_id:workspaceId}));setError('')}catch(e){setError(String(e))}}
  async function action(cmd:string){setBusy(true);try{await rpc({cmd,workspace_id:workspaceId,path,message,confirmed:true});await refresh();window.dispatchEvent(new Event('velella:git-changed'));if(cmd==='workbench.git.commit')setMessage('')}catch(e){setError(String(e))}finally{setBusy(false)}}
  return <div className={`wb-git ${variant==='sidebar'?'git-sidebar':''}`}>
    <div className="panel-toolbar"><GitBranch size={14}/><strong>{state?.branch?.trim()||'Git 审查'}</strong><Tip label="刷新更改"><Button aria-label="刷新更改" onClick={refresh} disabled={busy}><RefreshCw size={14}/></Button></Tip></div>
    {error&&<p role="alert" className="ide-error">{error}</p>}
    {variant!=='diff'&&<div className="git-change-list">{!state?<p>正在读取 Git 状态…</p>:!state.status?<p>工作区干净</p>:parseGitStatus(state.status).map((entry,index)=><Button key={`${entry.path}:${index}`} className={`git-change-row ${entry.kind} ${entry.path===path?'selected':''}`} title={entry.path} onClick={()=>setPath(entry.path)}><span>{entry.path}</span><small aria-label={entry.staged?'已暂存':'未暂存'}>{entry.code.trim()}</small></Button>)}</div>}
    {state?.git&&<>
      {variant!=='diff'&&<><Button className="git-open-diff" onClick={onOpenDiff} disabled={!onOpenDiff}>打开差异审查</Button><form onSubmit={e=>{e.preventDefault();void action('workbench.git.stage')}}><label>文件相对路径<Input aria-label="暂存文件路径" value={path} onChange={e=>setPath(e.target.value)} placeholder="例如 README.md" required/></label><div className="git-form-actions"><Button disabled={busy||!path}>暂存</Button><Button type="button" disabled={busy||!path} onClick={()=>action('workbench.git.unstage')}>取消暂存</Button></div></form></>}
      {variant!=='sidebar'&&<><h3>未暂存差异</h3><pre className="git-diff-text">{state.diff||'无未暂存差异'}</pre><h3>已暂存差异</h3><pre className="git-diff-text">{state.staged||'无已暂存差异'}</pre></>}
      {variant!=='diff'&&<form onSubmit={e=>{e.preventDefault();confirmation.current?.showModal()}}><label>提交说明<Input aria-label="提交说明" required value={message} onChange={e=>setMessage(e.target.value)}/></label><Button className="primary" disabled={busy||!message.trim()||!state.staged}>提交到本地</Button></form>}
    </>}
    <NativeDialog ref={confirmation} aria-labelledby="commit-confirm-title"><div className="dialog-heading"><h2 id="commit-confirm-title">确认本地提交</h2></div><div className="dialog-body"><p>将已暂存改动提交到当前本地分支。未保存草稿不在提交中。</p><p>{message}</p></div><div className="dialog-footer"><Button onClick={()=>confirmation.current?.close()}>取消</Button><Button className="primary" disabled={busy} onClick={()=>{confirmation.current?.close();void action('workbench.git.commit')}}>确认提交</Button></div></NativeDialog>
  </div>;
}
