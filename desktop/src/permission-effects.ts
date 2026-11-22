import {agentCatalog,permissionModesFor,type AgentId,type AgentPermissionMode} from './agent-preferences';
export type PermissionEffect={label:string;value:string};
export function permissionEffects(agent:AgentId,mode:AgentPermissionMode):PermissionEffect[]{
 const native='由 CLI 配置与操作系统权限决定';
 if(mode==='inherit')return ['文件读取','文件写入','命令执行','网络与目录'].map(label=>({label,value:'沿用工具配置，应用不覆盖'}));
 if(agent==='codex'){
  const manual=mode==='ask',auto=mode==='auto';
  return [{label:'文件读取',value:manual?'只读沙箱内允许':'按 Codex 沙箱与配置'},
   {label:'文件写入',value:manual?'需申请提升权限':auto?'工作区写入沙箱内允许':'不使用 Codex 沙箱限制'},
   {label:'命令执行',value:manual?'按 on-request 申请提升':auto?'由 Codex 自动审查审批请求':'跳过审批与沙箱'},
   {label:'网络与目录',value:mode==='full-access'?'仅受操作系统及外部环境限制':'保留 CLI 的网络、目录及沙箱规则'}];
 }
 if(agent==='omp')return [{label:'文件读取',value:mode==='always-ask-cached'?'未授权路径需确认，可记住授权':'自动批准'},
  {label:'文件写入',value:['write','yolo'].includes(mode)?'自动批准':'需要确认'},
  {label:'命令执行',value:mode==='yolo'?'自动批准':'需要确认'},
  {label:'网络与目录',value:native+'；审批模式不创建系统沙箱'}];
 if(agent==='claude-code')return [{label:'文件读取',value:'按 Claude 工具规则与目录配置'},
  {label:'文件写入',value:mode==='accept-edits'?'自动接受编辑':mode==='full-access'?'跳过权限检查':mode==='auto'?'由 Claude 自动审批':'使用 manual 模式确认'},
  {label:'命令执行',value:mode==='accept-edits'?'仍按 Claude 审批规则':mode==='full-access'?'跳过权限检查':mode==='auto'?'由 Claude 自动审批':'使用 manual 模式确认'},
  {label:'网络与目录',value:native+'；权限模式本身不等于系统隔离'}];
 if(agent==='opencode')return [{label:'工具调用',value:mode==='ask'?'默认 ask；专属 Agent 规则可覆盖':'自动批准未明确拒绝的请求'},
  {label:'已有规则',value:mode==='auto'?'保留配置中的显式拒绝':'合并默认权限规则；不改写配置文件'},
  {label:'网络与目录',value:native+'；不提供系统沙箱'}];
 if(agent==='kimi')return [{label:'工具调用',value:'自动批准常规工具'}, {label:'向你提问',value:mode==='auto'?'自主运行，不主动询问':'仍可能提问'}, {label:'网络与目录',value:native}];
 if(agent==='gemini')return [{label:'工具调用',value:'YOLO 自动批准'}, {label:'CLI 状态',value:'需有可用的 Gemini 安装；启动前检查 --yolo'}, {label:'网络与目录',value:native}];
 return [{label:'交互模式',value:`ZCode ${mode} 模式；具体审批由工具运行时执行`},
  {label:'权限范围',value:mode==='yolo'?'跳过工具审批':'使用该模式的原生规则，不额外承诺读写隔离'},
  {label:'模型与账号',value:'保留 zcode-free 入口和原模型限制'}, {label:'网络与目录',value:native}];
}
export function permissionRecipe(agent:AgentId,mode:AgentPermissionMode){
 const program=agentCatalog.find(value=>value.id===agent)!.program;
 if(mode==='inherit')return `${program}（不添加权限参数）`;
 if(agent==='opencode'&&mode==='ask')return 'OPENCODE_PERMISSION={"*":"ask"} opencode';
 let args:string;
 if(agent==='codex')args=mode==='ask'?'-a on-request -s read-only -c approvals_reviewer=user':mode==='auto'?'--approve-for-me':'--dangerously-bypass-approvals-and-sandbox';
 else if(agent==='claude-code')args=mode==='full-access'?'--dangerously-skip-permissions':`--permission-mode ${mode==='ask'?'manual':mode==='accept-edits'?'acceptEdits':'auto'}`;
 else if(agent==='omp')args=`--approval-mode ${mode}`;
 else if(agent==='zcode')args=`--mode ${mode}`;
 else args=mode==='auto'?'--auto':'--yolo';
 return `${program} ${args}`;
}
export function permissionLabel(agent:AgentId,mode:AgentPermissionMode){return permissionModesFor(agent).find(option=>option.id===mode)!.label}
