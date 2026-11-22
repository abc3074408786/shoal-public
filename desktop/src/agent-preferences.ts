export const agentCatalog = [
  {id:'codex',name:'Codex',program:'codex',check:true},
  {id:'claude-code',name:'Claude Code',program:'claude',check:true},
  {id:'gemini',name:'Gemini CLI',program:'gemini',check:false},
  {id:'opencode',name:'OpenCode',program:'opencode',check:false},
  {id:'omp',name:'Oh My Pi (OMP)',program:'omp',check:false},
  {id:'kimi',name:'Kimi',program:'kimi',check:false},
  {id:'zcode',name:'ZCode',program:'zcode-free',check:false},
  {id:'minimax',name:'MiniMax Code',program:'mcode',check:true},
] as const;
export type AgentId=typeof agentCatalog[number]['id'];
export type AgentPermissionMode='inherit'|'ask'|'accept-edits'|'auto'|'full-access'|'always-ask'|'always-ask-cached'|'write'|'yolo'|'guarded'|'build'|'edit';
export type PermissionOption={id:AgentPermissionMode;label:string;description:string;unattended?:boolean};
const inherit:PermissionOption={id:'inherit',label:'沿用 CLI 设置',description:'不添加权限覆盖，使用该工具自身的配置。'};
const permissionCatalog:Record<AgentId,readonly PermissionOption[]>={
  minimax:[inherit],
  codex:[inherit,{id:'ask',label:'手动确认 · 只读',description:'使用只读沙箱，由你审批需要提升权限的操作。'},{id:'auto',label:'自动审批 · 工作区',description:'使用 Codex 自动审批与工作区写入沙箱。',unattended:true},{id:'full-access',label:'完全访问',description:'跳过审批和 Codex 沙箱限制。',unattended:true}],
  'claude-code':[inherit,{id:'ask',label:'手动确认',description:'使用 Claude 的 manual 权限模式。'},{id:'accept-edits',label:'自动接受编辑',description:'自动接受文件编辑，其他操作按 CLI 规则处理。'},{id:'auto',label:'自动审批',description:'使用 Claude 的 auto 权限模式。',unattended:true},{id:'full-access',label:'跳过权限确认',description:'跳过 Claude 的权限检查。',unattended:true}],
  gemini:[inherit,{id:'full-access',label:'自动批准工具 · YOLO',description:'通过 Gemini 的 --yolo 启动；需安装可用的 CLI。',unattended:true}],
  opencode:[inherit,{id:'ask',label:'默认询问',description:'将默认规则设为 ask；专属 Agent 规则仍可覆盖。'},{id:'auto',label:'自动批准 · 保留拒绝',description:'批准未被明确拒绝的请求，保留原配置的拒绝规则。',unattended:true}],
  omp:[inherit,{id:'always-ask-cached',label:'读写均需授权',description:'未授权的读取、写入和执行需要确认，可记住读取授权。'},{id:'always-ask',label:'读取自动 · 写入确认',description:'自动批准读取，写入和执行需要确认。'},{id:'write',label:'读写自动 · 执行确认',description:'自动批准读取与写入，执行命令需要确认。'},{id:'yolo',label:'全部自动批准',description:'使用 OMP 的 yolo 审批模式。',unattended:true}],
  kimi:[inherit,{id:'yolo',label:'自动批准工具 · YOLO',description:'自动批准常规工具调用，Agent 仍可能提问。',unattended:true},{id:'auto',label:'完全自主 · Auto',description:'自动批准并自主运行，不向用户提问。',unattended:true}],
  zcode:[inherit,{id:'guarded',label:'受控模式 · Guarded',description:'使用 ZCode 的 guarded 交互权限模式。'},{id:'build',label:'构建模式 · Build',description:'使用 ZCode 的 build 交互权限模式。'},{id:'edit',label:'编辑模式 · Edit',description:'使用 ZCode 的 edit 交互权限模式。'},{id:'yolo',label:'跳过审批 · YOLO',description:'使用 ZCode 的 yolo 模式；保留 zcode-free 模型限制。',unattended:true}],
};
export const permissionModesFor=(id:AgentId)=>permissionCatalog[id];
export type GlobalPermissionMode='inherit'|'ask'|'auto'|'full-access';
export const globalPermissionModes:readonly PermissionOption[]=[inherit,{id:'ask',label:'手动确认',description:'为支持的工具使用询问模式。'},{id:'auto',label:'自动审批',description:'使用工具自身的自动审批模式。'},{id:'full-access',label:'完全访问 / YOLO',description:'为支持的工具跳过审批；具体隔离范围由工具决定。',unattended:true}];
export type OrchestrationDefaults={provider?:AgentId;templateId?:string};
export type AgentPreferences={defaultAgent:'auto'|'terminal'|AgentId;disabled:AgentId[];permissionDefault?:GlobalPermissionMode;permissions?:Partial<Record<AgentId,AgentPermissionMode>>;orchestration?:OrchestrationDefaults};
export function permissionOverride(preferences:AgentPreferences,id:AgentId):AgentPermissionMode|undefined{
  const mode=preferences.permissions?.[id];return permissionCatalog[id].some(option=>option.id===mode)?mode:undefined;
}
export function globalPermissionFor(id:AgentId,mode:GlobalPermissionMode):AgentPermissionMode|undefined{
  if(permissionCatalog[id].some(option=>option.id===mode))return mode;
  if(id==='omp'&&mode==='ask')return 'always-ask-cached';
  if(['omp','kimi','zcode'].includes(id)&&mode==='full-access')return 'yolo';
  return undefined;
}
export function resolvedAgentPermission(preferences:AgentPreferences,id:AgentId){
  const override=permissionOverride(preferences,id),global=preferences.permissionDefault||'inherit';
  const mode=override??globalPermissionFor(id,global);
  return {mode,source:override!==undefined?'agent' as const:'global' as const};
}
export function agentPermission(preferences:AgentPreferences,id:AgentId):AgentPermissionMode{
  const {mode}=resolvedAgentPermission(preferences,id);
  if(!mode)throw new Error(`${agentCatalog.find(agent=>agent.id===id)!.name} 不支持当前全局权限，请在设置中为它单独选择权限。`);
  return mode;
}
export function withPermissionDefault(preferences:AgentPreferences,mode:GlobalPermissionMode):AgentPreferences{
  const permissions={...preferences.permissions};
  for(const agent of agentCatalog)if(permissionOverride(preferences,agent.id)===undefined&&globalPermissionFor(agent.id,mode)===undefined)permissions[agent.id]=resolvedAgentPermission(preferences,agent.id).mode??'inherit';
  return parseAgentPreferences({...preferences,permissionDefault:mode,permissions});
}
export const AGENT_PREFERENCES_KEY='velella:agents:v1';
export const AGENT_PREFERENCES_EVENT='velella:agent-preferences';
const isAgent=(id:unknown):id is AgentId=>agentCatalog.some(agent=>agent.id===id);
export function parseAgentPreferences(value:unknown):AgentPreferences {
  const source=(value&&typeof value==='object'?value:{}) as Partial<AgentPreferences>;
  const disabled=Array.isArray(source.disabled)?[...new Set(source.disabled.filter(isAgent))]:[];
  const chosen=source.defaultAgent;
  const defaultAgent=chosen==='terminal'||chosen==='auto'||isAgent(chosen)&&!disabled.includes(chosen)?chosen!:'auto';
  const permissions:Partial<Record<AgentId,AgentPermissionMode>>={};
  for(const agent of agentCatalog){const mode=permissionOverride(source as AgentPreferences,agent.id);if(mode!==undefined)permissions[agent.id]=mode;}
  const permissionDefault=globalPermissionModes.some(option=>option.id===source.permissionDefault)?source.permissionDefault:undefined;
  const raw=source.orchestration&&typeof source.orchestration==='object'&&!Array.isArray(source.orchestration)?source.orchestration:{};
  const orchestration:OrchestrationDefaults={};
  if(isAgent(raw.provider))orchestration.provider=raw.provider;
  if(typeof raw.templateId==='string'&&/^[a-zA-Z0-9:_-]{1,200}$/.test(raw.templateId))orchestration.templateId=raw.templateId;
  return {defaultAgent,disabled,...(permissionDefault&&permissionDefault!=='inherit'?{permissionDefault}:{}),...(Object.keys(permissions).length?{permissions}:{}),...(Object.keys(orchestration).length?{orchestration}:{})};
}
export function defaultAgentLaunch(preferences:AgentPreferences):AgentId|'terminal' {
  const value=parseAgentPreferences(preferences);
  return value.defaultAgent==='auto'?agentCatalog.find(agent=>!value.disabled.includes(agent.id))?.id||'terminal':value.defaultAgent;
}
export function enabledAgents(preferences:AgentPreferences) {
  const value=parseAgentPreferences(preferences),preferred=defaultAgentLaunch(value);
  return agentCatalog.filter(agent=>!value.disabled.includes(agent.id)).sort((a,b)=>Number(b.id===preferred)-Number(a.id===preferred));
}
