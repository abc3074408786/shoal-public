import {useEffect,useRef,useState} from 'react';
import {ArrowLeft,Blocks,ChevronDown,Monitor,RefreshCw,Server,ShieldCheck,Workflow,Palette,TextCursorInput,Keyboard,Bell,Users,Plug,Terminal} from 'lucide-react';
import {Button} from './ui';
import {rpc} from './api';
import {scheduleAgentCheck} from './agent-check-queue';
import {currentTarget,namespacedKey} from './execution-target';
import {agentCatalog,globalPermissionModes,globalPermissionFor,permissionOverride,resolvedAgentPermission,withPermissionDefault,permissionModesFor,type AgentId,type AgentPermissionMode,type GlobalPermissionMode,type AgentPreferences,parseAgentPreferences} from './agent-preferences';
import {PermissionDetails} from './AgentPermissionField';
import {useAgentPreferences} from './useAgentPreferences';
import AgentLogo from './AgentLogo';
import AgentDefaultPicker from './AgentDefaultPicker';
import OrchestrationSettings from './OrchestrationSettings';
import LoadingState from './LoadingState';
import {useConfirm} from './ConfirmAction';
import SettingsPages,{settingsPageCopy,type ExtendedSettingsPage,type SettingsDestination} from './SettingsPages';
import {WORKBENCH_PREFERENCES_KEY,WORKBENCH_PREFERENCES_EVENT,readWorkbenchPreferences,parseWorkbenchPreferences} from './workbench-preferences';
import {ACCOUNT_PREFERENCES_KEY,ACCOUNT_PREFERENCES_EVENT,readAccountPreferences,parseAccountPreferences,type AccountProfile,type AccountProvider} from './account-preferences';
import {AGENT_PREFERENCES_KEY,AGENT_PREFERENCES_EVENT} from './agent-preferences';
import {TERMINAL_PREFERENCES_KEY,TERMINAL_PREFERENCES_EVENT,readTerminalPreferences,parseTerminalPreferences} from './terminal-preferences';
import {TerminalSettingsFields} from './TerminalSettings';
import {commitSettings,type SettingsWrite} from './settings-storage';
import {parseTheme,type Theme} from './ui-preferences';
import './app-settings.css';
import './agent-compact-settings.css';
import './settings-options.css';

