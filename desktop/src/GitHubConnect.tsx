import {useEffect,useRef,useState} from 'react';
import {Check,Copy,ExternalLink,Github,LockKeyhole,QrCode,X} from 'lucide-react';
import {rpc} from './api';
import {Button,Input,NativeDialog} from './ui';
import {openSourceUrl,RepositorySource,TaskSource} from './task-sources';

type Account={available:boolean;authenticated:boolean;login?:string};
type Attempt={request_id:string;state:string;user_code?:string;verification_uri?:string;qr_svg?:string;login?:string;message?:string};
type Repository={full_name:string;private:boolean;archived:boolean;description:string};
const verificationUrl='https://github.com/login/device';

export default function GitHubConnect({repositories,onClose,onSaved,onManual}:{repositories:RepositorySource[];onClose:()=>void;onSaved:(source:TaskSource)=>void;onManual:()=>void}) {
  const dialog=useRef<HTMLDialogElement>(null),alive=useRef(false),authId=useRef<string|null>(null),generation=useRef(0),repoGeneration=useRef(0),saving=useRef(false),starting=useRef(false),settled=useRef(new Set<string>());
  const [account,setAccount]=useState<Account|null>(null),[checking,setChecking]=useState(true),[attempt,setAttempt]=useState<Attempt|null>(null);
  const [rows,setRows]=useState<Repository[]>([]),[next,setNext]=useState<number|null>(null),[loading,setLoading]=useState(false),[selected,setSelected]=useState(''),[search,setSearch]=useState('');
  const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState(''),[copied,setCopied]=useState(false);
  useEffect(()=>{
    alive.current=true;dialog.current?.showModal();void checkAccount();
    return ()=>{alive.current=false;generation.current++;repoGeneration.current++;const id=authId.current;if(id)void rpc({cmd:'sources.github.auth.cancel',request_id:id}).catch(()=>{});};
  },[]);
  async function loadRepositories(page=1) {
    const current=++repoGeneration.current;setLoading(true);setError('');
    try {
      const response=await rpc({cmd:'sources.github.repositories',page});
      if(!alive.current||current!==repoGeneration.current)return;
      setRows(previous=>Array.from(new Map((page===1?response.repositories:[...previous,...response.repositories]).map((r:Repository)=>[r.full_name,r])).values()) as Repository[]);
      setNext(response.next_page??null);
    } catch(e) {if(alive.current&&current===repoGeneration.current)setError(String(e));}
    finally {if(alive.current&&current===repoGeneration.current)setLoading(false);}
  }
  async function checkAccount() {
    const current=++generation.current;setChecking(true);setError('');
    try {
      const result=await rpc({cmd:'sources.github.account'});
      if(!alive.current||current!==generation.current)return;
      setAccount(result);
      if(result.authenticated){setAttempt(null);void loadRepositories();}
    } catch(e) {if(alive.current&&current===generation.current)setError(String(e));}
    finally {if(alive.current&&current===generation.current)setChecking(false);}
  }
  function accept(value:Attempt) {
    if(!alive.current||authId.current!==value.request_id||settled.current.has(value.request_id))return;
    if(['complete','cancelled','failed'].includes(value.state))settled.current.add(value.request_id);
    setAttempt(value);
    if(value.state==='complete') {
      setAccount({available:true,authenticated:true,login:value.login});setNotice('GitHub 授权成功，请选择仓库。');void loadRepositories();
    }
  }
  async function poll(id:string) {
    try {const result=await rpc({cmd:'sources.github.auth.status',request_id:id});accept(result.attempt);}
    catch {if(alive.current&&authId.current===id&&!settled.current.has(id))setAttempt({request_id:id,state:'uncertain',message:'暂时无法确认授权结果。请检查连接后重新查询，不会自动发起新授权。'});}
  }
  useEffect(()=>{
    if(!attempt||!['starting','waiting'].includes(attempt.state))return;
    const id=attempt.request_id;let live=true,timer:ReturnType<typeof setTimeout>;
    const tick=async()=>{await poll(id);if(live)timer=setTimeout(tick,1200);};
    timer=setTimeout(tick,1200);return ()=>{live=false;clearTimeout(timer);};
  },[attempt?.request_id,attempt?.state]);
  async function begin() {
    if(starting.current||authId.current&&attempt&&['starting','waiting','uncertain'].includes(attempt.state))return;
    const id=crypto.randomUUID();authId.current=id;starting.current=true;setError('');setCopied(false);setNotice('');
    setAttempt({request_id:id,state:'requesting'});
    try {const result=await rpc({cmd:'sources.github.auth.start',request_id:id});accept(result.attempt);}
    catch {if(alive.current&&authId.current===id)await poll(id);}
    finally {starting.current=false;if(!alive.current)void rpc({cmd:'sources.github.auth.cancel',request_id:id}).catch(()=>{});}
  }
  async function stop() {
    const id=authId.current;if(!id)return;setError('');
    try {const result=await rpc({cmd:'sources.github.auth.cancel',request_id:id});accept(result.attempt);}
    catch {if(alive.current)setError('停止等待的结果未确认，请稍后检查登录。');}
  }
  async function connect() {
    if(saving.current||!selected)return;saving.current=true;setBusy(true);setError('');
    const local=repositories.find(r=>r.host==='github.com'&&r.scope===selected);
    try {
      const result=await rpc({cmd:'sources.save',source:{id:'',project_id:local?.project_id||'',provider:'github',name:selected,base_url:'https://github.com',scope:selected,auth_mode:'cli',login:'',jira_kind:''},token:''});
      if(alive.current){onSaved(result.source);onClose();}
    } catch(e) {if(alive.current)setError(String(e));}
    finally {saving.current=false;if(alive.current)setBusy(false);}
  }
  const pending=attempt&&['requesting','starting','waiting','uncertain'].includes(attempt.state);
  const visible=rows.filter(r=>r.full_name.toLowerCase().includes(search.trim().toLowerCase()));
  const qrReady=attempt?.state==='waiting'&&attempt.verification_uri===verificationUrl&&attempt.qr_svg&&attempt.user_code;
  return <NativeDialog ref={dialog} className="source-dialog github-connect" aria-labelledby="github-connect-title" onClose={onClose} onCancel={e=>{if(busy)e.preventDefault()}}>
    <div className="dialog-heading"><h2 id="github-connect-title">连接 GitHub</h2><Button disabled={busy} aria-label="关闭 GitHub 连接" onClick={onClose}><X size={16}/></Button></div>
    <div className="dialog-body">
      {checking?<div className="github-auth-intro" role="status"><Github size={30}/><h3>正在检查 GitHub 登录</h3><p>已有登录时，可以直接选择仓库。</p></div>:account?.authenticated?<>
        <div className="github-account"><Github size={24}/><div><strong>{account.login}</strong><p className="source-muted">选择要连接的仓库，开始开发时再选择本地项目。</p></div><Check size={18}/></div>
        <label>搜索仓库<Input autoFocus placeholder="按仓库名称筛选" value={search} onChange={e=>setSearch(e.target.value)} disabled={busy}/></label>
        <div className="github-repositories" role="radiogroup" aria-label="GitHub 仓库">
          {visible.map(repo=><label className="github-repository" key={repo.full_name}><input type="radio" name="github-repository" value={repo.full_name} checked={selected===repo.full_name} disabled={busy} onChange={()=>setSelected(repo.full_name)} aria-label={repo.full_name}/><span><strong>{repo.full_name}</strong>{repo.description&&<small>{repo.description}</small>}<small>{repo.private?'私有仓库':'公开仓库'}{repo.archived?' · 已归档':''}</small></span>{repo.private&&<LockKeyhole size={14} aria-hidden="true"/>}</label>)}
          {!visible.length&&!loading&&<p className="source-muted">{search?'当前已加载的仓库中没有匹配项。':'暂无可见仓库，可刷新重试或手动填写仓库路径。'}</p>}
          {loading&&<p role="status" className="source-muted">正在读取仓库…</p>}
        </div>
        <div className="github-actions"><Button disabled={loading||busy} onClick={()=>void loadRepositories()}>刷新仓库</Button>{next&&<Button disabled={loading||busy} onClick={()=>void loadRepositories(next)}>加载更多仓库</Button>}</div>
      </>:<>
        <div className="github-auth-intro"><Github size={30}/><h3>扫码连接 GitHub</h3><p>用手机打开 GitHub 授权页，完成后在这里选择仓库。</p></div>
        {qrReady?<div className="github-qr-card">
          <img className="github-qr" src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(attempt.qr_svg!)}`} alt="GitHub 设备授权二维码，扫码后输入下方设备码"/>
          <p>扫码后，在 GitHub 输入此设备码</p>
          <div className="github-device-code"><code>{attempt.user_code}</code><Button aria-label="复制设备码" onClick={()=>void navigator.clipboard.writeText(attempt.user_code!).then(()=>setCopied(true)).catch(()=>setError('复制失败，请手动输入上方设备码。'))}>{copied?<Check size={16}/>:<Copy size={16}/>}</Button></div>
          {copied&&<span role="status" className="source-muted">设备码已复制</span>}
          <div className="github-actions"><Button onClick={()=>void openSourceUrl(verificationUrl).catch(e=>setError(String(e)))}><ExternalLink size={15}/>在浏览器打开</Button><Button onClick={()=>void stop()}>停止等待</Button></div>
          <p className="source-muted" role="status">等待你在 GitHub 完成授权…</p>
          <p className="source-muted">授权应用：GitHub CLI · 二维码失效后可重新生成</p>
        </div>:pending?<div className="github-auth-intro" role="status"><QrCode size={36}/><p>{attempt?.state==='uncertain'?attempt.message:'正在生成登录二维码…'}</p><div className="github-actions">{attempt?.state==='uncertain'&&<Button onClick={()=>void poll(attempt.request_id)}>检查授权结果</Button>}<Button onClick={()=>void stop()}>停止等待</Button></div></div>:<>
          {attempt?.message&&<p role="status" className="source-muted">{attempt.message}</p>}
          {account?.available===false?<p className="source-muted">扫码连接需要 GitHub CLI。安装后重新检查，或使用下方的手动连接。</p>:<Button className="primary github-generate" onClick={()=>void begin()}><QrCode size={18}/>{attempt?'重新生成二维码':'生成登录二维码'}</Button>}
          <div className="github-actions"><Button onClick={()=>void checkAccount()}>重新检查登录</Button>{account?.available===false&&<Button onClick={()=>void openSourceUrl('https://cli.github.com/').catch(e=>setError(String(e)))}>获取 GitHub CLI<ExternalLink size={14}/></Button>}</div>
        </>}
      </>}
      {notice&&<p className="source-muted" role="status">{notice}</p>}
      {error&&<p role="alert" className="source-error">{error}</p>}
    </div>
    <div className="dialog-footer github-connect-footer"><Button disabled={busy} onClick={onManual}>手动连接 / 企业站点</Button><div><Button disabled={busy} onClick={onClose}>取消</Button>{account?.authenticated&&<Button className="primary" disabled={busy||!selected} onClick={()=>void connect()}>{busy?'正在连接…':'连接仓库'}</Button>}</div></div>
  </NativeDialog>;
}
