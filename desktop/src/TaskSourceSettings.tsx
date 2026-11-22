import {useEffect,useRef,useState} from 'react';
import {X} from 'lucide-react';
import {rpc} from './api';
import {Button,Input,NativeDialog} from './ui';
import {Provider,providers,RepositorySource,TaskSource} from './task-sources';
import GitHubConnect from './GitHubConnect';
import LinearConnect from './LinearConnect';

export default function TaskSourceSettings({repositories,provider,source,onClose,onSaved}:{repositories:RepositorySource[];provider:Provider;source?:TaskSource;onClose:()=>void;onSaved:(source:TaskSource)=>void}){
  const dialog=useRef<HTMLDialogElement>(null);
  const git=provider==='github'||provider==='gitlab',candidates=repositories.filter(r=>!r.provider||r.provider===provider);
  const [draft,setDraft]=useState<TaskSource>(()=>source||{id:'',project_id:'',provider,name:providers[provider],base_url:({github:'https://github.com',gitlab:'https://gitlab.com',linear:'https://api.linear.app',jira:''})[provider],scope:'',login:'',jira_kind:'cloud',auth_mode:git?'cli':'token'});
  const [token,setToken]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const inFlight=useRef(false);
  const [manual,setManual]=useState(false);
  const guided=provider==='github'&&!source&&!manual;
  const linearGuided=provider==='linear'&&!manual&&(!source||source.auth_mode==='oauth');
  useEffect(()=>{if(!guided&&!linearGuided)dialog.current?.showModal()},[guided,linearGuided]);
  const set=(field:keyof TaskSource,value:string)=>{setDraft({...draft,[field]:value});setError('')};
  async function save(e:React.FormEvent){e.preventDefault();if(inFlight.current)return;inFlight.current=true;setBusy(true);setError('');try{const r=await rpc({cmd:'sources.save',source:draft,token});setToken('');onSaved(r.source);onClose()}catch(e){setError(String(e))}finally{setBusy(false);inFlight.current=false}}
  if(guided)return <GitHubConnect repositories={repositories} onClose={onClose} onSaved={onSaved} onManual={()=>setManual(true)}/>;
  if(linearGuided)return <LinearConnect onClose={onClose} onSaved={onSaved} onManual={()=>{setDraft({...draft,auth_mode:'token'});setManual(true)}}/>;
  return <NativeDialog ref={dialog} className="source-dialog" aria-labelledby="source-settings-title" onClose={onClose} onCancel={e=>{if(busy)e.preventDefault()}}><form onSubmit={save}>
    <div className="dialog-heading"><h2 id="source-settings-title">{source?'编辑':'连接'} {providers[provider]}</h2><Button type="button" disabled={busy} aria-label="关闭来源设置" onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body">
      <label>连接名称<Input required maxLength={80} autoFocus value={draft.name} disabled={busy} onChange={e=>set('name',e.target.value)}/></label>
      {git&&!source&&candidates.length>0&&<label>已有项目的仓库<select aria-label="已有项目的仓库" disabled={busy} defaultValue="" onChange={e=>{const r=candidates[Number(e.target.value)];if(r)setDraft({...draft,project_id:r.project_id,name:r.project_name,base_url:`https://${r.host}`,scope:r.scope})}}><option value="">选择仓库，或在下方填写其他仓库</option>{candidates.map((r,i)=><option key={`${r.project_id}:${r.host}:${r.scope}`} value={i}>{r.project_name} · {r.host}/{r.scope}</option>)}</select></label>}
      <label>站点地址<Input type="url" required disabled={busy||!!source||provider==='linear'} placeholder={provider==='jira'?'https://your-team.atlassian.net':'https://…'} value={draft.base_url} onChange={e=>set('base_url',e.target.value)}/></label>
      {provider==='jira'&&<><label>Jira 版本<select value={draft.jira_kind} disabled={busy||!!source} onChange={e=>set('jira_kind',e.target.value)}><option value="cloud">Jira Cloud</option><option value="dc">Jira Data Center / Server</option></select></label><label>{draft.jira_kind==='cloud'?'账号邮箱':'用户名（使用 PAT 时留空）'}<Input required={draft.jira_kind==='cloud'} disabled={busy||!!source} autoComplete="off" value={draft.login} onChange={e=>set('login',e.target.value)}/></label></>}
      {git&&<label>登录方式<select aria-label="登录方式" disabled={busy} value={draft.auth_mode||'token'} onChange={e=>set('auth_mode',e.target.value)}><option value="cli">使用 {provider==='github'?'GitHub CLI (gh)':'GitLab CLI (glab)'} 的已有登录</option><option value="token">个人 Token / 公开仓库</option></select></label>}
      {draft.auth_mode==='cli'?<p className="source-muted">直接使用此站点的 CLI 登录，无需复制 Token。尚未登录时，请在终端运行 {provider==='github'?'gh':'glab'} auth login，再回来保存连接。</p>:<><label>{provider==='linear'?'个人 API Key':provider==='jira'&&draft.jira_kind==='cloud'?'API Token':'个人访问 Token'}<Input type="password" autoComplete="new-password" disabled={busy} value={token} onChange={e=>setToken(e.target.value)} placeholder={source?'留空保留已保存的凭据':provider==='github'?'公开仓库可留空':'仅保存在此 Windows 用户下'}/></label><p className="source-muted">凭据使用 Windows 用户级加密保存，不会随任务发送给 Agent。</p></>}
      {git?<label>仓库路径<Input required disabled={busy||!!source} value={draft.scope} placeholder="owner/repository" onChange={e=>set('scope',e.target.value)}/></label>:<p className="source-muted">连接后可筛选账号内的{provider==='linear'?'团队':'项目'}。开始开发时再选择本地项目。</p>}
      {error&&<p role="alert" className="source-error">{error}</p>}
    </div><div className="dialog-footer"><Button type="button" disabled={busy} onClick={onClose}>取消</Button><Button className="primary" disabled={busy}>{busy?'正在验证连接…':'连接'}</Button></div>
  </form></NativeDialog>;
}