type Check={state:'checking'|'available'|'missing'|'error';version?:string};
const rawSetting=(key:string)=>{try{return localStorage.getItem(key)}catch{return null}};
export default function AppSettings({onClose,onNavigate,onLogin,canLogin=false,onDirtyChange,notice}:{onClose:()=>void;onNavigate?:(destination:SettingsDestination)=>void;onLogin?:(profile:AccountProfile|{provider:AccountProvider})=>Promise<void>;canLogin?:boolean;onDirtyChange?:(dirty:boolean)=>void;notice?:string}){
  const ask=useConfirm();
  const [target]=useState(currentTarget);
  const heading=useRef<HTMLHeadingElement>(null),{preferences}=useAgentPreferences(target);
  const [initial]=useState(()=>({agent:preferences,workbench:readWorkbenchPreferences(),terminal:readTerminalPreferences(),accounts:readAccountPreferences(target),theme:parseTheme((()=>{try{return JSON.parse(rawSetting('velella:ui:theme')||'null')}catch{return null}})()),raw:Object.fromEntries([namespacedKey(target,AGENT_PREFERENCES_KEY),WORKBENCH_PREFERENCES_KEY,TERMINAL_PREFERENCES_KEY,namespacedKey(target,ACCOUNT_PREFERENCES_KEY),'velella:ui:theme'].map(key=>[key,rawSetting(key)]))}));
  const [workbenchDraft,setWorkbenchDraft]=useState(initial.workbench),[terminalDraft,setTerminalDraft]=useState(initial.terminal),[accountDraft,setAccountDraft]=useState(initial.accounts),[themeDraft,setThemeDraft]=useState<Theme>(initial.theme);
  const [draft,setDraft]=useState<AgentPreferences>(()=>preferences),[error,setError]=useState('');
  const [checks,setChecks]=useState<Partial<Record<AgentId,Check>>>({}),[refresh,setRefresh]=useState(0);
  const [page,setPage]=useState<'agents'|'permissions'|'orchestration'|'terminal'|ExtendedSettingsPage>('agents'),[permissionSupport,setPermissionSupport]=useState<boolean|null>(null);
  useEffect(()=>{let live=true;void rpc({cmd:'protocol'},target).then(result=>{if(live)setPermissionSupport(result.capabilities?.includes('terminal-permissions-v1')===true)}).catch(()=>{if(live)setPermissionSupport(false)});return()=>{live=false}},[target]);
  useEffect(()=>{heading.current?.focus({preventScroll:true});heading.current?.closest('.app-settings-content')?.scrollTo(0,0)},[page]);
  useEffect(()=>{let live=true;const scan=new AbortController();
    for(const agent of agentCatalog.filter(agent=>agent.check)){
      setChecks(old=>({...old,[agent.id]:{state:'checking'}}));
      void scheduleAgentCheck(namespacedKey(target,'agent.check'),()=>rpc({cmd:'agent.check',agent:agent.id},target),scan.signal).then(result=>{if(live)setChecks(old=>({...old,[agent.id]:{state:result.available?'available':'missing',version:result.version||undefined}}))}).catch(()=>{if(live)setChecks(old=>({...old,[agent.id]:{state:'error'}}))});
    }
    return()=>{live=false;scan.abort()};
  },[refresh,target]);
  function enable(id:AgentId,enabled:boolean){setDraft(old=>parseAgentPreferences({...old,disabled:enabled?old.disabled.filter(value=>value!==id):[...old.disabled,id]}))}
  function setDefault(id:AgentPreferences['defaultAgent']){setDraft(old=>({...old,defaultAgent:id,disabled:old.disabled.filter(value=>value!==id)}))}
  const checking=Object.values(checks).some(check=>check?.state==='checking');
  const serialized=JSON.stringify;
  const changed=serialized(draft)!==serialized(initial.agent)||serialized(workbenchDraft)!==serialized(initial.workbench)||serialized(terminalDraft)!==serialized(initial.terminal)||serialized(accountDraft)!==serialized(initial.accounts)||themeDraft!==initial.theme;
  useEffect(()=>{onDirtyChange?.(changed);return()=>onDirtyChange?.(false)},[changed,onDirtyChange]);
  function saveAll(){
    const writes:SettingsWrite[]=[];
    function add(key:string,before:unknown,after:unknown,event:string){if(serialized(before)!==serialized(after))writes.push({key,before:initial.raw[key],after:serialized(after),event})}
    add(namespacedKey(target,AGENT_PREFERENCES_KEY),initial.agent,parseAgentPreferences(draft),AGENT_PREFERENCES_EVENT);
    add(WORKBENCH_PREFERENCES_KEY,initial.workbench,parseWorkbenchPreferences(workbenchDraft),WORKBENCH_PREFERENCES_EVENT);
    add(TERMINAL_PREFERENCES_KEY,initial.terminal,parseTerminalPreferences(terminalDraft),TERMINAL_PREFERENCES_EVENT);
    add(namespacedKey(target,ACCOUNT_PREFERENCES_KEY),initial.accounts,parseAccountPreferences(accountDraft),ACCOUNT_PREFERENCES_EVENT);
    add('velella:ui:theme',initial.theme,themeDraft,WORKBENCH_PREFERENCES_EVENT);
    commitSettings(writes);onClose();
  }
  const extended=page!=='agents'&&page!=='permissions'&&page!=='orchestration';
  async function navigate(destination:SettingsDestination){if((destination==='tools'||destination==='project')&&!canLogin){setError('请先打开一个可用项目工作区，再使用这个入口。');return}if(changed&&!await ask('离开设置并放弃未保存的更改？'))return;onNavigate?.(destination)}
  return <section className="app-settings app-settings-page" aria-labelledby="app-settings-title">
    <form onSubmit={event=>{event.preventDefault();try{saveAll()}catch(error){setError(`设置未能保存：${String(error)}`)}}}>
      <div className="app-settings-layout">
        <nav className="app-settings-nav" aria-label="设置分类">
          <Button type="button" className="settings-page-back" onClick={onClose}><ArrowLeft size={16}/><span>返回工作界面</span></Button>
          <div className="settings-nav-brand"><h2 id="app-settings-title">设置</h2></div>
          <span className="settings-nav-caption">偏好设置</span>
          <Button type="button" aria-label="Agent" aria-current={page==='agents'?'page':undefined} onClick={()=>setPage('agents')}><Blocks size={18}/><span>Agent</span></Button>
          <Button type="button" aria-label="权限" aria-current={page==='permissions'?'page':undefined} onClick={()=>setPage('permissions')}><ShieldCheck size={16}/><span>权限</span></Button>
          <Button type="button" aria-label="编排" aria-current={page==='orchestration'?'page':undefined} onClick={()=>setPage('orchestration')}><Workflow size={18}/><span>编排</span></Button>
          {([{id:'accounts',label:'AI 账号',Icon:Users},{id:'appearance',label:'外观',Icon:Palette},{id:'editing',label:'输入与编辑',Icon:TextCursorInput},{id:'terminal',label:'终端',Icon:Terminal},{id:'shortcuts',label:'快捷键',Icon:Keyboard},{id:'notifications',label:'通知',Icon:Bell},{id:'connections',label:'连接与工具',Icon:Plug}] as const).map(item=><Button key={item.id} type="button" title={item.label} aria-label={item.label} aria-current={page===item.id?'page':undefined} onClick={()=>setPage(item.id)}><item.Icon size={18}/><span>{item.label}</span></Button>)}
          <div className="settings-host" title="Agent 与账号选择按执行主机保存；界面偏好保存在这台桌面">{target.kind==='local'?<Monitor size={16}/>:<Server size={16}/>}<div><strong>{target.kind==='local'?'本机':'当前远端主机'}</strong><span>Agent 与账号按主机保存</span></div></div>
        </nav>
        <div className="app-settings-main">
          <section className="app-settings-content" aria-labelledby={extended?'extended-settings-title':page==='agents'?'agent-settings-title':page==='permissions'?'permission-settings-title':'orchestration-settings-title'}>
          {extended?<><header className="app-settings-intro"><h3 ref={heading} tabIndex={-1} id="extended-settings-title">{page==='terminal'?'终端':settingsPageCopy[page as ExtendedSettingsPage].title}</h3><p>{page==='terminal'?'保存后应用到这台桌面的所有终端，保留运行中的会话。':settingsPageCopy[page as ExtendedSettingsPage].description}</p></header>{page==='terminal'?<TerminalSettingsFields value={terminalDraft} onChange={setTerminalDraft}/>:<SettingsPages page={page as ExtendedSettingsPage} value={workbenchDraft} onChange={setWorkbenchDraft} theme={themeDraft} onTheme={setThemeDraft} accounts={accountDraft} onAccounts={setAccountDraft} target={target} onNavigate={onNavigate?destination=>void navigate(destination):undefined} onLogin={onLogin} canLogin={canLogin&&!changed}/>} {page==='accounts'&&changed&&<p>请先保存设置，再打开登录终端。</p>}</>:page==='orchestration'?<><header className="app-settings-intro"><h3 ref={heading} tabIndex={-1} id="orchestration-settings-title">编排</h3><p>连接鱼群 CLI，设置新建协作的默认方式。</p></header><OrchestrationSettings target={target} value={draft.orchestration||{}} onChange={orchestration=>setDraft(old=>parseAgentPreferences({...old,orchestration}))}/></>:<>
          {page==='agents'?<div className="agent-compact-page">
          <header className="app-settings-intro"><h3 ref={heading} tabIndex={-1} id="agent-settings-title">Agent</h3><p>选择常用工具，设置新终端的默认启动方式。</p></header>
          <section className="agent-compact-default">
            <div><label htmlFor="default-agent">默认启动</label><p>新建终端时优先选择</p></div>
            <AgentDefaultPicker id="default-agent" value={draft.defaultAgent} disabled={draft.disabled} onChange={setDefault}/>
          </section>
          <div className="agent-list-heading"><div><h4>工具清单</h4><span>{agentCatalog.length-draft.disabled.length} / {agentCatalog.length} 已启用</span></div><Button type="button" disabled={checking} onClick={()=>setRefresh(value=>value+1)}><RefreshCw size={13} className={checking?'loading-icon':undefined}/>重新检测</Button></div>
          <div className="agent-compact-table-wrap"><table className="agent-compact-table">
            <caption className="sr-only">Agent 工具清单</caption>
            <thead><tr><th scope="col">工具</th><th scope="col" className="agent-command-cell">启动命令</th><th scope="col">状态</th><th scope="col">启用</th></tr></thead>
            <tbody>
            {agentCatalog.map(agent=>{const enabled=!draft.disabled.includes(agent.id),check=checks[agent.id],selected=draft.defaultAgent===agent.id;
              const status=!agent.check?'未检测':!check||check.state==='checking'?'检测中…':check.state==='available'?'已检测到':check.state==='missing'?'未检测到':'检测失败';
              return <tr className={!enabled?'is-disabled':undefined} key={agent.id}>
                <th scope="row"><div className="agent-compact-identity"><AgentLogo agent={agent.id} size={24}/><div><strong>{agent.name}</strong>{selected&&<span className="agent-compact-default-label">默认</span>}<code className="agent-mobile-command">{agent.program}</code></div></div></th>
                <td className="agent-command-cell"><code>{agent.program}</code></td>
                <td>{agent.check&&(!check||check.state==='checking')?<LoadingState compact label="检测中"/>:<span className="agent-check-status" data-state={check?.state||'unknown'} title={check?.version}>{status}</span>}</td>
                <td><label className="agent-enable-toggle"><input type="checkbox" role="switch" aria-label={`启用 ${agent.name}`} checked={enabled} onChange={event=>enable(agent.id,event.target.checked)}/><span aria-hidden="true"/></label></td>
              </tr>;
            })}
            </tbody><tfoot><tr><td colSpan={4}>仅显示已启用工具的新建入口</td></tr></tfoot>
          </table></div>
          <p className="agent-compact-note">关闭入口不会结束已有会话。Codex、Claude 和 MiniMax Code 支持自动检测。MiniMax 使用 mcode login 登录；其余工具请确认已安装。</p>
          </div>:<>
          <header className="app-settings-intro"><h3 ref={heading} tabIndex={-1} id="permission-settings-title">默认权限</h3><p>为每个 Agent 选择新开终端时使用的权限。</p></header>
          <p className="permission-scope-note">仅应用于当前主机的新终端。已有会话和恢复会话保留原权限；受限与受管任务使用自身策略。</p>
          {permissionSupport===null?<LoadingState compact label="正在检查执行主机" className="permission-support-note"/>:permissionSupport!==true&&<p className="permission-support-note" role="status">当前后台尚不支持权限覆盖，请升级后台后使用。可将已有选择恢复为“沿用 CLI 设置”。</p>}
          <section className="agent-default-setting"><div className="agent-default-preview"><span className="agent-default-icon"><ShieldCheck size={22}/></span><div><label htmlFor="permission-default">全局默认权限</label><p>用于没有单独覆盖的 Agent</p></div></div>
            <div className="agent-default-select"><select id="permission-default" value={draft.permissionDefault||'inherit'} onChange={event=>setDraft(old=>withPermissionDefault(old,event.target.value as GlobalPermissionMode))}>
              {globalPermissionModes.map(option=><option key={option.id} value={option.id} disabled={option.id!=='inherit'&&permissionSupport!==true}>{option.label}</option>)}
            </select><ChevronDown size={14} aria-hidden="true"/></div></section>
          <div className="permission-default-heading"><h4>Agent 权限覆盖</h4><span>{agentCatalog.filter(agent=>permissionOverride(draft,agent.id)!==undefined).length} 个单独设置</span></div>
          <p className="permission-exceptions">单个 Agent 的选择优先于全局默认。不支持所选全局模式的工具保留原选择，并显示为单独设置。</p>
          <div className="agent-permissions-list">
            {agentCatalog.map(agent=>{const {mode,source}=resolvedAgentPermission(draft,agent.id),override=permissionOverride(draft,agent.id),options=permissionModesFor(agent.id),selected=options.find(option=>option.id===mode);
              return <div className="agent-permission-row" key={agent.id}>
                <span className="agent-logo-frame"><AgentLogo agent={agent.id} size={22}/></span>
                <div className="agent-permission-control"><label htmlFor={`permission-${agent.id}`}>{agent.name}<span className="permission-source-label">{source==='agent'?'单独设置 · 覆盖全局':'跟随全局默认'}</span></label>
                  <div className="agent-default-select"><select id={`permission-${agent.id}`} aria-describedby={`permission-note-${agent.id}`} value={override??'default'} onChange={event=>setDraft(old=>{const permissions={...old.permissions};if(event.target.value==='default')delete permissions[agent.id];else permissions[agent.id]=event.target.value as AgentPermissionMode;return parseAgentPreferences({...old,permissions})})}>
                    <option value="default" disabled={globalPermissionFor(agent.id,draft.permissionDefault||'inherit')===undefined}>跟随全局默认</option>
                    {options.map(option=><option key={option.id} value={option.id} disabled={option.id!=='inherit'&&permissionSupport!==true}>{option.label}</option>)}
                  </select><ChevronDown size={14} aria-hidden="true"/></div>
                  <p id={`permission-note-${agent.id}`} data-unattended={selected?.unattended||undefined}>{selected?`${selected.label}：${selected.description}`:'此工具不支持当前全局模式，请单独选择。'}</p>
                  {mode&&<details className="permission-row-details"><summary>权限效果</summary><PermissionDetails agent={agent.id} mode={mode}/></details>}
                </div>
              </div>;
            })}
          </div>
          <p className="agent-settings-note">默认不覆盖 CLI 配置。启动前会检查当前工具是否支持所选参数；不会改写工具的全局配置。</p>
          </>}
          </>}
          {error&&<p className="error" role="alert">{error}</p>}
          {notice&&<p className="error" role="alert">{notice}</p>}
          </section>
          <footer className="app-settings-footer"><span data-changed={changed}>{changed?'有未保存的更改':page==='orchestration'?'仅影响新建协作':page==='agents'||page==='permissions'||page==='accounts'?'仅影响新开的原生终端':'偏好保存在这台桌面'}</span><Button type="button" onClick={onClose}>取消</Button><Button className="primary" type="submit">保存设置</Button></footer>
        </div>
      </div>
    </form>
  </section>;
}
