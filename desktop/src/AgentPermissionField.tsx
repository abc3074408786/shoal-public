import {useId} from 'react';
import {resolvedAgentPermission,permissionModesFor,type AgentId,type AgentPermissionMode,type AgentPreferences} from './agent-preferences';
import {permissionEffects,permissionLabel,permissionRecipe} from './permission-effects';
import './permission-effects.css';
export function PermissionDetails({agent,mode}:{agent:AgentId;mode:AgentPermissionMode}){
 return <div className="permission-details"><dl>{permissionEffects(agent,mode).map(effect=><div key={effect.label}><dt>{effect.label}</dt><dd>{effect.value}</dd></div>)}</dl>
  <details className="permission-recipe"><summary>查看 CLI 启动参数</summary><code>{permissionRecipe(agent,mode)}</code><p>这里只显示权限部分。启动前验证工具支持情况，不改写全局 CLI 配置。</p></details></div>;
}
export default function AgentPermissionField({agent,preferences,value,onChange,disabled=false}:{agent:AgentId;preferences:AgentPreferences;value:'default'|AgentPermissionMode;onChange:(value:'default'|AgentPermissionMode)=>void;disabled?:boolean}){
 const id=useId(),saved=resolvedAgentPermission(preferences,agent).mode,effective=value==='default'?saved:value;
 return <section className="permission-launch-field" aria-label="本次启动权限"><label htmlFor={id}>本次权限</label>
  <select id={id} value={value} disabled={disabled} onChange={event=>onChange(event.target.value as 'default'|AgentPermissionMode)}>
   <option value="default">使用已保存设置 · {saved?permissionLabel(agent,saved):'当前全局模式不受支持'}</option>
   {permissionModesFor(agent).map(option=><option key={option.id} value={option.id}>{option.label}</option>)}
  </select><p>只影响本次新建，不修改默认权限。重试沿用首次提交的选择。</p>{effective?<PermissionDetails agent={agent} mode={effective}/>:<p role="alert">请选择此工具支持的权限模式。</p>}
 </section>;
}
