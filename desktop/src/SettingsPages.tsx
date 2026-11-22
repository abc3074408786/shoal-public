import {useEffect,useRef,useState,type ReactNode} from 'react';
import {Copy,FolderOpen,RefreshCw,Trash2} from 'lucide-react';
import {Button,Input} from './ui';
import {open,isTauri,notifyDesktop} from './desktop-platform';
import {rpc} from './api';
import {scheduleAgentCheck} from './agent-check-queue';
import {namespacedKey,type ExecutionTarget} from './execution-target';
import {shortcutCatalog,shortcutConflicts,shortcutFromEvent,validShortcut,reservedShortcut,type WorkbenchPreferences,type ShortcutId} from './workbench-preferences';
import type {AccountPreferences,AccountProfile,AccountProvider} from './account-preferences';
import type {Theme} from './ui-preferences';
export type SettingsDestination='hosts'|'sources'|'automation'|'tools'|'project';
export type ExtendedSettingsPage='appearance'|'editing'|'shortcuts'|'notifications'|'accounts'|'connections';
export const settingsPageCopy:Record<ExtendedSettingsPage,{title:string;description:string}>={
  appearance:{title:'外观',description:'调整这台桌面的主题和界面大小。'},
  editing:{title:'输入与编辑',description:'设置文件编辑器的显示与保存方式。'},
  shortcuts:{title:'快捷键',description:'设置常用操作的按键，终端内的 CLI 按键由工具自身管理。'},
  notifications:{title:'通知',description:'在任务需要你处理时收到桌面提醒。'},
  accounts:{title:'AI 账号',description:'管理 Codex 和 Claude Code 使用的配置目录。'},
  connections:{title:'连接与工具',description:'前往已有的连接、项目和工作区管理。'},
};
function Row({label,note,children}:{label:string;note?:string;children:ReactNode}){return <div className="settings-option-row"><div><strong>{label}</strong>{note&&<p>{note}</p>}</div><div className="settings-option-control">{children}</div></div>}
function Toggle({label,value,onChange,disabled=false}:{label:string;value:boolean;onChange:(value:boolean)=>void;disabled?:boolean}){return <label className="agent-enable-toggle"><input type="checkbox" role="switch" aria-label={label} checked={value} disabled={disabled} onChange={e=>onChange(e.target.checked)}/><span aria-hidden="true"/></label>}
export default function SettingsPages({page,value,onChange,theme,onTheme,accounts,onAccounts,target,onNavigate,onLogin,canLogin}:{page:ExtendedSettingsPage;value:WorkbenchPreferences;onChange:(v:WorkbenchPreferences)=>void;theme:Theme;onTheme:(v:Theme)=>void;accounts:AccountPreferences;onAccounts:(v:AccountPreferences)=>void;target:ExecutionTarget;onNavigate?:(destination:SettingsDestination)=>void;onLogin?:(profile:AccountProfile|{provider:AccountProvider})=>Promise<void>;canLogin?:boolean}){
  const [recording,setRecording]=useState<ShortcutId|null>(null),[error,setError]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{setError('');setMessage('');setRecording(null)},[page]);
  const edit=(patch:Partial<WorkbenchPreferences['editor']>)=>onChange({...value,editor:{...value.editor,...patch}});
  const notify=(patch:Partial<WorkbenchPreferences['notifications']>)=>onChange({...value,notifications:{...value.notifications,...patch}});
  return <div className="settings-options">
    {page==='appearance'&&<>
      <Row label="主题" note="与“视图”菜单使用同一个主题设置。"><select aria-label="界面主题" value={theme} onChange={e=>onTheme(e.target.value as Theme)}><option value="light">浅色</option><option value="dark">深色</option></select></Row>
      <Row label="界面大小" note="保存后应用到整个工作台。"><select aria-label="界面大小" value={value.uiScale} onChange={e=>onChange({...value,uiScale:Number(e.target.value)})}>{[80,90,100,110,120,130].map(v=><option key={v} value={v}>{v}%{v===100?' · 默认':''}</option>)}</select></Row>
      <div className="settings-type-preview"><strong>把注意力留给当前工作。</strong><p>项目、文件与 Agent 会话，共用一个工作台。</p><code>velella-cli status</code></div>
    </>}
    {page==='editing'&&<>
      <Row label="编辑器字号"><input type="number" aria-label="编辑器字号" min={10} max={24} value={value.editor.fontSize} onChange={e=>{if(e.target.value)edit({fontSize:Math.min(24,Math.max(10,Number(e.target.value)))})}}/></Row>
      <Row label="自动换行" note="自动模式下，文档换行、代码不换行。"><select aria-label="自动换行" value={value.editor.wordWrap} onChange={e=>edit({wordWrap:e.target.value as WorkbenchPreferences['editor']['wordWrap']})}><option value="auto">按文件类型</option><option value="on">始终换行</option><option value="off">关闭</option></select></Row>
      <Row label="代码缩略图"><Toggle label="代码缩略图" value={value.editor.minimap} onChange={minimap=>edit({minimap})}/></Row>
      <Row label="自动保存文件" note="停止输入后写入文件；发生冲突或保存失败时暂停，保留本机草稿。"><Toggle label="自动保存文件" value={value.editor.autoSave} onChange={autoSave=>edit({autoSave})}/></Row>
      <Row label="自动保存延迟"><select aria-label="自动保存延迟" disabled={!value.editor.autoSave} value={value.editor.autoSaveDelay} onChange={e=>edit({autoSaveDelay:Number(e.target.value)})}>{[500,1000,1500,3000,5000,10000].map(v=><option key={v} value={v}>{v/1000} 秒</option>)}</select></Row>
    </>}
    {page==='shortcuts'&&<>
      <p>点击按键后输入新的组合，按 Escape 取消。保留系统、编辑和终端常用按键。</p>
      {shortcutCatalog.map(action=><Row key={action.id} label={action.label}><Button type="button" className="settings-keybinding" aria-label={`修改${action.label}快捷键`} aria-pressed={recording===action.id} onClick={()=>{setRecording(action.id);setError('')}} onBlur={()=>setRecording(null)} onKeyDown={e=>{
        if(recording!==action.id)return;e.preventDefault();e.stopPropagation();
        if(e.key==='Escape'){setRecording(null);return}
        const keys=shortcutFromEvent(e);if(!keys)return;
        if(!validShortcut(keys)||reservedShortcut(keys)){setError('这个组合由系统、编辑器或终端保留，请使用带 Ctrl、Command 或 Alt 的其他组合。');return}
        const shortcuts={...value.shortcuts,[action.id]:keys},conflicts=shortcutConflicts(shortcuts);
        if(conflicts.length){setError('快捷键冲突：'+conflicts.join('、'));return}
        onChange({...value,shortcuts});setRecording(null);setError('');
      }}>{recording===action.id?'请按组合键…':(value.shortcuts[action.id]||action.keys).replace('Mod',/Mac/i.test(navigator.platform)?'Command':'Ctrl')}</Button><Button type="button" aria-label={`恢复${action.label}快捷键`} disabled={!value.shortcuts[action.id]} onClick={()=>{const shortcuts={...value.shortcuts};delete shortcuts[action.id];const conflicts=shortcutConflicts(shortcuts);if(conflicts.length){setError('恢复后会产生冲突，请先恢复全部快捷键。');return}onChange({...value,shortcuts});setError('')}}>恢复</Button></Row>)}
      <Button type="button" onClick={()=>{onChange({...value,shortcuts:{}});setError('');setRecording(null)}}>恢复全部默认快捷键</Button>
    </>}
    {page==='notifications'&&<>
      <Row label="桌面通知" note="需要桌面应用保持运行。"><Toggle label="桌面通知" value={value.notifications.enabled} onChange={enabled=>notify({enabled})}/></Row>
      {([['review','任务等待审查'],['attention','任务需要处理或恢复'],['delivered','任务已交付'],['backgroundOnly','仅在应用不在前台时通知'],['sound','播放系统提示音']] as const).map(([key,label])=><Row key={key} label={label}><Toggle label={label} disabled={!value.notifications.enabled} value={value.notifications[key]} onChange={next=>notify({[key]:next})}/></Row>)}
      <div className="settings-inline-action"><Button type="button" disabled={!isTauri()||busy} onClick={async()=>{setBusy(true);setError('');setMessage('');try{await notifyDesktop({title:'鱼群通知测试',body:'桌面通知请求已发送。',silent:!value.notifications.sound});setMessage('已向系统发送测试通知，请查看系统通知中心。')}catch(e){setError(String(e))}finally{setBusy(false)}}}>发送测试通知</Button><p>{isTauri()?'测试立即发送；其他更改在保存后生效。':'系统通知需要在桌面应用中测试。'}</p></div>
    </>}
    {page==='accounts'&&<AccountSettings value={accounts} onChange={onAccounts} target={target} onLogin={onLogin} canLogin={canLogin}/>}
    {page==='connections'&&<>
      {([{id:'hosts',label:'执行主机',note:'登记、检查和切换 SSH 执行主机。'}, {id:'sources',label:'任务来源',note:'连接 GitHub、GitLab、Linear 和 Jira。'}, {id:'automation',label:'自动化',note:'管理日程、配置与运行记录。'}, {id:'tools',label:'工作区工具',note:'管理 SSH 配置、浏览器与 Git 工作树。'}, {id:'project',label:'当前项目设置',note:'设置默认基线与准备脚本。'}] as const).map(link=><Row key={link.id} label={link.label} note={link.note}><Button type="button" disabled={!onNavigate||(target.kind!=='local'&&(link.id==='sources'||link.id==='automation'))} onClick={()=>onNavigate?.(link.id)}>打开</Button></Row>)}
    </>}
    {error&&<p role="alert" className="error">{error}</p>}{message&&<p role="status">{message}</p>}
  </div>;
}
function AccountSettings({value,onChange,target,onLogin,canLogin}:{value:AccountPreferences;onChange:(v:AccountPreferences)=>void;target:ExecutionTarget;onLogin?:(profile:AccountProfile|{provider:AccountProvider})=>Promise<void>;canLogin?:boolean}){
  const [provider,setProvider]=useState<AccountProvider>('codex'),[name,setName]=useState(''),[home,setHome]=useState(''),[error,setError]=useState(''),[message,setMessage]=useState('');
  const [checks,setChecks]=useState<Record<string,string>>({}),[checking,setChecking]=useState(false),[busy,setBusy]=useState(false),[support,setSupport]=useState<boolean|null>(null);
  const scan=useRef<AbortController|null>(null);
  useEffect(()=>{setChecking(false);setChecks({});return()=>{scan.current?.abort();scan.current=null}},[target]);
  useEffect(()=>{let live=true;void rpc({cmd:'protocol'},target).then(r=>{if(live)setSupport(r.capabilities?.includes('terminal-accounts-v1')===true)}).catch(()=>{if(live)setSupport(false)});return()=>{live=false}},[target]);
  async function check(){
    if(checking||scan.current)return;
    const controller=new AbortController();scan.current=controller;setChecking(true);setError('');
    const profiles=[...(['codex','claude-code'] as const).map(provider=>({id:provider,provider,home:undefined})),...value.profiles];
    try{await Promise.all(profiles.map(async p=>{
      try{
        if(p.home&&support!==true){setChecks(old=>({...old,[p.id]:'当前后台不支持目录检测'}));return}
        const r=await scheduleAgentCheck(namespacedKey(target,'agent.check'),()=>rpc({cmd:'agent.check',agent:p.provider,...(p.home?{account_home:p.home}:{})},target),controller.signal);
        if(!controller.signal.aborted)setChecks(old=>({...old,[p.id]:!r.available?'未检测到 CLI':typeof r.authenticated!=='boolean'?'登录状态未知':r.authenticated?'已登录':'未登录'}));
      }catch{if(!controller.signal.aborted)setChecks(old=>({...old,[p.id]:'检测失败'}))}
    }))}finally{if(scan.current===controller){scan.current=null;if(!controller.signal.aborted)setChecking(false)}}
  }
  async function login(profile:AccountProfile|{provider:AccountProvider}){if(!onLogin||busy)return;setBusy(true);setError('');try{await onLogin(profile)}catch(e){setError(String(e))}finally{setBusy(false)}}
  return <>
    <p>新建原生终端可使用指定配置目录。已有会话、恢复会话、受限终端和受管任务保留自己的配置。CLI 环境中的 API Key 仍由工具自身处理。</p>
    {support===false&&<p role="status">当前后台尚不支持账号目录。可以保存目录清单，使用前需要升级后台。</p>}
    <div className="settings-section-heading"><h4>账号目录</h4><Button type="button" disabled={checking} onClick={()=>void check()}><RefreshCw size={13}/>{checking?'正在检测…':'检查登录状态'}</Button></div>
    {(['codex','claude-code'] as const).map(id=><section className="settings-account-group" key={id} aria-label={`${id} 账号目录`}>
      <Row label={id==='codex'?'Codex':'Claude Code'} note={checks[id]||'CLI 当前配置 · 尚未检测'}><Button type="button" disabled={!onLogin||!canLogin||busy} onClick={()=>void login({provider:id})}>打开登录终端</Button></Row>
      <Row label="新建终端使用" note="切换目录只影响之后从工作区打开的原生终端。"><select aria-label={`${id} 默认账号目录`} value={value.defaults[id]||''} onChange={e=>onChange({...value,defaults:{...value.defaults,[id]:e.target.value||undefined}})}><option value="">沿用 CLI 配置</option>{value.defaults[id]&&!value.profiles.some(p=>p.provider===id&&p.id===value.defaults[id])&&<option value={value.defaults[id]} disabled>所选目录已不可用 · 请重新选择</option>}{value.profiles.filter(p=>p.provider===id).map(p=><option key={p.id} value={p.id} disabled={support!==true}>{p.name}</option>)}</select></Row>
      {value.profiles.filter(p=>p.provider===id).map(p=><div className="settings-account-profile" key={p.id}><div><strong>{p.name}</strong><code>{p.home}</code><p role="status">{checks[p.id]||'尚未检测'}</p></div><div><Button type="button" disabled={!onLogin||!canLogin||busy||support!==true} onClick={()=>void login(p)}>登录</Button><Button type="button" aria-label={`移除账号目录 ${p.name}`} title="仅移除关联，保留目录与凭据" onClick={()=>{const defaults={...value.defaults};if(defaults[p.provider]===p.id)delete defaults[p.provider];onChange({profiles:value.profiles.filter(old=>old.id!==p.id),defaults})}}><Trash2 size={14}/></Button></div></div>)}
    </section>)}
    <h4>添加已有配置目录</h4>
    <div className="settings-account-form">
      <label>工具<select aria-label="账号工具" value={provider} onChange={e=>setProvider(e.target.value as AccountProvider)}><option value="codex">Codex</option><option value="claude-code">Claude Code</option></select></label>
      <label>名称<Input value={name} aria-label="账号目录名称" maxLength={60} onChange={e=>setName(e.target.value)}/></label>
      <label>配置目录<Input value={home} aria-label="账号配置目录" placeholder={target.kind==='local'?'D:\\AgentAccounts\\work':'/srv/agent-accounts/work'} onChange={e=>setHome(e.target.value)}/></label>
      {target.kind==='local'&&isTauri()&&<Button type="button" onClick={async()=>{try{const result=await open({directory:true,multiple:false,title:'选择已有账号配置目录'});if(typeof result==='string')setHome(result)}catch(e){setError(String(e))}}}><FolderOpen size={14}/>选择目录</Button>}
      <Button type="button" disabled={!name.trim()||!home.trim()||value.profiles.length>=32} onClick={()=>{setError('');if(!/^(?:[A-Za-z]:[\\/]|\\\\[^\\]+\\[^\\]+|\/)/.test(home.trim())||/[\x00-\x1f]/.test(home)){setError('请填写执行主机上的绝对目录路径。');return}if(value.profiles.some(p=>p.provider===provider&&p.home===home.trim())){setError('这个目录已在清单中。');return}onChange({...value,profiles:[...value.profiles,{id:crypto.randomUUID(),provider,name:name.trim(),home:home.trim()}]});setName('');setHome('')}}>添加到清单</Button>
    </div>
    <p>名称、目录与默认选择在保存后生效；移除关联保留原文件。登录按钮会立即打开当前工作区的终端，由你完成工具自身的登录。</p>
    {!canLogin&&<p>打开可用工作区并保存设置后，可以使用登录终端。</p>}
    <details><summary>其他工具的登录</summary><p>其他 Agent 的账号继续在各自 CLI 中管理。</p>{['gemini','opencode auth login','omp','kimi login','zcode-free'].map(command=><div className="settings-command" key={command}><code>{command}</code><Button type="button" aria-label={`复制 ${command}`} onClick={async()=>{try{await navigator.clipboard.writeText(command);setMessage('已复制命令')}catch{setError('复制失败，请手动选择命令。')}}}><Copy size={13}/></Button></div>)}</details>
    {error&&<p role="alert" className="error">{error}</p>}{message&&<p role="status">{message}</p>}
  </>;
}
