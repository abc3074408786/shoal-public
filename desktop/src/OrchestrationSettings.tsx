import {useEffect,useState} from 'react';
import {Copy,RefreshCw,Terminal} from 'lucide-react';
import {rpc} from './api';
import {agentCatalog,type AgentId,type OrchestrationDefaults} from './agent-preferences';
import AgentLogo from './AgentLogo';
import LoadingState from './LoadingState';
import type {ExecutionTarget} from './execution-target';
import {Button} from './ui';
import './orchestration-settings.css';

type Provider={id:string;installed:boolean};
type Template={id:string;name:string;revision:number;plan:{parallelism:number;roles:unknown[];nodes:unknown[]}};
type CliInspection={found:boolean;path?:string|null;source?:'path'|'installation'|null};
const guides=[
  {label:'检查版本',command:'velella-cli --version'},
  {label:'检查连接',command:'velella-cli status --json'},
  {label:'查看编排',command:'velella-cli swarm list --json'},
  {label:'Agent 协作指南',command:'velella-cli skills get velella-collaboration'},
];

export default function OrchestrationSettings({target,value,onChange}:{target:ExecutionTarget;value:OrchestrationDefaults;onChange:(value:OrchestrationDefaults)=>void}){
  const [data,setData]=useState<{providers:Provider[];templates:Template[];cli:CliInspection|null}|null>(null);
  const [error,setError]=useState(''),[refresh,setRefresh]=useState(0),[checking,setChecking]=useState(true),[copyStatus,setCopyStatus]=useState('');
  async function copy(text:string){try{await navigator.clipboard.writeText(text);setCopyStatus('已复制命令')}catch{setCopyStatus('复制失败，请手动选择文字复制')}}
  useEffect(()=>{
    if(target.kind!=='local')return;
    let live=true;setError('');setChecking(true);
    void Promise.all([
      rpc<{providers:Provider[]}>({cmd:'orchestration.capabilities'},target),
      rpc<{templates:Template[]}>({cmd:'orchestration.templates.list'},target),
      // Old runtimes may configure defaults without CLI file inspection.
      rpc<CliInspection>({cmd:'cli.inspect'},target).catch(()=>null),
    ]).then(([capabilities,templates,cli])=>{
      if(!Array.isArray(capabilities.providers)||!Array.isArray(templates.templates))throw Error('当前后台未提供可用的编排配置');
      if(live)setData({providers:capabilities.providers,templates:templates.templates,cli:cli&&typeof cli.found==='boolean'?cli:null});
    }).catch(error=>{if(live)setError(String(error))}).finally(()=>{if(live)setChecking(false)});
    return()=>{live=false};
  },[target,refresh]);
  if(target.kind!=='local')return <p className="orchestration-settings-notice" role="status">鱼群 CLI 编排配置目前仅支持本机，请切回本机后配置。</p>;
  if(!data)return error?<div role="alert"><p>无法读取编排配置：{error}</p><Button type="button" onClick={()=>setRefresh(old=>old+1)}>重新检查</Button></div>:<LoadingState label="正在读取编排配置"/>;
  const providers=data.providers.filter(provider=>agentCatalog.some(agent=>agent.id===provider.id));
  const template=data.templates.find(template=>template.id===value.templateId);
  const missingProvider=!!value.provider&&!providers.some(provider=>provider.id===value.provider),missingTemplate=!!value.templateId&&!template;
  return <div className="orchestration-settings-fields">
    <section className="orchestration-cli-panel" aria-labelledby="orchestration-cli-title">
      <header><div className="orchestration-cli-identity"><Terminal size={20} aria-hidden="true"/><div><h4 id="orchestration-cli-title">鱼群 CLI</h4><code>velella-cli</code></div></div><Button type="button" disabled={checking} onClick={()=>setRefresh(old=>old+1)}><RefreshCw size={13} className={checking?'loading-icon':undefined}/>{checking?'检查中…':'重新检查'}</Button></header>
      <p>在终端里管理鱼群，创建编排、派发任务，让多个 Agent 协作。</p>
      <dl className="orchestration-cli-status">
        <div><dt>CLI 文件</dt><dd>{data.cli?(data.cli.found?'已找到':'未找到'):'暂无法检测'}</dd></div>
        <div><dt>编排后台</dt><dd>{error?'检查失败':'已连接'}</dd></div>
      </dl>
      {error&&<p role="alert" className="orchestration-settings-warning">重新检查失败：{error}。下方保留上次读取的配置。</p>}
      {data.cli?.found?<details className="orchestration-cli-location"><summary>查看 CLI 路径</summary><code>{data.cli.path}</code><p>{data.cli.source==='path'?'来自后台进程的 PATH。':'来自标准安装目录，当前后台 PATH 中未找到；可用此绝对路径运行。'}这里只检查文件，版本与命令连接可在终端中确认。</p></details>:<p className="orchestration-cli-hint">{data.cli?'后台 PATH 和标准安装目录中未找到 CLI；如已自定义安装，请在终端检查。':'当前后台未提供 CLI 文件检测；可在终端运行下方的版本与连接命令。'}</p>}
    </section>

    <section className="orchestration-defaults" aria-labelledby="orchestration-defaults-title">
      <h4 id="orchestration-defaults-title">新建协作</h4><p className="orchestration-section-help">用于界面中新建协作的预填，可在创建时调整。</p>
      <div className="orchestration-setting-row"><div><label htmlFor="orchestration-provider">默认主控</label><p id="orchestration-provider-note">理解目标并生成协作方案。</p></div><select id="orchestration-provider" aria-describedby="orchestration-provider-note" value={value.provider||''} onChange={event=>onChange({...value,provider:event.target.value?event.target.value as AgentId:undefined})}>
        <option value="">沿用新建窗口默认值</option>{missingProvider&&<option value={value.provider} disabled>当前主控不可用 · {value.provider}</option>}{providers.map(provider=><option key={provider.id} value={provider.id}>{agentCatalog.find(agent=>agent.id===provider.id)!.name}{provider.installed?'':' · 未检测到'}</option>)}
      </select></div>
      <div className="orchestration-setting-row"><div><label htmlFor="orchestration-template">工作流模板</label><p id="orchestration-template-note">复用已保存的角色与步骤。</p></div><select id="orchestration-template" aria-describedby="orchestration-template-note" value={value.templateId||''} onChange={event=>onChange({...value,templateId:event.target.value||undefined})}>
        <option value="">按目标生成</option>{missingTemplate&&<option value={value.templateId} disabled>所选模板已不可用</option>}{data.templates.map(template=><option key={template.id} value={template.id}>{template.name} · v{template.revision}</option>)}
      </select></div>
      {(missingProvider||missingTemplate)&&<p className="orchestration-settings-warning" role="status">当前默认值已不可用；创建协作前需要重新选择。</p>}
      {template?<div className="orchestration-template-summary" aria-label="模板摘要"><span>{template.name}</span><span>{template.plan.roles.length} 个角色 · {template.plan.nodes.length} 个步骤 · 最多并行 {template.plan.parallelism}</span></div>:!data.templates.length&&<p className="orchestration-section-help">还没有模板，可在编排工作页将方案保存为模板。</p>}
    </section>

    <details className="orchestration-providers"><summary>主控工具 <span>{providers.filter(provider=>provider.installed).length} / {providers.length} 已检测到</span></summary>
      <div className="orchestration-provider-list" role="list" aria-label="编排主控工具">{providers.map(provider=><div role="listitem" key={provider.id}><AgentLogo agent={provider.id as AgentId} size={18}/><strong>{agentCatalog.find(agent=>agent.id===provider.id)!.name}</strong><span>{provider.installed?'已检测到':'未检测到'}</span></div>)}</div>
      <p className="orchestration-section-help">检测到工具不代表已登录或完成协作验证；启动时仍需检查工具自身配置。</p>
    </details>

    <details className="orchestration-cli-guide"><summary>CLI 用法与 Skill 指南</summary>
      <dl className="orchestration-command-groups"><div><dt><code>swarm</code></dt><dd>创建编排、确认方案、暂停、恢复与验收。</dd></div><div><dt><code>collab</code></dt><dd>成员加入、会话消息、任务派发与回执。</dd></div></dl>
      <p className="orchestration-section-help">Skill 为 Agent 提供鱼群 CLI 的使用指南。以下操作仅复制命令，请在自己的终端运行。</p>
      <div className="orchestration-command-list">{guides.map(guide=><div key={guide.command}><span>{guide.label}</span><code>{guide.command}</code><Button type="button" aria-label={`复制${guide.label}命令`} onClick={()=>void copy(guide.command)}><Copy size={13}/></Button></div>)}</div>
    </details>
    <p className="orchestration-copy-status" role="status">{copyStatus}</p>
  </div>;
}
